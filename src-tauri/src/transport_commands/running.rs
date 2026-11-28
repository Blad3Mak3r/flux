use super::*;

impl Running {
    /// Gives the output the sound of the VJ grid (see [`crate::vj_audio`]).
    pub fn set_grid_audio(&self, mix: flux_engine::grid_audio::GridMix) {
        self.output.set_grid_audio(mix);
    }

    /// Reads where the transport is, for a thread of its own (the video output).
    pub fn reader(&self) -> flux_engine::output::PositionReader {
        self.output.reader()
    }

    // The lanes need every subscriber and the setup they were opened for; they cannot be narrower.
    #[expect(
        clippy::too_many_arguments,
        reason = "the setup, the injected app and the channels of the output"
    )]
    pub(super) fn new(
        output: Output,
        setup: OutputSetup,
        preferences: crate::preferences::Preferences,
        cues: SharedCues,
        app: AppHandle,
        advance: Option<Advance>,
        on_position: Channel<Position>,
        on_levels: Channel<Vec<TrackPeak>>,
        on_live: Channel<LiveState>,
        on_audio_routing: Channel<AudioRoutingState>,
    ) -> Result<Self, OutputError> {
        let (position_tx, position_rx) = mpsc::channel();
        let (levels_tx, levels_rx) = mpsc::channel();
        let (live_tx, live_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        // Built before the lanes, so that failing to start one stops the other: `Drop` joins them.
        let mut running = Self {
            pending_audio: None,
            stationary_requested: false,
            channels: OutputChannels {
                position: on_position.clone(),
                levels: on_levels.clone(),
                live: on_live.clone(),
                routing: on_audio_routing.clone(),
            },
            app: app.clone(),
            advance: advance.clone(),
            desired_audio: Arc::new(Mutex::new(setup.output.clone())),
            on_audio_routing: audio_tx,
            output,
            setup,
            preferences,
            cues,
            on_position: position_tx,
            on_levels: levels_tx,
            on_live: live_tx,
            stop_lanes: Arc::new(AtomicBool::new(false)),
            lanes: Vec::new(),
        };

        let reader = running.output.reader();
        let active = running.setup.output.clone();
        let desired = Arc::clone(&running.desired_audio);
        running.spawn_lane(
            "flux-audio-routing",
            report(
                audio_rx,
                on_audio_routing,
                move |lane: &mut AudioRoutingLane| {
                    let desired = desired.lock().unwrap_or_else(PoisonError::into_inner);
                    lane.next(AudioRoutingState {
                        active: reader.audio_routes(),
                        active_tracks: active.track_routes.clone(),
                        desired_tracks: desired.track_routes.clone(),
                        pending: desired.audio_routes != active.audio_routes
                            || desired.track_routes != active.track_routes,
                    })
                },
            ),
        )?;
        let reader = running.output.reader();
        running.spawn_lane(
            "flux-position",
            report(position_rx, on_position, move |lane: &mut PositionLane| {
                lane.next(reader.position())
            }),
        )?;

        let (reader, cues) = (running.output.reader(), running.cues.clone());
        let live_session = Arc::clone(&app.state::<SharedSession>());
        running.spawn_lane(
            "flux-live",
            report(live_rx, on_live, move |lane: &mut LiveLane| {
                let position = reader.position();
                lane.next(if let Some((plan, tick)) = reader.show() {
                    live_session
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .execution_select(&plan.entries[tick.entry].timeline.id);
                    plan.entries[tick.entry].cues.state_at(position.frame)
                } else {
                    cues.current().state_at(position.frame)
                })
            }),
        )?;

        if let Some(advance) = advance {
            let reader = running.output.reader();
            let mut end = EndLane::new(reader.shows_ended());
            running.spawn_lane("flux-advance", move || {
                if end.ended(reader.shows_ended()) {
                    advance.run();
                }
            })?;
        }

        let (reader, device) = (running.output.reader(), running.output.programme_device());
        let mut back = DeviceLane::default();
        running.spawn_lane("flux-device", move || {
            let lost = reader.position().device_lost;
            if back.back(lost, Instant::now(), || device.is_present()) {
                log::info!("audio device is back");
                let _ = app.emit(OUTPUT_DEVICE_BACK, ());
            }
        })?;

        let reader = running.output.reader();
        let mut log = LogLane::default();
        let mut previous_routes: Vec<flux_engine::audio_routing::AudioRouteState> = Vec::new();
        running.spawn_lane("flux-log", move || {
            for (level, line) in log.next(reader.position(), reader.faults(), Instant::now()) {
                log::log!(level, "{line}");
            }
            let routes = reader.audio_routes();
            for route in &routes {
                if previous_routes.iter().find(|r| r.id == route.id) != Some(route) {
                    log::info!(
                        "audio destination {}: {:?}, device {}, channels {:?}",
                        route.id,
                        route.status,
                        route.device,
                        route.channels
                    );
                }
            }
            previous_routes = routes;
        })?;

        let levels = running.output.levels();
        let levels_reader = running.output.reader();
        running.spawn_lane(
            "flux-levels",
            report(levels_rx, on_levels, move |lane: &mut LevelsLane| {
                lane.next(if let Some((plan, tick)) = levels_reader.show() {
                    plan.entries[tick.entry].clips.levels().take_peaks()
                } else {
                    levels.current().take_peaks()
                })
            }),
        )?;
        Ok(running)
    }

    fn spawn_lane(
        &mut self,
        name: &str,
        mut step: impl FnMut() + Send + 'static,
    ) -> Result<(), OutputError> {
        let stop = Arc::clone(&self.stop_lanes);
        let lane = thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    step();
                    thread::sleep(LANE_INTERVAL);
                }
            })
            .map_err(|err| OutputError::Stream(err.to_string()))?;
        self.lanes.push(lane);
        Ok(())
    }
}

/// A new subscriber receives the first reading even when nothing moved since the last tick.
pub(super) fn report<T: IpcResponse, L: Default>(
    subscriptions: mpsc::Receiver<Channel<T>>,
    mut channel: Channel<T>,
    mut next: impl FnMut(&mut L) -> Option<T>,
) -> impl FnMut() {
    let mut lane = L::default();
    move || {
        if let Some(replacement) = subscriptions.try_iter().last() {
            channel = replacement;
            lane = L::default();
        }
        if let Some(value) = next(&mut lane) {
            let _ = channel.send(value);
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop_lanes.store(true, Ordering::Relaxed);
        for lane in self.lanes.drain(..) {
            let _ = lane.join();
        }
    }
}
