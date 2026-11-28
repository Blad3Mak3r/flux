use super::*;

/// One interface channel an LTC signal is written to, and how loud.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LtcTap {
    /// Zero-based index into each frame of the interleaved buffer.
    pub channel: usize,
    /// Linear factor applied to the generator's full-scale signal.
    pub gain: f32,
}

impl LtcTap {
    /// The tap for a project output: a 1-based channel and its level.
    fn new(send: LtcSend) -> Self {
        Self {
            channel: usize::from(send.channel) - 1,
            gain: send.level,
        }
    }
}

/// One LTC output as the streams need it: where it goes, how loud and how far ahead.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LtcSend {
    /// 1-based interface channel.
    pub(super) channel: u16,
    /// Linear peak level.
    pub(super) level: f32,
    /// Latency offset in ms, positive = earlier.
    pub(super) offset_ms: i32,
}

/// Groups outputs into one [`LtcChannel`] per distinct offset: outputs with the same offset share
/// a generator, as outputs on one device always did.
pub(super) fn ltc_lanes(
    sends: &[LtcSend],
    config: LtcConfig,
    rate: FrameRate,
    start: u64,
    clock: Clock,
) -> Vec<LtcChannel> {
    let mut lanes: Vec<(i32, LtcChannel)> = Vec::new();
    for &send in sends {
        let at = lanes
            .iter()
            .position(|(offset_ms, _)| *offset_ms == send.offset_ms)
            .unwrap_or_else(|| {
                let stream = LtcStream::new(config, rate, start, 0)
                    .with_offset(clock.ms_to_samples(send.offset_ms));
                lanes.push((
                    send.offset_ms,
                    LtcChannel {
                        stream,
                        channels: Vec::new(),
                    },
                ));
                lanes.len() - 1
            });
        lanes[at].1.channels.push(LtcTap::new(send));
    }
    lanes.into_iter().map(|(_, lane)| lane).collect()
}

/// The LTC generator and the interface channels it goes to. One signal feeds every channel, so
/// two LTC outputs on one device with the same offset cost a single generator; each channel has a
/// level of its own.
#[derive(Debug)]
pub struct LtcChannel {
    pub stream: LtcStream,
    pub channels: Vec<LtcTap>,
}

/// Fills audio buffers from the transport. Owned by the audio callback, so it must not allocate
/// or wait.
#[derive(Debug)]
pub struct Renderer {
    show: Option<crate::show::ShowRunner>,
    pub(super) audio_lanes: Option<Vec<crate::audio_routing::AudioLane>>,
    pub(super) operator_channels: Vec<usize>,
    pub(super) transport: Transport,
    messages: Receiver<Message>,
    pub(super) clips: Arc<ClipSet>,
    /// Zero-based interface channels the programme goes to: left then right, or one for both.
    programme: Vec<usize>,
    /// One lane per distinct latency offset.
    ltc: Vec<LtcChannel>,
    /// The metronome, when it plays on the programme's device.
    click: Option<ClickOut>,
    channels: usize,
    shared: Arc<SharedClock>,
    seen_epoch: u64,
    pace: PaceWatch,
    grid: crate::grid_audio::GridPlayer,
}

impl Renderer {
    pub fn new(
        mut transport: Transport,
        messages: Receiver<Message>,
        clips: Arc<ClipSet>,
        programme: Vec<usize>,
        ltc: impl IntoIterator<Item = LtcChannel>,
        channels: usize,
        shared: Arc<SharedClock>,
    ) -> Self {
        transport.set_end(clips.end());
        Self {
            show: None,
            audio_lanes: None,
            operator_channels: Vec::new(),
            seen_epoch: transport.epoch(),
            transport,
            messages,
            clips,
            programme,
            ltc: ltc.into_iter().collect(),
            click: None,
            channels,
            shared,
            pace: PaceWatch::default(),
            grid: crate::grid_audio::GridPlayer::default(),
        }
    }

    /// Plays the metronome on this stream too, on its own channels.
    pub fn with_click(mut self, click: Option<ClickOut>) -> Self {
        self.click = click;
        self
    }

