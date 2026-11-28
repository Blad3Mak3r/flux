use super::*;

/// A continuous voice cursor for an independent device clock. Small clock drift is corrected
/// by bounded resampling, rather than skipping or repeating chunks of spoken words.
#[derive(Debug, Default, Clone)]
pub(super) struct VoiceCursor {
    pub(super) next: Option<f64>,
    pub(super) step: f64,
}

impl VoiceCursor {
    pub(super) fn follow(&mut self, target: u64, reset: bool) {
        let target = target as f64;
        let position = self.next.unwrap_or(target);
        if reset || (position - target).abs() > f64::from(ENGINE_RATE) * 0.25 {
            self.next = Some(target);
            self.step = 1.0;
        } else {
            self.next = Some(position);
            self.step =
                1.0 + ((target - position) / (f64::from(ENGINE_RATE) * 2.0)).clamp(-0.002, 0.002);
        }
    }

    pub(super) fn mix(&mut self, clips: &ClipSet, left: &mut [f32], right: &mut [f32]) {
        self.mix_selected(clips, left, right, None);
    }
    fn mix_selected(
        &mut self,
        clips: &ClipSet,
        left: &mut [f32],
        right: &mut [f32],
        tracks: Option<&[String]>,
    ) {
        let position = self.next.unwrap_or(0.0);
        let from = position.floor() as u64;
        let fraction = position.fract();
        // At most MIX_CHUNK output frames, with a rate within 0.2% of unity.
        let needed = (fraction + left.len() as f64 * self.step).ceil() as usize + 1;
        let mut source_left = [0.0; MIX_CHUNK + 4];
        let mut source_right = [0.0; MIX_CHUNK + 4];
        if let Some(tracks) = tracks {
            clips.mix_tracks(
                from,
                &mut source_left[..needed],
                &mut source_right[..needed],
                tracks,
            );
        } else {
            clips.mix_operator(
                from,
                &mut source_left[..needed],
                &mut source_right[..needed],
            );
        }
        for (i, (left, right)) in left.iter_mut().zip(right).enumerate() {
            let sample = fraction + i as f64 * self.step;
            let at = sample.floor() as usize;
            let blend = sample.fract() as f32;
            *left = source_left[at] + (source_left[at + 1] - source_left[at]) * blend;
            *right = source_right[at] + (source_right[at + 1] - source_right[at]) * blend;
        }
        self.next = Some(position + left.len() as f64 * self.step);
    }
}

/// Plays LTC on an audio device that is not the master's. The two sample clocks are independent
/// and drift apart, so this does not count samples against the transport: it reads where the
/// master is, plays LTC from there, and starts over from the master's position whenever it is a
/// frame or more away, or the master seeks.
#[derive(Debug)]
pub struct LtcFollower {
    pub(super) audio_lanes: Option<Vec<crate::audio_routing::AudioLane>>,
    pub(super) voice_cursor: VoiceCursor,
    operator_clips: Arc<ClipSet>,
    operator_updates: Receiver<Arc<ClipSet>>,
    pub(super) operator_channels: Vec<usize>,
    /// One lane per distinct latency offset; none when the stream plays only the click.
    lanes: Vec<LtcChannel>,
    channels: usize,
    clock: Clock,
    shared: Arc<SharedClock>,
    /// The last position read; kept when the master is publishing and the read is skipped.
    snapshot: Snapshot,
    /// The transport position of the next sample the lanes write. `None` until synchronised, and
    /// after the transport stops playing.
    next: Option<u64>,
    seen_epoch: u64,
    /// The metronome, when it plays on this device. A follower with no LTC `outputs` plays only
    /// the click.
    click: Option<ClickOut>,
    /// The transport position of the next click sample; kept apart from `next` because it follows
    /// the master far more closely than an LTC frame needs.
    click_next: Option<u64>,
}

impl LtcFollower {
    pub fn new(
        lanes: Vec<LtcChannel>,
        channels: usize,
        clock: Clock,
        shared: Arc<SharedClock>,
    ) -> Self {
        let snapshot = shared.load();
        let (operator_clips, operator_updates) = shared.subscribe_operator();
        Self {
            voice_cursor: VoiceCursor::default(),
            operator_clips,
            operator_updates,
            audio_lanes: None,
            operator_channels: Vec::new(),
            lanes,
            channels,
            clock,
            shared,
            seen_epoch: snapshot.epoch,
            snapshot,
            next: None,
            click: None,
            click_next: None,
        }
    }

    /// Plays the metronome on this stream too, on its own channels.
    pub fn with_click(mut self, click: Option<ClickOut>) -> Self {
        self.click = click;
        self
    }

