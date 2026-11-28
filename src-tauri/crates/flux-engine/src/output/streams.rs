use super::*;

/// Runs `open` on a thread of its own, which keeps the stream it returns for as long as the output
/// runs, and waits until the stream is open or has failed. A cpal stream is not `Send` on every
/// platform, so it is created, played and dropped on that one thread. The same thread notices a
/// stream that is lost without an error and, when `watch.retry`, opens it again when the device
/// comes back.
pub(super) fn spawn_stream<I: Send + 'static>(
    name: &str,
    stop: Arc<AtomicBool>,
    watch: Watch,
    mut open: impl FnMut() -> Result<(cpal::Stream, I), OutputError> + Send + 'static,
) -> Result<(JoinHandle<()>, I), OutputError> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            devices::join_mta();
            let (stream, info) = match open() {
                Ok(opened) => opened,
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(info));
            let base = Instant::now();
            let mut stream = Some(stream);
            let mut supervisor = Supervisor::new(base.elapsed(), watch.retry);
            while !stop.load(Ordering::Relaxed) {
                thread::park_timeout(AUDIO_PARK);
                let now = base.elapsed();
                let beats = watch.beat.load(Ordering::Relaxed);
                match supervisor.step(now, beats, watch.lost.load(Ordering::Relaxed)) {
                    Step::Idle => {}
                    Step::Lose => {
                        watch.lost.store(true, Ordering::Relaxed);
                        if watch.retry {
                            stream = None;
                        }
                    }
                    Step::Reopen => match open() {
                        Ok((reopened, _)) => {
                            stream = Some(reopened);
                            watch.lost.store(false, Ordering::Relaxed);
                            supervisor.reopened(base.elapsed());
                        }
                        Err(_) => supervisor.reopen_failed(base.elapsed()),
                    },
                }
            }
            drop(stream);
        })
        .map_err(|err| OutputError::Stream(err.to_string()))?;
    match ready_rx.recv() {
        Ok(Ok(info)) => Ok((thread, info)),
        Ok(Err(err)) => {
            let _ = thread.join();
            Err(err)
        }
        Err(_) => {
            let _ = thread.join();
            Err(OutputError::Stream("the audio thread stopped".into()))
        }
    }
}

/// Starts the stream of an LTC (or click) device that is not the programme's. When its device goes
/// away the thread opens it again as soon as it is back.
pub(super) fn spawn_follower(
    name: &str,
    stop: &Arc<AtomicBool>,
    shared: &Arc<SharedClock>,
    lost: &Arc<AtomicBool>,
    spec: LtcSpec,
) -> Result<(JoinHandle<()>, String), OutputError> {
    let beat: Arc<AtomicU64> = Arc::default();
    let watch = Watch {
        beat: Arc::clone(&beat),
        lost: Arc::clone(lost),
        retry: true,
    };
    let (shared, lost) = (Arc::clone(shared), Arc::clone(lost));
    spawn_stream(name, Arc::clone(stop), watch, move || {
        open_ltc_stream(
            spec.clone(),
            Arc::clone(&shared),
            Arc::clone(&lost),
            Arc::clone(&beat),
        )
    })
}

/// An output device and the stream format chosen for it.
pub(super) struct OpenedDevice {
    device: cpal::Device,
    name: String,
    channels: u16,
    format: SampleFormat,
}

/// Opens the device `hint` names with the fewest channels that cover `needed`, at `rate`.
pub(super) fn open_device(
    hint: &str,
    driver: Option<&str>,
    needed: u16,
    rate: u32,
) -> Result<OpenedDevice, OutputError> {
    let device = devices::open_audio(hint, driver).ok_or_else(|| {
        if hint.is_empty() {
            OutputError::NoDefaultDevice
        } else {
            OutputError::DeviceMissing(hint.to_owned())
        }
    })?;
    let name = device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| hint.to_owned());

    let ranges: Vec<ConfigRange> = device
        .supported_output_configs()
        .map_err(|err| OutputError::Stream(err.to_string()))?
        .map(|config| ConfigRange {
            channels: config.channels(),
            min_rate: config.min_sample_rate(),
            max_rate: config.max_sample_rate(),
            format: config.sample_format(),
        })
        .collect();
    let (channels, format) =
        pick_config(&ranges, needed, rate).ok_or_else(|| OutputError::Unsupported {
            device: name.clone(),
            channels: needed,
            rate,
        })?;
    Ok(OpenedDevice {
        device,
        name,
        channels,
        format,
    })
}

