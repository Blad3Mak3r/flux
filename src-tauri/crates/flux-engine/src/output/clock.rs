use super::*;

/// A device that does not pace its callbacks (ALSA's null device) makes the transport run many
/// times faster than real time. This compares the audio rendered with the wall clock.
#[derive(Debug, Default)]
pub(super) struct PaceWatch {
    window_start: Option<u64>,
    window_frames: u64,
}

/// Longer than this over real time and the clock is not a clock.
pub(super) const MAX_PACE_RATIO: f64 = 1.5;
pub(super) const PACE_WINDOW_NANOS: u64 = 1_000_000_000;

impl PaceWatch {
    /// Counts `frames` rendered at wall-clock time `now` (nanoseconds). Once a second has passed
    /// it returns whether the stream ran faster than real time over that second.
    pub(super) fn observe(&mut self, now: u64, frames: u64) -> Option<bool> {
        let start = *self.window_start.get_or_insert(now);
        self.window_frames += frames;
        let elapsed = now.saturating_sub(start);
        if elapsed < PACE_WINDOW_NANOS {
            return None;
        }
        let rendered = self.window_frames as f64 / f64::from(ENGINE_RATE);
        let ratio = rendered / (elapsed as f64 / 1e9);
        self.window_start = Some(now);
        self.window_frames = 0;
        Some(ratio > MAX_PACE_RATIO)
    }
}

/// What the audio callback last published about the transport.
#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub show: Option<crate::show::ShowTick>,
    pub state: TransportState,
    pub epoch: u64,
    /// Samples since the timeline origin, at the end of the last buffer.
    pub position: u64,
    /// When it was published, in nanoseconds since [`SharedClock`] was created.
    pub stamp: u64,
}

impl Snapshot {
    /// The position now: the published one plus the time that has passed since, so readers are
    /// not limited to the granularity of the audio buffer.
    pub fn estimated_position(&self, now: u64, clock: Clock) -> u64 {
        if self.state != TransportState::Playing {
            return self.position;
        }
        let elapsed = u128::from(now.saturating_sub(self.stamp)).min(MAX_EXTRAPOLATION.as_nanos());
        self.position + (elapsed * u128::from(clock.sample_rate()) / 1_000_000_000) as u64
    }
}

/// The transport position shared between the audio callback and everything that reads it.
///
/// The callback only ever `try_lock`s: if a reader happens to hold the lock it skips the update,
/// and the next buffer publishes a fresher one. It never waits.
#[derive(Debug)]
pub struct SharedClock {
    pub(super) show_fault: AtomicBool,
    pub(crate) show_generation: AtomicU64,
    pub(crate) show_plan: OnceLock<Arc<crate::show::ShowPlan>>,
    pub(crate) show_spans: OnceLock<mpsc::SyncSender<crate::show::ShowSpan>>,
    snapshot: Mutex<Snapshot>,
    base: Instant,
    pub(super) lost: Arc<AtomicBool>,
    /// Which of the other outputs (LTC on a device of its own, the click, MTC ports) are lost.
    health: Health,
    pub(super) unpaced: AtomicBool,
    /// How many times the show ran to its end by itself (not stopped by the user).
    pub(super) shows_ended: AtomicU64,
    /// The faults the streams and the MTC thread report. Only counted here: a lane thread reads
    /// them and writes the log, because nothing on those threads may wait for a file.
    faults: [AtomicU64; 4],
    pub(super) operator_route: Option<flux_project::AudioOutput>,
    pub(super) audio_config: OutputConfig,
    pub(super) audio_health: RwLock<Vec<crate::audio_routing::RouteHealth>>,
    pub(super) operator_set: Mutex<Arc<ClipSet>>,
    operator_subscribers: Mutex<Vec<Sender<Arc<ClipSet>>>>,
    pub(super) pending_plays: AtomicU64,
    pub(super) operator_health: RwLock<Option<Arc<AtomicBool>>>,
    pub(super) operator_invalid: AtomicBool,
    /// The sound of the VJ grid, which plays whether the transport does or not.
    pub(super) grid_audio: crate::grid_audio::GridAudio,
    /// The tempo the MIDI clock runs at, as the bits of an `f64`: the control side moves it while
    /// the show plays and the clock thread reads it.
    pub(super) clock_bpm: AtomicU64,
}