    /// Fills one interleaved buffer. `now` is [`SharedClock::now`] at the time of the call.
    pub fn render<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        data.fill(T::EQUILIBRIUM);
        while let Ok(set) = self.operator_updates.try_recv() {
            self.operator_clips = set;
        }
        if let Some(snapshot) = self.shared.try_load() {
            self.snapshot = snapshot;
        }
        let snapshot = self.snapshot;
        if let Some((clock, start)) = self.shared.show_timecode(&snapshot) {
            self.clock = clock;
            if let Some(plan) = self.shared.show_plan.get() {
                let Some(tick) = snapshot.show else {
                    return;
                };
                let entry = &plan.entries[tick.entry];
                self.operator_clips = Arc::clone(&entry.clips);
                if let Some(click) = &self.click {
                    click.shared.set(entry.timeline.tempo);
                }
                if self.seen_epoch != snapshot.epoch {
                    for lane in &mut self.lanes {
                        lane.stream
                            .set_timecode(entry.timeline.timecode.frame_rate, start);
                    }
                }
            }
        }
        if snapshot.state != TransportState::Playing {
            self.voice_cursor.next = None;
            self.next = None;
            self.click_next = None;
            self.seen_epoch = snapshot.epoch;
            return;
        }

        let target = snapshot.estimated_position(now, self.clock);
        let one_frame = self.clock.frames_to_samples(1);
        let changed_epoch = self.seen_epoch != snapshot.epoch;
        self.voice_cursor
            .follow(target, changed_epoch || self.shared.is_lost());
        if changed_epoch {
            self.click_next = None;
        }
        let in_step = !changed_epoch
            && self
                .next
                .is_some_and(|next| next.abs_diff(target) < one_frame);
        if !in_step {
            self.seen_epoch = snapshot.epoch;
            for lane in &mut self.lanes {
                lane.stream.resync(target);
            }
            self.next = Some(target);
        }

        let grid = self
            .click
            .as_ref()
            .and_then(|click| click.shared.grid(ENGINE_RATE));
        let mut click_at = match self.click_next {
            Some(next) if grid.is_some() && next.abs_diff(target) < CLICK_SYNC_SAMPLES => next,
            _ => target,
        };
        let click_channels: &[usize] = self.click.as_ref().map_or(&[], |click| &click.channels);
        for chunk in data.chunks_mut(self.channels * MIX_CHUNK) {
            let frames = chunk.len() / self.channels;
            let mut click = [0.0f32; MIX_CHUNK];
            if let Some(grid) = &grid {
                grid.add(click_at, &mut click[..frames]);
            }
            let mut operator_left = [0.0f32; MIX_CHUNK];
            let mut operator_right = [0.0f32; MIX_CHUNK];
            if self.audio_lanes.is_none()
                && !self.operator_channels.is_empty()
                && !self.shared.is_lost()
            {
                self.voice_cursor.mix(
                    &self.operator_clips,
                    &mut operator_left[..frames],
                    &mut operator_right[..frames],
                );
            }
            if let Some(lanes) = &mut self.audio_lanes {
                let mut advanced = self.voice_cursor.clone();
                for lane in lanes {
                    lane.left.fill(0.0);
                    lane.right.fill(0.0);
                    if !self.shared.is_lost() {
                        let mut cursor = self.voice_cursor.clone();
                        cursor.mix_selected(
                            &self.operator_clips,
                            &mut lane.left[..frames],
                            &mut lane.right[..frames],
                            Some(&lane.tracks),
                        );
                        advanced = cursor;
                    }
                }
                self.voice_cursor = advanced;
            }
            click_at += frames as u64;
            for (at, frame) in chunk.chunks_exact_mut(self.channels).enumerate() {
                // LTC is written after the click, so it wins a channel both ask for.
                if click[at] != 0.0 {
                    for &channel in click_channels {
                        frame[channel] = T::from_sample(click[at].clamp(-1.0, 1.0));
                    }
                }
                for (index, &channel) in self.operator_channels.iter().enumerate() {
                    let voice = if self.operator_channels.len() == 1 {
                        (operator_left[at] + operator_right[at]) * 0.5
                    } else if index == 0 {
                        operator_left[at]
                    } else {
                        operator_right[at]
                    };
                    let beat = if click_channels.contains(&channel) {
                        click[at]
                    } else {
                        0.0
                    };
                    frame[channel] = T::from_sample((voice + beat).clamp(-1.0, 1.0));
                }
                if let Some(lanes) = &self.audio_lanes {
                    for (channel, sample) in frame.iter_mut().enumerate() {
                        let beat = if click_channels.contains(&channel) {
                            click[at]
                        } else {
                            0.0
                        };
                        *sample = T::from_sample(
                            (lanes.iter().map(|l| l.sample(channel, at)).sum::<f32>() + beat)
                                .clamp(-1.0, 1.0),
                        );
                    }
                }
                for lane in &mut self.lanes {
                    let sample = lane.stream.next_sample();
                    for tap in &lane.channels {
                        frame[tap.channel] = T::from_sample(sample * tap.gain);
                    }
                }
            }
        }
        self.click_next = grid.is_some().then_some(click_at);
        let written = (data.len() / self.channels) as u64;
        self.next = self.next.map(|next| next + written);
    }
}

impl Fill for LtcFollower {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        self.render(data, now);
    }
}