pub(super) fn operator_channels(
    shared: &SharedClock,
    device: &str,
    driver: Option<&str>,
) -> Vec<usize> {
    if !shared.audio_config.audio_routes.is_empty() {
        return Vec::new();
    }
    shared
        .operator_route
        .as_ref()
        .filter(|r| same_device(driver, &r.device_name, device))
        .map_or_else(Vec::new, |r| {
            r.channels.iter().map(|c| usize::from(*c) - 1).collect()
        })
}

pub(super) fn mark_audio_routes(
    shared: &SharedClock,
    device: &str,
    driver: Option<&str>,
    channels: u16,
    lost: &Arc<AtomicBool>,
) {
    use crate::audio_routing::AudioRouteStatus;
    for health in shared
        .audio_health
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .iter_mut()
    {
        if health.state.status != AudioRouteStatus::Unused
            && !health.state.channels.is_empty()
            && same_device(driver, device, &health.state.device)
        {
            let conflict = shared
                .audio_config
                .audio_routes
                .iter()
                .find(|b| b.id == health.state.id)
                .is_some_and(|bus| {
                    crate::audio_routing::conflict(&shared.audio_config, bus, |a, b| {
                        same_device(driver, a, b)
                    })
                });
            health.state.status = if conflict {
                AudioRouteStatus::Conflict
            } else if health.state.channels.iter().all(|c| *c <= channels) {
                AudioRouteStatus::Ready
            } else {
                AudioRouteStatus::Missing
            };
            health.lost =
                (health.state.status == AudioRouteStatus::Ready).then(|| Arc::clone(lost));
        }
    }
}

pub(super) fn open_stream(
    spec: AudioSpec,
    messages: Receiver<Message>,
    shared: Arc<SharedClock>,
    beat: Arc<AtomicU64>,
) -> Result<(cpal::Stream, AudioInfo), OutputError> {
    let rate = spec.ltc_config.sample_rate;
    let opened = match open_device(&spec.device_hint, spec.driver.as_deref(), spec.needed, rate) {
        Err(OutputError::Unsupported { .. })
            if shared.operator_route.is_some() || !shared.audio_config.audio_routes.is_empty() =>
        {
            let required = spec
                .programme
                .iter()
                .copied()
                .chain(spec.ltc.iter().map(|s| s.channel))
                .chain(spec.click.iter().flat_map(|c| c.channels.iter().copied()))
                .max()
                .unwrap_or(2);
            open_device(&spec.device_hint, spec.driver.as_deref(), required, rate)?
        }
        result => result?,
    };
    let channels = opened.channels;

    let ltc = ltc_lanes(
        &spec.ltc,
        spec.ltc_config,
        spec.rate,
        spec.start,
        spec.clock,
    );
    // Channels beyond what the device opened cannot be written to.
    let programme = spec
        .programme
        .iter()
        .map(|&channel| usize::from(channel) - 1)
        .filter(|&channel| channel < usize::from(channels))
        .collect();
    let mut renderer = Renderer::new(
        Transport::new(spec.clock, spec.start),
        messages,
        spec.clips,
        programme,
        ltc,
        usize::from(channels),
        Arc::clone(&shared),
    )
    .with_click(spec.click.as_ref().map(|click| click.out(channels)));
    let requested = operator_channels(&shared, &spec.device_hint, spec.driver.as_deref());
    let valid = requested
        .iter()
        .all(|channel| *channel < usize::from(channels));
    shared.operator_invalid.store(!valid, Ordering::Relaxed);
    renderer.operator_channels = if valid { requested } else { Vec::new() };
    mark_audio_routes(
        &shared,
        &spec.device_hint,
        spec.driver.as_deref(),
        channels,
        &shared.lost,
    );
    renderer.audio_lanes =
        crate::audio_routing::lanes(&shared.audio_config, &spec.device_hint, channels, |a, b| {
            same_device(spec.driver.as_deref(), a, b)
        });
    let failed = Arc::clone(&shared);
    let stream = start_stream(&opened, rate, renderer, shared, beat, move |err| {
        failed.stream_failed(err.kind(), None);
    })?;
    Ok((
        stream,
        AudioInfo {
            device: opened.name,
            channels,
        },
    ))
}