/// What a [`SharedClock`] counts going wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Faults {
    /// Buffers the device under- or overran, which are heard as a dropout.
    pub xruns: u64,
    /// Other errors of a stream. They mark the device as lost.
    pub stream_errors: u64,
    /// MIDI messages that could not be sent.
    pub mtc_errors: u64,
    /// MIDI or OSC messages of the cue tracks that could not be sent.
    pub cue_errors: u64,
}

impl SharedClock {
    pub(crate) fn show_timecode(&self, snapshot: &Snapshot) -> Option<(Clock, u64)> {
        let entry = self.show_plan.get()?.entries.get(snapshot.show?.entry)?;
        Some((
            Clock::new(ENGINE_RATE, entry.timeline.timecode.frame_rate),
            entry.timeline.timecode.start,
        ))
    }

    pub fn new() -> Self {
        Self {
            show_fault: AtomicBool::new(false),
            show_generation: AtomicU64::new(0),
            show_plan: OnceLock::new(),
            show_spans: OnceLock::new(),
            snapshot: Mutex::new(Snapshot {
                show: None,
                state: TransportState::Stopped,
                epoch: 0,
                position: 0,
                stamp: 0,
            }),
            base: Instant::now(),
            lost: Arc::new(AtomicBool::new(false)),
            health: Health::default(),
            unpaced: AtomicBool::new(false),
            shows_ended: AtomicU64::new(0),
            faults: Default::default(),
            operator_route: None,
            audio_config: OutputConfig::default(),
            audio_health: RwLock::new(Vec::new()),
            operator_health: RwLock::new(None),
            operator_invalid: AtomicBool::new(false),
            grid_audio: crate::grid_audio::GridAudio::default(),
            clock_bpm: AtomicU64::new(120f64.to_bits()),
            operator_set: Mutex::new(Arc::new(ClipSet::default())),
            operator_subscribers: Mutex::new(Vec::new()),
            pending_plays: AtomicU64::new(0),
        }
    }

    pub(super) fn subscribe_operator(&self) -> (Arc<ClipSet>, Receiver<Arc<ClipSet>>) {
        let mut subscribers = self
            .operator_subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (tx, rx) = mpsc::channel();
        let current = Arc::clone(
            &self
                .operator_set
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        subscribers.push(tx);
        (current, rx)
    }

    pub(super) fn update_operator(&self, set: Arc<ClipSet>) {
        let mut subscribers = self
            .operator_subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        *self
            .operator_set
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Arc::clone(&set);
        subscribers.retain(|tx| tx.send(Arc::clone(&set)).is_ok());
    }

    /// A stream reported an error. Safe to call from the error callback: it only touches atomics.
    /// A dropout is counted and the device stays in use; any other error loses the device: the
    /// programme's, or the one `flag` belongs to when the stream is not the programme's.
    pub fn stream_failed(&self, kind: cpal::ErrorKind, flag: Option<&AtomicBool>) {
        if kind == cpal::ErrorKind::Xrun {
            self.faults[0].fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.faults[1].fetch_add(1, Ordering::Relaxed);
        self.show_fault.store(true, Ordering::Release);
        flag.unwrap_or(&*self.lost).store(true, Ordering::Relaxed);
    }

    /// The flag a stream of the programme's device raises when it is lost.
    pub(crate) fn lost_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.lost)
    }

    /// Which outputs are lost, per output.
    pub fn health(&self) -> &Health {
        &self.health
    }

    /// A MIDI message could not be sent.
    pub(super) fn mtc_failed(&self) {
        self.faults[2].fetch_add(1, Ordering::Relaxed);
        self.show_fault.store(true, Ordering::Release);
    }

    /// A message of a cue track could not be sent.
    pub(crate) fn cue_failed(&self) {
        self.faults[3].fetch_add(1, Ordering::Relaxed);
        self.show_fault.store(true, Ordering::Release);
    }

    pub fn faults(&self) -> Faults {
        let [xruns, stream_errors, mtc_errors, cue_errors] = &self.faults;
        Faults {
            xruns: xruns.load(Ordering::Relaxed),
            stream_errors: stream_errors.load(Ordering::Relaxed),
            mtc_errors: mtc_errors.load(Ordering::Relaxed),
            cue_errors: cue_errors.load(Ordering::Relaxed),
        }
    }

    /// Moves the tempo of the MIDI clock.
    pub(super) fn set_clock_bpm(&self, bpm: f64) {
        self.clock_bpm.store(bpm.to_bits(), Ordering::Relaxed);
    }

    /// Nanoseconds since this clock was created.
    pub fn now(&self) -> u64 {
        self.base.elapsed().as_nanos() as u64
    }