    /// Fills one interleaved buffer. `now` is [`SharedClock::now`] at the time of the call.
    pub fn render<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        if self.show.is_none()
            && let Some(plan) = self.shared.show_plan.get()
        {
            self.show = Some(crate::show::ShowRunner::new(
                Arc::clone(plan),
                &mut self.transport,
            ));
        }
        let mut consumed_plays = 0;
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Transport(command) => {
                    if matches!(
                        command,
                        Command::Pause | Command::Stop | Command::Seek(_) | Command::Restore { .. }
                    ) {
                        self.shared.show_generation.fetch_add(1, Ordering::AcqRel);
                    }
                    if matches!(command, Command::Play) {
                        consumed_plays += 1;
                    }
                    if let Some(show) = &mut self.show {
                        show.transport(command, &mut self.transport);
                    } else {
                        self.transport.apply(command);
                    }
                }
                Message::Show(command) => {
                    if matches!(command, crate::show::ShowCommand::Prepare(_)) {
                        self.shared.show_generation.fetch_add(1, Ordering::AcqRel);
                    }
                    if let Some(show) = &mut self.show {
                        show.command(command, &mut self.transport);
                    }
                }
                Message::Clips(clips) => {
                    self.transport.set_end(clips.end());
                    self.clips = clips;
                }
            }
        }
        data.fill(T::EQUILIBRIUM);
        self.grid.begin(&self.shared.grid_audio);
        let frames = (data.len() / self.channels) as u64;
        if self.show.is_some() {
            if self.shared.show_fault.load(Ordering::Acquire) {
                self.transport.apply(Command::Pause);
                if let Some(show) = &mut self.show {
                    show.tick.status = crate::show::ShowStatus::Blocked;
                    show.tick.go = crate::show::GoAction::None;
                }
            }
            let mut offset = 0;
            while offset < data.len() / self.channels * self.channels {
                let available = ((data.len() - offset) / self.channels) as u64;
                let count = {
                    let Some(show) = self.show.as_mut() else {
                        break;
                    };
                    let count = show.slice(&mut self.transport, available);
                    self.clips = Arc::clone(&show.current().clips);
                    if let Some(click) = &self.click {
                        click.shared.set(show.current().timeline.tempo);
                    }
                    count
                };
                let playing = self.transport.state() == TransportState::Playing;
                self.sync_ltc();
                let from = self.transport.position_samples();
                let Some(tick) = self.show.as_ref().map(|show| show.tick) else {
                    break;
                };
                let end = offset + count as usize * self.channels;
                if playing {
                    self.mix(&mut data[offset..end]);
                } else {
                    self.mix_grid_alone(&mut data[offset..end]);
                }
                if let Some(sender) = self.shared.show_spans.get() {
                    let include_end = playing
                        && self.show.as_ref().is_some_and(|show| {
                            if from + count != show.current().end {
                                return false;
                            }
                            tick.control
                                .and_then(|i| show.current().timeline.controls.get(i))
                                .is_none_or(|control| match control.kind {
                                    flux_project::ControlKind::Loop { turns, .. } => {
                                        tick.devamp_pending || turns.is_some_and(|n| tick.turn >= n)
                                    }
                                    _ => true,
                                })
                        });
                    let span = crate::show::ShowSpan {
                        include_end,
                        generation: self.shared.show_generation.load(Ordering::Acquire),
                        entry: tick.entry,
                        from,
                        to: from + if playing { count } else { 0 },
                        epoch: self.transport.epoch(),
                        turn: tick.turn,
                        release: !playing,
                    };
                    if sender.try_send(span).is_err() {
                        self.transport.apply(Command::Pause);
                        if let Some(show) = &mut self.show {
                            show.tick.status = crate::show::ShowStatus::Blocked;
                            show.tick.go = crate::show::GoAction::None;
                        }
                        self.shared.cue_failed();
                    }
                }
                self.transport.advance(count);
                offset = end;
            }
        } else {
            self.sync_ltc();
            if self.transport.state() == TransportState::Playing {
                self.mix(data);
            } else {
                self.mix_grid_alone(data);
            }
            if self.transport.advance(frames) {
                self.shared.shows_ended.fetch_add(1, Ordering::Relaxed);
            }
        }
        if let Some(unpaced) = self.pace.observe(now, frames) {
            self.shared.unpaced.store(unpaced, Ordering::Relaxed);
        }
        self.shared.publish(Snapshot {
            show: self.show.as_ref().map(|show| show.tick),
            state: self.transport.state(),
            epoch: self.transport.epoch(),
            position: self.transport.position_samples(),
            stamp: now,
        });
        // `try_update` is the new name, but it needs a newer Rust than the one this builds with.
        #[allow(deprecated)]
        let _ = self.shared.pending_plays.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |pending| Some(pending.saturating_sub(consumed_plays)),
        );
    }

    fn sync_ltc(&mut self) {
        if self.transport.epoch() != self.seen_epoch {
            self.seen_epoch = self.transport.epoch();
            for lane in &mut self.ltc {
                if let Some(show) = &self.show {
                    let tc = &show.current().timeline.timecode;
                    lane.stream.set_timecode(tc.frame_rate, tc.start);
                }
                lane.stream.resync(self.transport.position_samples());
            }
        }
    }

    /// While the transport is not playing, writes the sound of the VJ grid, if any, to the
    /// programme's channels; `data` is silent then.
    fn mix_grid_alone<T: Sample + FromSample<f32>>(&mut self, data: &mut [T]) {
        if self.grid.is_silent() {
            crate::analysis::tap_record_silence(data.len() / self.channels, ENGINE_RATE);
            return;
        }
        for chunk in data.chunks_mut(self.channels * MIX_CHUNK) {
            let frames = chunk.len() / self.channels;
            let mut left = [0.0f32; MIX_CHUNK];
            let mut right = [0.0f32; MIX_CHUNK];
            self.grid.chunk(&mut left[..frames], &mut right[..frames]);
            for (at, frame) in chunk.chunks_exact_mut(self.channels).enumerate() {
                match self.programme.as_slice() {
                    [] => {}
                    [only] => {
                        frame[*only] =
                            T::from_sample(((left[at] + right[at]) * 0.5).clamp(-1.0, 1.0))
                    }
                    [first, second, ..] => {
                        frame[*first] = T::from_sample(left[at].clamp(-1.0, 1.0));
                        frame[*second] = T::from_sample(right[at].clamp(-1.0, 1.0));
                    }
                }
            }
        }
    }

    /// Writes the programme, the metronome and LTC into `data`, which holds the buffer's frames.
    /// A metronome channel the programme also uses gets the click added to the programme; one it
    /// does not use carries the click alone.
    fn mix<T: Sample + FromSample<f32>>(&mut self, data: &mut [T]) {
        let channels = self.channels;
        let position = self.transport.position_samples();
        let grid = self
            .click
            .as_ref()
            .and_then(|click| click.shared.grid(ENGINE_RATE));
        let click_channels: &[usize] = self.click.as_ref().map_or(&[], |click| &click.channels);
        for (index, chunk) in data.chunks_mut(channels * MIX_CHUNK).enumerate() {
            let frames = chunk.len() / channels;
            let at_chunk = position + (index * MIX_CHUNK) as u64;
            let mut left = [0.0f32; MIX_CHUNK];
            let mut right = [0.0f32; MIX_CHUNK];
            if self.audio_lanes.is_none() {
                self.clips
                    .mix(at_chunk, &mut left[..frames], &mut right[..frames]);
                self.grid.chunk(&mut left[..frames], &mut right[..frames]);
            }
            // Routed audio has its own mix; the grid's sound goes to the programme's channels on top.
            let mut grid_left = [0.0f32; MIX_CHUNK];
            let mut grid_right = [0.0f32; MIX_CHUNK];
            if let Some(lanes) = &mut self.audio_lanes {
                self.grid
                    .chunk(&mut grid_left[..frames], &mut grid_right[..frames]);
                for lane in lanes {
                    lane.mix(&self.clips, at_chunk, frames);
                }
            }
            let mut operator_left = [0.0f32; MIX_CHUNK];
            let mut operator_right = [0.0f32; MIX_CHUNK];
            if self.audio_lanes.is_none() && !self.operator_channels.is_empty() {
                self.clips.mix_operator(
                    at_chunk,
                    &mut operator_left[..frames],
                    &mut operator_right[..frames],
                );
            }
            let mut click = [0.0f32; MIX_CHUNK];
            if let Some(grid) = &grid {
                grid.add(at_chunk, &mut click[..frames]);
            }
            for (at, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                let beat = click[at];
                let click_on = |channel: usize| {
                    if click_channels.contains(&channel) {
                        beat
                    } else {
                        0.0
                    }
                };
                match self.programme.as_slice() {
                    [] => {}
                    [only] => {
                        frame[*only] = T::from_sample(
                            ((left[at] + right[at]) * 0.5 + click_on(*only)).clamp(-1.0, 1.0),
                        )
                    }
                    [first, second, ..] => {
                        frame[*first] =
                            T::from_sample((left[at] + click_on(*first)).clamp(-1.0, 1.0));
                        frame[*second] =
                            T::from_sample((right[at] + click_on(*second)).clamp(-1.0, 1.0));
                    }
                }
                if beat != 0.0 {
                    for &channel in click_channels {
                        if !self.programme.contains(&channel) {
                            frame[channel] = T::from_sample(beat.clamp(-1.0, 1.0));
                        }
                    }
                }
                for (channel_index, &channel) in self.operator_channels.iter().enumerate() {
                    let voice = if self.operator_channels.len() == 1 {
                        (operator_left[at] + operator_right[at]) * 0.5
                    } else if channel_index == 0 {
                        operator_left[at]
                    } else {
                        operator_right[at]
                    };
                    let current = match self.programme.as_slice() {
                        [only] if *only == channel => (left[at] + right[at]) * 0.5,
                        [first, ..] if *first == channel => left[at],
                        [_, second, ..] if *second == channel => right[at],
                        _ => 0.0,
                    } + click_on(channel);
                    frame[channel] = T::from_sample((current + voice).clamp(-1.0, 1.0));
                }
                if let Some(lanes) = &self.audio_lanes {
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        let grid = match self.programme.as_slice() {
                            [only] if *only == channel => (grid_left[at] + grid_right[at]) * 0.5,
                            [first, ..] if *first == channel => grid_left[at],
                            [_, second, ..] if *second == channel => grid_right[at],
                            _ => 0.0,
                        };
                        let sum = lanes.iter().map(|l| l.sample(channel, at)).sum::<f32>()
                            + click_on(channel)
                            + grid;
                        *sample = T::from_sample(sum.clamp(-1.0, 1.0));
                    }
                }
                for lane in &mut self.ltc {
                    let sample = lane.stream.next_sample();
                    for tap in &lane.channels {
                        frame[tap.channel] = T::from_sample(sample * tap.gain);
                    }
                }
            }
        }
    }
}

/// Something that fills the buffers of one audio stream.
pub(super) trait Fill: Send + 'static {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64);
}

impl Fill for Renderer {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        self.render(data, now);
    }
}