/// Opens the stream LTC plays on when its device is not the programme's. It follows the master's
/// position, so a failure here loses the LTC and nothing else.
pub(super) fn open_ltc_stream(
    spec: LtcSpec,
    shared: Arc<SharedClock>,
    lost: Arc<AtomicBool>,
    beat: Arc<AtomicU64>,
) -> Result<(cpal::Stream, String), OutputError> {
    let rate = spec.config.sample_rate;
    let needed = spec
        .ltc
        .iter()
        .map(|send| send.channel)
        .chain(
            operator_channels(&shared, &spec.device_hint, spec.driver.as_deref())
                .into_iter()
                .map(|c| c as u16 + 1),
        )
        .chain(
            spec.click
                .iter()
                .flat_map(|click| click.channels.iter().copied()),
        )
        .chain(
            shared
                .audio_config
                .audio_routes
                .iter()
                .filter(|b| crate::audio_routing::used(&shared.audio_config, b))
                .filter_map(|b| b.route.as_ref())
                .filter(|r| same_device(spec.driver.as_deref(), &r.device_name, &spec.device_hint))
                .flat_map(|r| r.channels.iter().copied()),
        )
        .max()
        .unwrap_or(1);
    let opened = match open_device(&spec.device_hint, spec.driver.as_deref(), needed, rate) {
        Err(OutputError::Unsupported { .. }) if !shared.audio_config.audio_routes.is_empty() => {
            let required = spec
                .ltc
                .iter()
                .map(|s| s.channel)
                .chain(spec.click.iter().flat_map(|c| c.channels.iter().copied()))
                .max()
                .unwrap_or(1);
            open_device(&spec.device_hint, spec.driver.as_deref(), required, rate)?
        }
        result => result?,
    };
    let mut follower = LtcFollower::new(
        ltc_lanes(&spec.ltc, spec.config, spec.rate, spec.start, spec.clock),
        usize::from(opened.channels),
        spec.clock,
        Arc::clone(&shared),
    )
    .with_click(spec.click.as_ref().map(|click| click.out(opened.channels)));
    follower.operator_channels =
        operator_channels(&shared, &spec.device_hint, spec.driver.as_deref());
    follower.audio_lanes = crate::audio_routing::lanes(
        &shared.audio_config,
        &spec.device_hint,
        opened.channels,
        |a, b| same_device(spec.driver.as_deref(), a, b),
    );
    let failed = Arc::clone(&shared);
    let route_clock = Arc::clone(&shared);
    let route_lost = Arc::clone(&lost);
    let stream = start_stream(&opened, rate, follower, shared, beat, move |err| {
        failed.stream_failed(err.kind(), Some(&lost));
    })?;
    mark_audio_routes(
        &route_clock,
        &spec.device_hint,
        spec.driver.as_deref(),
        opened.channels,
        &route_lost,
    );
    Ok((stream, opened.name))
}

/// Builds the stream in the format chosen for the device, and starts it.
pub(super) fn start_stream<F: Fill>(
    opened: &OpenedDevice,
    rate: u32,
    filler: F,
    shared: Arc<SharedClock>,
    beat: Arc<AtomicU64>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, OutputError> {
    let config = cpal::StreamConfig {
        channels: opened.channels,
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let device = &opened.device;
    let stream = match opened.format {
        SampleFormat::F32 => build::<f32, _>(device, config, filler, shared, beat, on_error),
        SampleFormat::I32 => build::<i32, _>(device, config, filler, shared, beat, on_error),
        SampleFormat::I24 => build::<cpal::I24, _>(device, config, filler, shared, beat, on_error),
        SampleFormat::I16 => build::<i16, _>(device, config, filler, shared, beat, on_error),
        other => Err(OutputError::Stream(format!(
            "unsupported sample format {other}"
        ))),
    }?;
    stream
        .play()
        .map_err(|err| OutputError::Stream(err.to_string()))?;
    Ok(stream)
}

pub(super) fn build<T, F>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut filler: F,
    shared: Arc<SharedClock>,
    beat: Arc<AtomicU64>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, OutputError>
where
    T: SizedSample + FromSample<f32>,
    F: Fill,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                beat.fetch_add(1, Ordering::Relaxed);
                filler.fill(data, shared.now());
            },
            on_error,
            None,
        )
        .map_err(|err| OutputError::Stream(err.to_string()))
}