    pub(super) fn publish(&self, snapshot: Snapshot) {
        if let Ok(mut current) = self.snapshot.try_lock() {
            *current = snapshot;
        }
    }

    pub fn load(&self) -> Snapshot {
        *self.snapshot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Like [`load`](Self::load) but never waits, for a second audio callback: `None` when the
    /// master callback is publishing at this very moment.
    pub fn try_load(&self) -> Option<Snapshot> {
        match self.snapshot.try_lock() {
            Ok(snapshot) => Some(*snapshot),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(*poisoned.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }

    /// Whether the audio device failed or went away while running.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// Whether the device renders audio faster than real time, so the position cannot be trusted.
    pub fn is_unpaced(&self) -> bool {
        self.unpaced.load(Ordering::Relaxed)
    }

    /// How many times the show ran to its end by itself since this clock was created.
    pub fn shows_ended(&self) -> u64 {
        self.shows_ended.load(Ordering::Relaxed)
    }
}

impl Default for SharedClock {
    fn default() -> Self {
        Self::new()
    }
}

/// The lost flag of one stream or MTC thread and the project outputs that depend on it. The thread
/// only ever stores into the flag, so it is safe to set from an audio callback.
#[derive(Debug, Clone)]
pub(crate) struct Lamp {
    /// Bit *i* is the output at position *i* of `OutputInfo.ltc` (or `.mtc`).
    pub(super) outputs: u32,
    lost: Arc<AtomicBool>,
}

impl Lamp {
    pub(crate) fn new(outputs: impl IntoIterator<Item = usize>, lost: &Arc<AtomicBool>) -> Self {
        Self {
            outputs: mask(outputs),
            lost: Arc::clone(lost),
        }
    }
}

/// The bits of the outputs at these positions.
pub(super) fn mask(outputs: impl IntoIterator<Item = usize>) -> u32 {
    outputs.into_iter().fold(0, |mask, index| mask | bit(index))
}

/// The bit of the output at `index`. Outputs past the 32nd have none: they are never shown as lost.
pub(super) fn bit(index: usize) -> u32 {
    u32::try_from(index)
        .ok()
        .and_then(|index| 1u32.checked_shl(index))
        .unwrap_or(0)
}

/// What a running output has in each of its streams and ports, to tell which are lost.
#[derive(Debug, Default, Clone)]
pub(crate) struct Lamps {
    /// The LTC outputs that play on the programme's stream: they are lost with it.
    pub(crate) programme_ltc: u32,
    /// Whether the metronome plays on the programme's stream.
    pub(crate) programme_click: bool,
    /// The streams LTC plays on when it is not the programme's.
    pub(crate) ltc: Vec<Lamp>,
    /// One per MIDI port MTC goes to.
    pub(crate) mtc: Vec<Lamp>,
    /// The stream the metronome plays on, when it is not the programme's.
    pub(crate) click: Option<Arc<AtomicBool>>,
}

/// Which outputs are lost, one by one. Written when the output opens; read by whoever reports the
/// position, never by an audio callback.
#[derive(Debug, Default)]
pub struct Health(RwLock<Lamps>);

impl Health {
    pub(crate) fn replace(&self, lamps: Lamps) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = lamps;
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Lamps> {
        self.0.read().unwrap_or_else(PoisonError::into_inner)
    }

    /// Bit *i* is set when `OutputInfo.ltc[i]` is lost. `programme_lost` is the programme's stream.
    pub fn ltc_lost(&self, programme_lost: bool) -> u32 {
        let lamps = self.read();
        let own = lamps
            .ltc
            .iter()
            .filter(|lamp| lamp.lost.load(Ordering::Relaxed))
            .fold(0, |mask, lamp| mask | lamp.outputs);
        own | if programme_lost {
            lamps.programme_ltc
        } else {
            0
        }
    }

    /// Bit *i* is set when `OutputInfo.mtc[i]` is lost.
    pub fn mtc_lost(&self) -> u32 {
        self.read()
            .mtc
            .iter()
            .filter(|lamp| lamp.lost.load(Ordering::Relaxed))
            .fold(0, |mask, lamp| mask | lamp.outputs)
    }

    /// Whether the stream the metronome plays on is lost.
    pub fn click_lost(&self, programme_lost: bool) -> bool {
        let lamps = self.read();
        match &lamps.click {
            Some(lost) => lost.load(Ordering::Relaxed),
            None => lamps.programme_click && programme_lost,
        }
    }
}
