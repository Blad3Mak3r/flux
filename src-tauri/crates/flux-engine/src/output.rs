//! Real-time output: the audio stream that plays the programme and is the master clock, the LTC
//! that goes out on it or on a stream of its own, and the MIDI thread that sends MTC from that same
//! clock.
//!
//! What can be tested without hardware is kept apart from the device code: [`Renderer`] and
//! [`LtcFollower`] fill a buffer for any sample type, [`pick_config`] chooses a stream format, and
//! [`Snapshot`] turns the last published position into a current one. [`Output`] only wires them to
//! cpal and midir.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::{Serialize, Serializer};
use thiserror::Error;

use super::cue_routes::{CueInfo, CueRoutes};
use super::ltc::{LtcConfig, LtcStream};
use super::metronome::{ClickOut, SharedMetronome};
use super::mixer::{ClipSet, MIX_CHUNK, Programme, SharedLevels};
use super::mtc::MtcScheduler;
use super::transport::{Command, Transport, TransportState};
use crate::devices;
use flux_audio::ENGINE_RATE;
use flux_core::Clock;
use flux_project::{
    DEFAULT_LTC_LEVEL, FrameRate, LtcOutput, MtcOutput, OscDestination, OutputConfig, ProjectError,
    Tempo, Timeline,
};

const MIDI_CLIENT_NAME: &str = "FLUX";
/// The longest the MTC thread sleeps before it looks at the transport again.
pub(crate) const MTC_TICK: Duration = Duration::from_millis(1);
/// The last stretch before a quarter frame that the MTC thread spins instead of sleeping, because a
/// sleep can overshoot by more than this.
pub(crate) const MTC_SPIN: Duration = Duration::from_micros(200);
/// The audio thread keeps its stream alive and wakes up this often to see whether to stop.
const AUDIO_PARK: Duration = Duration::from_millis(50);
/// The published position is one buffer old at most; extrapolating further than this would only
/// hide a stalled stream.
pub(crate) const MAX_EXTRAPOLATION: Duration = Duration::from_millis(100);
/// A metronome on a device of its own starts over from the master's position only when it is this
/// far away (10 ms): a click is a short sound, so it must follow the master closely, but the
/// extrapolated position jitters by a millisecond or two and each start-over is a jump in the click.
const CLICK_SYNC_SAMPLES: u64 = ENGINE_RATE as u64 / 100;

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("audio device not found: {0}")]
    DeviceMissing(String),
    #[error("this system has no default audio output; choose a device in the Output tab")]
    NoDefaultDevice,
    #[error("{device} cannot play {channels} channels at {rate} Hz")]
    Unsupported {
        device: String,
        channels: u16,
        rate: u32,
    },
    #[error("audio stream: {0}")]
    Stream(String),
    #[error("stop microphone recording before playback")]
    Recording,
    #[error("the output is not running")]
    NotRunning,
    #[error(transparent)]
    Project(#[from] ProjectError),
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for OutputError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Stream(code) = self
            && code.starts_with("show.error.")
        {
            return serializer.serialize_str(code);
        }
        if matches!(self, Self::Recording) {
            serializer.serialize_str("voiceRecordingActive")
        } else {
            serializer.collect_str(self)
        }
    }
}

/// What the control side tells the audio callback.
#[derive(Debug)]
pub enum Message {
    Transport(Command),
    Show(crate::show::ShowCommand),
    /// The audio to play from now on. The callback only swaps it in; see [`Output`] for who frees
    /// the old one.
    Clips(Arc<ClipSet>),
}

/// A device that does not pace its callbacks (ALSA's null device) makes the transport run many
/// times faster than real time. This compares the audio rendered with the wall clock.
#[derive(Debug, Default)]
struct PaceWatch {
    window_start: Option<u64>,
    window_frames: u64,
}

/// Longer than this over real time and the clock is not a clock.
const MAX_PACE_RATIO: f64 = 1.5;
const PACE_WINDOW_NANOS: u64 = 1_000_000_000;

impl PaceWatch {
    /// Counts `frames` rendered at wall-clock time `now` (nanoseconds). Once a second has passed
    /// it returns whether the stream ran faster than real time over that second.
    fn observe(&mut self, now: u64, frames: u64) -> Option<bool> {
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
    show_fault: AtomicBool,
    pub(crate) show_generation: AtomicU64,
    pub(crate) show_plan: OnceLock<Arc<crate::show::ShowPlan>>,
    pub(crate) show_spans: OnceLock<mpsc::SyncSender<crate::show::ShowSpan>>,
    snapshot: Mutex<Snapshot>,
    base: Instant,
    lost: Arc<AtomicBool>,
    /// Which of the other outputs (LTC on a device of its own, the click, MTC ports) are lost.
    health: Health,
    unpaced: AtomicBool,
    /// How many times the show ran to its end by itself (not stopped by the user).
    shows_ended: AtomicU64,
    /// The faults the streams and the MTC thread report. Only counted here: a lane thread reads
    /// them and writes the log, because nothing on those threads may wait for a file.
    faults: [AtomicU64; 4],
    operator_route: Option<flux_project::AudioOutput>,
    audio_config: OutputConfig,
    audio_health: RwLock<Vec<crate::audio_routing::RouteHealth>>,
    operator_set: Mutex<Arc<ClipSet>>,
    operator_subscribers: Mutex<Vec<Sender<Arc<ClipSet>>>>,
    pending_plays: AtomicU64,
    operator_health: RwLock<Option<Arc<AtomicBool>>>,
    operator_invalid: AtomicBool,
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
            operator_set: Mutex::new(Arc::new(ClipSet::default())),
            operator_subscribers: Mutex::new(Vec::new()),
            pending_plays: AtomicU64::new(0),
        }
    }

    fn subscribe_operator(&self) -> (Arc<ClipSet>, Receiver<Arc<ClipSet>>) {
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

    fn update_operator(&self, set: Arc<ClipSet>) {
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
    fn mtc_failed(&self) {
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

    /// Nanoseconds since this clock was created.
    pub fn now(&self) -> u64 {
        self.base.elapsed().as_nanos() as u64
    }

    fn publish(&self, snapshot: Snapshot) {
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
    outputs: u32,
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
fn mask(outputs: impl IntoIterator<Item = usize>) -> u32 {
    outputs.into_iter().fold(0, |mask, index| mask | bit(index))
}

/// The bit of the output at `index`. Outputs past the 32nd have none: they are never shown as lost.
fn bit(index: usize) -> u32 {
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
struct LtcSend {
    /// 1-based interface channel.
    channel: u16,
    /// Linear peak level.
    level: f32,
    /// Latency offset in ms, positive = earlier.
    offset_ms: i32,
}

/// Groups outputs into one [`LtcChannel`] per distinct offset: outputs with the same offset share
/// a generator, as outputs on one device always did.
fn ltc_lanes(
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
    audio_lanes: Option<Vec<crate::audio_routing::AudioLane>>,
    operator_channels: Vec<usize>,
    transport: Transport,
    messages: Receiver<Message>,
    clips: Arc<ClipSet>,
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
            }
            if let Some(lanes) = &mut self.audio_lanes {
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
                        let sum = lanes.iter().map(|l| l.sample(channel, at)).sum::<f32>()
                            + click_on(channel);
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
trait Fill: Send + 'static {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64);
}

impl Fill for Renderer {
    fn fill<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        self.render(data, now);
    }
}

/// A continuous voice cursor for an independent device clock. Small clock drift is corrected
/// by bounded resampling, rather than skipping or repeating chunks of spoken words.
#[derive(Debug, Default, Clone)]
struct VoiceCursor {
    next: Option<f64>,
    step: f64,
}

impl VoiceCursor {
    fn follow(&mut self, target: u64, reset: bool) {
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

    fn mix(&mut self, clips: &ClipSet, left: &mut [f32], right: &mut [f32]) {
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
    audio_lanes: Option<Vec<crate::audio_routing::AudioLane>>,
    voice_cursor: VoiceCursor,
    operator_clips: Arc<ClipSet>,
    operator_updates: Receiver<Arc<ClipSet>>,
    operator_channels: Vec<usize>,
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

/// One entry of a device's `supported_output_configs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigRange {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
    pub format: SampleFormat,
}

/// Picks the stream format: the fewest channels that still cover `needed`, at `rate`, in a sample
/// format the renderer can write (32-bit float preferred, then 32 and 16-bit integer).
pub fn pick_config(ranges: &[ConfigRange], needed: u16, rate: u32) -> Option<(u16, SampleFormat)> {
    let preference = |format: SampleFormat| match format {
        SampleFormat::F32 => Some(0),
        SampleFormat::I32 => Some(1),
        SampleFormat::I16 => Some(2),
        _ => None,
    };
    ranges
        .iter()
        .filter(|range| {
            range.channels >= needed && (range.min_rate..=range.max_rate).contains(&rate)
        })
        .filter_map(|range| Some((range.channels, preference(range.format)?, range.format)))
        .min_by_key(|&(channels, preference, _)| (channels, preference))
        .map(|(channels, _, format)| (channels, format))
}

/// What a running output looks like to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub device: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// One entry per LTC output the project asks for, in the project's order.
    pub ltc: Vec<LtcInfo>,
    /// One entry per MIDI port the project asks for MTC on, in the project's order.
    pub mtc: Vec<MtcInfo>,
    /// Where the metronome plays; `None` when the project has no metronome output.
    pub metronome: Option<MetronomeInfo>,
    pub operator: Option<MetronomeInfo>,
    /// One entry per MIDI port and OSC destination the cue tracks of the timeline send to.
    pub cues: Vec<CueInfo>,
    /// Things that did not stop the output but the user should know, e.g. a missing MIDI port.
    pub warnings: Vec<String>,
}

/// How one LTC output ended up. When it did not open, `device` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LtcInfo {
    pub device: String,
    /// 1-based interface channel.
    pub channel: u16,
    pub ok: bool,
}

/// How the metronome ended up. When it did not open, `device` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetronomeInfo {
    pub device: String,
    pub ok: bool,
}

/// How one MTC output ended up. When it did not open, `port` is the name the project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MtcInfo {
    pub port: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub state: TransportState,
    /// Frame count of the timecode being played, including the project's start offset.
    pub frame: u64,
    pub position_samples: u64,
    /// The programme's audio device failed or went away.
    pub device_lost: bool,
    /// Bit *i* is set when `OutputInfo.ltc[i]` failed or went away. Only the first 32 outputs
    /// have a bit.
    pub ltc_lost: u32,
    /// Bit *i* is set when `OutputInfo.mtc[i]` failed or went away.
    pub mtc_lost: u32,
    /// The stream the metronome plays on failed or went away.
    pub metronome_lost: bool,
    pub operator_lost: bool,
    /// The device renders audio faster than real time (ALSA's null device does), so the position
    /// and the timecode are wrong. Pick another device.
    pub clock_unpaced: bool,
}

/// Reads where the transport is. It does not borrow the [`Output`], so a UI thread can hold one.
#[derive(Debug, Clone)]
pub struct PositionReader {
    shared: Arc<SharedClock>,
    clock: Clock,
    start: u64,
}

impl PositionReader {
    pub fn show(&self) -> Option<(Arc<crate::show::ShowPlan>, crate::show::ShowTick)> {
        Some((
            Arc::clone(self.shared.show_plan.get()?),
            self.shared.load().show.unwrap_or_default(),
        ))
    }

    pub fn show_snapshot(
        &self,
    ) -> Option<(Arc<crate::show::ShowPlan>, crate::show::ShowTick, Position)> {
        let snapshot = self.shared.load();
        Some((
            Arc::clone(self.shared.show_plan.get()?),
            snapshot.show.unwrap_or_default(),
            self.position_at(snapshot),
        ))
    }

    pub fn audio_routes(&self) -> Vec<crate::audio_routing::AudioRouteState> {
        self.shared
            .audio_health
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|r| r.state())
            .collect()
    }

    /// How many times the show has run to its end by itself: a change says it just ended, which
    /// the position alone cannot (a stop from the user looks the same).
    pub fn shows_ended(&self) -> u64 {
        self.shared.shows_ended()
    }

    /// What went wrong on the streams and the MIDI port since the output started.
    pub fn faults(&self) -> Faults {
        self.shared.faults()
    }

    pub fn position(&self) -> Position {
        self.position_at(self.shared.load())
    }

    fn position_at(&self, snapshot: Snapshot) -> Position {
        let (clock, start) = self
            .shared
            .show_timecode(&snapshot)
            .unwrap_or((self.clock, self.start));
        let position = snapshot.estimated_position(self.shared.now(), clock);
        let device_lost = self.shared.is_lost();
        let health = self.shared.health();
        Position {
            state: snapshot.state,
            frame: start + clock.samples_to_frames(position),
            position_samples: position,
            device_lost,
            ltc_lost: health.ltc_lost(device_lost),
            mtc_lost: health.mtc_lost(),
            metronome_lost: health.click_lost(device_lost),
            operator_lost: self
                .shared
                .operator_health
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .is_some_and(|lost| lost.load(Ordering::Relaxed)),
            clock_unpaced: self.shared.is_unpaced(),
        }
    }
}

/// What `Output::reconfigure` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reconfigured {
    /// The LTC and MTC outputs now match the project; the programme's stream was not touched.
    Applied { info: OutputInfo },
    /// The change reaches the programme's stream: the output has to be opened again.
    NeedsRestart,
}

/// What the programme's stream is opened with. A project that asks for anything else needs the
/// stream reopened. The positions of the LTC outputs in the project are not part of it: they only
/// say which row of the UI a stream belongs to.
#[derive(Debug, Clone, PartialEq)]
struct MasterKey {
    driver: Option<String>,
    device_hint: String,
    /// 1-based interface channels of the programme.
    programme: Vec<u16>,
    /// How many channels the stream has to open.
    needed: u16,
    /// The LTC outputs written into the programme's stream.
    ltc: Vec<LtcSend>,
    rate: FrameRate,
    start: u64,
}

/// Where the metronome plays and how, which is part of every stream it can play on.
#[derive(Debug, Clone, PartialEq)]
struct ClickKey {
    device: String,
    channels: Vec<u16>,
    gain_db: f32,
    place: ClickPlaceKey,
}

#[derive(Debug, Clone, PartialEq)]
enum ClickPlaceKey {
    Programme,
    /// The stream of the LTC device the group was opened by.
    Group(String),
    Own,
}

/// An LTC device that is not the programme's, with the metronome when it plays there too.
struct FollowerWant {
    group: LtcGroup,
    click: Option<ClickSpec>,
}

/// What a project asks for, split into what needs the programme's stream and what does not.
struct Plan {
    master: MasterKey,
    click: Option<ClickKey>,
    /// The LTC outputs on the programme's stream.
    programme_ltc: Vec<LtcSlot>,
    programme_click: Option<ClickSpec>,
    followers: Vec<FollowerWant>,
    /// The metronome's device, when it carries nothing else.
    own_click: Option<(String, ClickSpec)>,
    /// The metronome, shared with every stream that plays it.
    metronome: Option<Arc<SharedMetronome>>,
    mtc: Vec<MtcOutput>,
    /// How many LTC outputs the project has.
    ltc_outputs: usize,
    unassigned_ltc: Vec<(usize, u16)>,
}

impl Plan {
    /// `metronome` is the one a running output already uses, so a stream opened later keeps
    /// following its tempo. `same` tells whether two device name hints open the same device.
    fn build(
        output: &OutputConfig,
        timeline: &Timeline,
        metronome: Option<Arc<SharedMetronome>>,
        same: impl Fn(&str, &str) -> bool,
    ) -> Self {
        // An empty device name is the system's default output, so a project without any device
        // chosen still plays. A project written before `output.audio` existed carries the
        // programme on the first LTC device.
        let device_hint = match (&output.audio, output.ltc.first()) {
            (Some(audio), _) => audio.device_name.clone(),
            (None, Some(ltc)) => ltc.device_name.clone(),
            (None, None) => String::new(),
        };
        let programme: Vec<u16> = output
            .audio
            .as_ref()
            .map(|audio| audio.channels.clone())
            .filter(|channels| !channels.is_empty())
            .unwrap_or_else(|| vec![1, 2]);
        // LTC shares the programme's stream unless it names another device, which then gets a
        // stream of its own.
        let LtcPlan {
            shared: programme_ltc,
            mut own,
        } = plan_ltc(&output.ltc, &device_hint, &same);
        // The metronome has an output of its own. It plays on the programme's stream when it names
        // the programme's device, on the stream of an LTC device it shares, or else on a stream
        // of its own: no device is opened twice.
        let operator_channels = output
            .operator
            .as_ref()
            .filter(|_| output.audio_routes.is_empty())
            .filter(|r| same(&r.device_name, &device_hint))
            .map_or_else(Vec::new, |r| r.channels.clone());
        if let Some(route) = &output.operator
            && output.audio_routes.is_empty()
            && !same(&route.device_name, &device_hint)
            && !own.iter().any(|g| same(&g.device, &route.device_name))
        {
            own.push(LtcGroup {
                device: route.device_name.clone(),
                slots: Vec::new(),
            });
        }
        for bus in output
            .audio_routes
            .iter()
            .filter(|b| crate::audio_routing::used(output, b))
        {
            if let Some(route) = &bus.route
                && route.device_name != flux_project::UNASSIGNED_DEVICE
                && !same(&route.device_name, &device_hint)
                && !own.iter().any(|g| same(&g.device, &route.device_name))
            {
                own.push(LtcGroup {
                    device: route.device_name.clone(),
                    slots: Vec::new(),
                });
            }
        }
        let shared_metronome = output.metronome.as_ref().map(|metronome_output| {
            metronome.unwrap_or_else(|| {
                Arc::new(SharedMetronome::new(
                    timeline.tempo,
                    metronome_output.gain_db,
                ))
            })
        });
        let click = output
            .metronome
            .as_ref()
            .zip(shared_metronome.as_ref())
            .map(|(metronome, shared)| {
                let channels = if metronome.channels.is_empty() {
                    vec![1, 2]
                } else {
                    metronome.channels.clone()
                };
                let place = if same(&metronome.device_name, &device_hint) {
                    ClickPlace::Programme
                } else if let Some(group) = own
                    .iter()
                    .position(|group| same(&group.device, &metronome.device_name))
                {
                    ClickPlace::Group(group)
                } else {
                    ClickPlace::Own
                };
                let spec = ClickSpec {
                    shared: Arc::clone(shared),
                    channels,
                };
                (
                    metronome.device_name.clone(),
                    metronome.gain_db,
                    place,
                    spec,
                )
            });
        let click_key = click
            .as_ref()
            .map(|(device, gain_db, place, spec)| ClickKey {
                device: device.clone(),
                channels: spec.channels.clone(),
                gain_db: *gain_db,
                place: match place {
                    ClickPlace::Programme => ClickPlaceKey::Programme,
                    ClickPlace::Group(at) => ClickPlaceKey::Group(own[*at].device.clone()),
                    ClickPlace::Own => ClickPlaceKey::Own,
                },
            });
        let click_for = |wanted: ClickPlace| {
            click
                .as_ref()
                .filter(|(_, _, place, _)| *place == wanted)
                .map(|(_, _, _, spec)| spec.clone())
        };
        let programme_click = click_for(ClickPlace::Programme);
        let own_click = click_for(ClickPlace::Own)
            .zip(click.as_ref())
            .map(|(spec, (device, ..))| (device.clone(), spec));
        let needed = programme_ltc
            .iter()
            .map(|slot| slot.channel)
            .chain(programme.iter().copied())
            .chain(operator_channels.iter().copied())
            .chain(
                output
                    .audio_routes
                    .iter()
                    .filter(|b| crate::audio_routing::used(output, b))
                    .filter_map(|b| b.route.as_ref())
                    .filter(|r| same(&r.device_name, &device_hint))
                    .flat_map(|r| r.channels.iter().copied()),
            )
            .chain(
                programme_click
                    .iter()
                    .flat_map(|spec| spec.channels.iter().copied()),
            )
            .max()
            .unwrap_or(2);
        let followers = own
            .into_iter()
            .enumerate()
            .map(|(index, group)| FollowerWant {
                group,
                click: click_for(ClickPlace::Group(index)),
            })
            .collect();
        Self {
            master: MasterKey {
                driver: output.driver.clone(),
                device_hint,
                programme,
                needed,
                ltc: sends_of(&programme_ltc),
                rate: timeline.timecode.frame_rate,
                start: timeline.timecode.start,
            },
            click: click_key,
            programme_ltc,
            programme_click,
            followers,
            own_click,
            metronome: shared_metronome,
            mtc: output.mtc.clone(),
            ltc_outputs: output.ltc.len(),
            unassigned_ltc: output
                .ltc
                .iter()
                .enumerate()
                .filter(|(_, r)| r.device_name == flux_project::UNASSIGNED_DEVICE)
                .map(|(i, r)| (i, r.channel))
                .collect(),
        }
    }
}

/// What the streams and threads besides the programme's are opened with.
struct BranchCtx {
    driver: Option<String>,
    /// The generator runs at full scale and every channel applies its own level.
    ltc_config: LtcConfig,
    rate: FrameRate,
    start: u64,
    clock: Clock,
}

impl BranchCtx {
    fn new(driver: Option<String>, timeline: &Timeline) -> Self {
        let ltc_config = LtcConfig {
            level: 1.0,
            ..LtcConfig::default()
        };
        let rate = timeline.timecode.frame_rate;
        Self {
            driver,
            clock: Clock::new(ltc_config.sample_rate, rate),
            ltc_config,
            rate,
            start: timeline.timecode.start,
        }
    }
}

/// A stream (LTC and/or the click) on a device that is not the programme's.
struct Follower {
    /// The device name hint it was opened by.
    device: String,
    sends: Vec<LtcSend>,
    with_click: bool,
    /// The name of the device that opened.
    opened: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// A thread sending MTC to a MIDI port.
struct MtcBranch {
    hint: String,
    index: usize,
    offset_ms: i32,
    /// The system's own position of the port.
    at: usize,
    name: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Stops a stream or thread of its own and waits for it.
fn shut(stop: &AtomicBool, handle: JoinHandle<()>) {
    stop.store(true, Ordering::Relaxed);
    handle.thread().unpark();
    let _ = handle.join();
}

impl Follower {
    fn shut(self) {
        shut(&self.stop, self.handle);
    }
}

impl MtcBranch {
    fn shut(self) {
        shut(&self.stop, self.handle);
    }
}

/// For each wanted item, the running one that already does it (each used once), and the running
/// ones nothing wants any more.
fn pair<W, R>(
    wanted: &[W],
    running: &[R],
    same: impl Fn(&W, &R) -> bool,
) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut taken = vec![false; running.len()];
    let paired = wanted
        .iter()
        .map(|want| {
            let at = (0..running.len()).find(|&at| !taken[at] && same(want, &running[at]))?;
            taken[at] = true;
            Some(at)
        })
        .collect();
    let unused = (0..running.len()).filter(|&at| !taken[at]).collect();
    (paired, unused)
}

/// What the branches ended up as, for the UI.
struct Opened {
    ltc: Vec<LtcInfo>,
    mtc: Vec<MtcInfo>,
    metronome: Option<MetronomeInfo>,
    operator: Option<MetronomeInfo>,
}

/// The LTC and click streams on other devices and the MTC threads of a running output. Applying a
/// plan keeps what already does what the plan asks, and only closes and opens the rest.
#[derive(Default)]
struct Branches {
    followers: Vec<Follower>,
    /// The metronome's stream, when its device carries nothing else.
    click: Option<Follower>,
    mtc: Vec<MtcBranch>,
}

impl Branches {
    fn shut_all(&mut self) {
        for follower in self.followers.drain(..).chain(self.click.take()) {
            follower.shut();
        }
        for port in self.mtc.drain(..) {
            port.shut();
        }
    }

    fn apply(
        &mut self,
        plan: &Plan,
        ctx: &BranchCtx,
        shared: &Arc<SharedClock>,
        programme: &str,
        warnings: &mut Vec<String>,
    ) -> Opened {
        let mut lamps = Lamps {
            programme_ltc: mask(
                plan.programme_ltc
                    .iter()
                    .flat_map(|slot| slot.outputs.iter().copied()),
            ),
            programme_click: plan.programme_click.is_some(),
            ..Lamps::default()
        };
        // One entry per project output, in the project's order, whatever stream it landed on.
        let mut ltc_info: Vec<Option<LtcInfo>> = vec![None; plan.ltc_outputs];
        for &(index, channel) in &plan.unassigned_ltc {
            ltc_info[index] = Some(LtcInfo {
                device: String::new(),
                channel,
                ok: false,
            });
        }
        let mut place = |slots: &[LtcSlot], device: &str, ok: bool| {
            for slot in slots {
                for &index in &slot.outputs {
                    ltc_info[index] = Some(LtcInfo {
                        device: device.to_owned(),
                        channel: slot.channel,
                        ok,
                    });
                }
            }
        };
        place(&plan.programme_ltc, programme, true);
        let mut metronome_info = plan.click.as_ref().map(|key| MetronomeInfo {
            device: if key.place == ClickPlaceKey::Programme {
                programme.to_owned()
            } else {
                key.device.clone()
            },
            ok: key.place == ClickPlaceKey::Programme,
        });

        // What each stream is asked to do, as it is compared with the ones running.
        let asked: Vec<(&str, Vec<LtcSend>, bool)> = plan
            .followers
            .iter()
            .map(|want| {
                (
                    want.group.device.as_str(),
                    sends_of(&want.group.slots),
                    want.click.is_some(),
                )
            })
            .collect();
        let (paired, unused) = pair(
            &asked,
            &self.followers,
            |(device, sends, click), running| {
                running.device == *device && running.sends == *sends && running.with_click == *click
            },
        );
        let mut old: Vec<Option<Follower>> = std::mem::take(&mut self.followers)
            .into_iter()
            .map(Some)
            .collect();
        // A device the new streams need may be one a closing stream holds: close first.
        for at in unused {
            if let Some(follower) = old[at].take() {
                follower.shut();
            }
        }
        for (want, paired) in plan.followers.iter().zip(paired) {
            let group = &want.group;
            let opened = match paired.and_then(|at| old[at].take()) {
                Some(running) => Ok(running),
                None => open_follower(
                    "flux-ltc",
                    &group.device,
                    sends_of(&group.slots),
                    want.click.clone(),
                    ctx,
                    shared,
                ),
            };
            match opened {
                Ok(follower) => {
                    place(&group.slots, &follower.opened, true);
                    let outputs = group
                        .slots
                        .iter()
                        .flat_map(|slot| slot.outputs.iter().copied());
                    lamps.ltc.push(Lamp::new(outputs, &follower.lost));
                    if let (true, Some(info)) = (want.click.is_some(), &mut metronome_info) {
                        *info = MetronomeInfo {
                            device: follower.opened.clone(),
                            ok: true,
                        };
                        lamps.click = Some(Arc::clone(&follower.lost));
                    }
                    self.followers.push(follower);
                }
                Err(err) => {
                    warnings.push(format!("LTC is off on {}, {err}", group.device));
                    place(&group.slots, &group.device, false);
                    if want.click.is_some() {
                        warnings.push(format!("The metronome is off on {}, {err}", group.device));
                    }
                }
            }
        }

        // A device that carries nothing but the metronome gets a stream that plays only the click.
        let running_click = match self.click.take() {
            Some(running)
                if plan
                    .own_click
                    .as_ref()
                    .is_some_and(|(device, _)| running.device == *device) =>
            {
                Some(running)
            }
            Some(running) => {
                running.shut();
                None
            }
            None => None,
        };
        if let Some((device, spec)) = &plan.own_click {
            let opened = match running_click {
                Some(running) => Ok(running),
                None => open_follower(
                    "flux-click",
                    device,
                    Vec::new(),
                    Some(spec.clone()),
                    ctx,
                    shared,
                ),
            };
            match opened {
                Ok(follower) => {
                    lamps.click = Some(Arc::clone(&follower.lost));
                    metronome_info = Some(MetronomeInfo {
                        device: follower.opened.clone(),
                        ok: true,
                    });
                    self.click = Some(follower);
                }
                Err(err) => warnings.push(format!("The metronome is off on {device}, {err}")),
            }
        }

        let operator = shared.operator_route.as_ref().map(|route| {
            let primary = same_device(
                ctx.driver.as_deref(),
                &route.device_name,
                &plan.master.device_hint,
            );
            let follower = self
                .followers
                .iter()
                .find(|f| same_device(ctx.driver.as_deref(), &route.device_name, &f.device));
            let flag = if primary && shared.operator_invalid.load(Ordering::Relaxed) {
                Arc::new(AtomicBool::new(true))
            } else if primary {
                Arc::clone(&shared.lost)
            } else {
                follower.map_or_else(|| Arc::new(AtomicBool::new(true)), |f| Arc::clone(&f.lost))
            };
            let ok = !flag.load(Ordering::Relaxed);
            *shared
                .operator_health
                .write()
                .unwrap_or_else(PoisonError::into_inner) = Some(flag);
            MetronomeInfo {
                device: if primary {
                    programme.into()
                } else {
                    route.device_name.clone()
                },
                ok,
            }
        });
        let mtc = self.apply_mtc(plan, ctx, shared, warnings, &mut lamps);
        shared.health().replace(lamps);
        Opened {
            ltc: ltc_info.into_iter().flatten().collect(),
            mtc,
            metronome: metronome_info,
            operator,
        }
    }

    /// One thread per port, and a port listed twice (or named two ways) shares the first one's
    /// result. Returns one entry per MTC output of the project.
    fn apply_mtc(
        &mut self,
        plan: &Plan,
        ctx: &BranchCtx,
        shared: &Arc<SharedClock>,
        warnings: &mut Vec<String>,
        lamps: &mut Lamps,
    ) -> Vec<MtcInfo> {
        // The outputs naming the same port the same way are one.
        let mut first_of: HashMap<(&str, usize), usize> = HashMap::new();
        let mut distinct: Vec<&MtcOutput> = Vec::new();
        let group_of: Vec<usize> = plan
            .mtc
            .iter()
            .map(|mtc| {
                *first_of
                    .entry((mtc.port_name.as_str(), mtc.port_index))
                    .or_insert_with(|| {
                        distinct.push(mtc);
                        distinct.len() - 1
                    })
            })
            .collect();

        let (paired, unused) = pair(&distinct, &self.mtc, |want, running| {
            running.hint == want.port_name
                && running.index == want.port_index
                && running.offset_ms == want.offset_ms.unwrap_or(0)
        });
        let mut old: Vec<Option<MtcBranch>> = std::mem::take(&mut self.mtc)
            .into_iter()
            .map(Some)
            .collect();
        for at in unused {
            if let Some(port) = old[at].take() {
                port.shut();
            }
        }
        // What each distinct port ended up as: the branch that sends to it, or why it is off.
        let mut resolved: Vec<Option<Result<usize, MtcInfo>>> =
            distinct.iter().map(|_| None).collect();
        let mut running: Vec<MtcBranch> = Vec::new();
        // The ports kept come first, so a port that is new and turns out to be one of them
        // shares it instead of opening it twice.
        for (at, paired) in paired.into_iter().enumerate() {
            if let Some(port) = paired.and_then(|from| old[from].take()) {
                running.push(port);
                resolved[at] = Some(Ok(running.len() - 1));
            }
        }
        for (at, want) in distinct.iter().enumerate() {
            if resolved[at].is_some() {
                continue;
            }
            let result = open_mtc(want, ctx, shared, &mut running, warnings);
            resolved[at] = Some(result);
        }

        let branch_of = |output: usize| match &resolved[group_of[output]] {
            Some(Ok(branch)) => Some(*branch),
            _ => None,
        };
        for (branch, port) in running.iter().enumerate() {
            let outputs = (0..plan.mtc.len()).filter(|&output| branch_of(output) == Some(branch));
            lamps.mtc.push(Lamp::new(outputs, &port.lost));
        }
        let info = (0..plan.mtc.len())
            .map(|output| match &resolved[group_of[output]] {
                Some(Ok(branch)) => MtcInfo {
                    port: running[*branch].name.clone(),
                    ok: true,
                },
                Some(Err(info)) => info.clone(),
                None => MtcInfo {
                    port: plan.mtc[output].port_name.clone(),
                    ok: false,
                },
            })
            .collect();
        self.mtc = running;
        info
    }
}

/// Opens the MIDI port `want` names and starts its MTC thread, pushing it onto `running`. A port
/// that turns out to be one `running` already sends to is not opened twice. Returns the position
/// in `running` of the branch that sends to it, or why it is off.
fn open_mtc(
    want: &MtcOutput,
    ctx: &BranchCtx,
    shared: &Arc<SharedClock>,
    running: &mut Vec<MtcBranch>,
    warnings: &mut Vec<String>,
) -> Result<usize, MtcInfo> {
    let hint = want.port_name.as_str();
    if hint.is_empty() {
        return Err(MtcInfo {
            port: String::new(),
            ok: false,
        });
    }
    let (position, name, connection) = match connect_midi(hint, want.port_index, MIDI_CLIENT_NAME) {
        Ok(opened) => opened,
        Err(reason) => {
            warnings.push(format!("MTC is off, {reason}"));
            return Err(MtcInfo {
                port: hint.to_owned(),
                ok: false,
            });
        }
    };
    if let Some(shared_with) = running.iter().position(|port| port.at == position) {
        return Ok(shared_with);
    }
    let offset_ms = want.offset_ms.unwrap_or(0);
    let scheduler = MtcScheduler::new(ctx.clock, ctx.rate, ctx.start)
        .with_offset(ctx.clock.ms_to_samples(offset_ms));
    let (stop, lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let link = PortLink {
        connection,
        hint: hint.to_owned(),
        index: want.port_index,
    };
    let (clock, shared, flags) = (
        ctx.clock,
        Arc::clone(shared),
        (Arc::clone(&stop), Arc::clone(&lost)),
    );
    let spawned = thread::Builder::new()
        .name("flux-mtc".into())
        .spawn(move || run_mtc(scheduler, clock, link, shared, flags.0, flags.1));
    match spawned {
        Ok(handle) => {
            running.push(MtcBranch {
                hint: hint.to_owned(),
                index: want.port_index,
                offset_ms,
                at: position,
                name,
                stop,
                lost,
                handle,
            });
            Ok(running.len() - 1)
        }
        Err(err) => {
            warnings.push(format!("MTC is off on {name}: {err}"));
            Err(MtcInfo {
                port: name,
                ok: false,
            })
        }
    }
}

/// Opens the stream of an LTC (or click) device that is not the programme's.
fn open_follower(
    name: &str,
    device: &str,
    sends: Vec<LtcSend>,
    click: Option<ClickSpec>,
    ctx: &BranchCtx,
    shared: &Arc<SharedClock>,
) -> Result<Follower, OutputError> {
    let with_click = click.is_some();
    let spec = LtcSpec {
        driver: ctx.driver.clone(),
        device_hint: device.to_owned(),
        ltc: sends.clone(),
        click,
        config: ctx.ltc_config,
        rate: ctx.rate,
        start: ctx.start,
        clock: ctx.clock,
    };
    let (stop, lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (handle, opened) = spawn_follower(name, &stop, shared, &lost, spec)?;
    Ok(Follower {
        device: device.to_owned(),
        sends,
        with_click,
        opened,
        stop,
        lost,
        handle,
    })
}

/// The audio API and device name hint the programme's stream was opened with.
#[derive(Debug, Clone)]
pub struct ProgrammeDevice {
    driver: Option<String>,
    hint: String,
}

impl ProgrammeDevice {
    /// Whether the device is there to be opened.
    pub fn is_present(&self) -> bool {
        devices::open_audio(&self.hint, self.driver.as_deref()).is_some()
    }
}

/// A running output. Dropping it stops the threads and closes the devices.
pub struct Output {
    messages: Sender<Message>,
    /// Every clip set ever sent. The audio callback swaps sets in and must never be the one to free
    /// the old one, so this side keeps them alive until the output stops.
    clip_sets: Mutex<Vec<Arc<ClipSet>>>,
    /// The faders and peak meters of the clip set being played.
    levels: SharedLevels,
    reader: PositionReader,
    /// The tempo the metronome clicks to, when the project has a metronome output.
    metronome: Option<Arc<SharedMetronome>>,
    /// Stops the programme's stream and the cue threads. The other streams and MTC threads each
    /// have a flag of their own, so one can be closed while the rest keep running.
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// The LTC and click streams on other devices and the MTC threads.
    branches: Branches,
    /// What the programme's stream and the metronome were opened with: a project that asks for
    /// anything else needs the output opened again.
    master: MasterKey,
    click: Option<ClickKey>,
    /// Why the programme could not be built fully, and why the cue tracks could not be opened.
    programme_warnings: Vec<String>,
    cue_warnings: Vec<String>,
    /// The threads that fire the MIDI and OSC cue tracks.
    cues: CueRoutes,
    /// Whether the cue tracks fire at all. Auditioning in Edit on a preview device does not.
    fire_cues: bool,
    info: OutputInfo,
    programme_device: ProgrammeDevice,
}

impl Output {
    /// Opens the devices a project's `output` section names and starts the stream, stopped at
    /// the timeline start. The audio device is required because its clock drives everything; a
    /// missing MIDI port only produces a warning. With `fire_cues` off (an audition) the MIDI and OSC
    /// tracks stay silent.
    pub fn start(
        output: &OutputConfig,
        timeline: &Timeline,
        programme: Programme,
        fire_cues: bool,
    ) -> Result<Self, OutputError> {
        let driver = output.driver.as_deref();
        let plan = Plan::build(output, timeline, None, |a, b| same_device(driver, a, b));
        let Plan {
            master,
            click,
            programme_ltc,
            programme_click,
            metronome,
            ..
        } = &plan;

        let ctx = BranchCtx::new(output.driver.clone(), timeline);
        let BranchCtx {
            ltc_config,
            clock,
            rate,
            start,
            ..
        } = ctx;
        let mut shared_clock = SharedClock::new();
        shared_clock.operator_route = output.operator.clone();
        shared_clock.audio_config = output.clone();
        *shared_clock
            .audio_health
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner) = output
            .audio_routes
            .iter()
            .map(|b| {
                use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
                RouteHealth {
                    state: AudioRouteState {
                        id: b.id.clone(),
                        device: b
                            .route
                            .as_ref()
                            .map_or_else(String::new, |r| r.device_name.clone()),
                        channels: b
                            .route
                            .as_ref()
                            .map_or_else(Vec::new, |r| r.channels.clone()),
                        status: if !crate::audio_routing::used(output, b) {
                            AudioRouteStatus::Unused
                        } else if crate::audio_routing::conflict(output, b, |a, b| {
                            same_device(driver, a, b)
                        }) {
                            AudioRouteStatus::Conflict
                        } else {
                            AudioRouteStatus::Missing
                        },
                    },
                    lost: None,
                }
            })
            .collect();
        let shared = Arc::new(shared_clock);
        let stop = Arc::new(AtomicBool::new(false));
        let (messages, message_rx) = mpsc::channel();
        let Programme { set, warnings } = programme;
        let clips = Arc::new(set);
        shared.update_operator(Arc::clone(&clips));
        let levels = SharedLevels::new(clips.levels());

        let programme_device = ProgrammeDevice {
            driver: output.driver.clone(),
            hint: master.device_hint.clone(),
        };
        let spec = AudioSpec {
            driver: output.driver.clone(),
            device_hint: master.device_hint.clone(),
            needed: master.needed,
            programme: master.programme.clone(),
            clips: Arc::clone(&clips),
            ltc: sends_of(programme_ltc),
            click: programme_click.clone(),
            ltc_config,
            rate,
            start,
            clock,
        };
        // The programme's stream cannot be reopened in place: the renderer owns the receiver of
        // the transport's commands. A new output takes over when its device is back.
        let beat: Arc<AtomicU64> = Arc::default();
        let watch = Watch {
            beat: Arc::clone(&beat),
            lost: shared.lost_flag(),
            retry: false,
        };
        let (audio_thread, audio) = spawn_stream("flux-audio", Arc::clone(&stop), watch, {
            let shared = Arc::clone(&shared);
            let mut once = Some((spec, message_rx));
            move || {
                let (spec, messages) = once.take().ok_or(OutputError::NotRunning)?;
                open_stream(spec, messages, Arc::clone(&shared), Arc::clone(&beat))
            }
        })?;

        let mut branches = Branches::default();
        let mut secondary_warnings = Vec::new();
        let opened = branches.apply(&plan, &ctx, &shared, &audio.device, &mut secondary_warnings);

        // The cue tracks: one thread per MIDI port and per OSC destination.
        let mut cues = CueRoutes::new(clock, Arc::clone(&shared), Arc::clone(&stop));
        let mut cue_warnings = Vec::new();
        let cue_info = if fire_cues {
            cues.apply(timeline, &output.osc, &mut cue_warnings)
        } else {
            Vec::new()
        };

        let info = OutputInfo {
            device: audio.device,
            sample_rate: ltc_config.sample_rate,
            channels: audio.channels,
            ltc: opened.ltc,
            mtc: opened.mtc,
            metronome: opened.metronome,
            operator: opened.operator,
            cues: cue_info,
            warnings: [&warnings, &secondary_warnings, &cue_warnings]
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
        };
        Ok(Self {
            messages,
            clip_sets: Mutex::new(vec![clips]),
            levels,
            reader: PositionReader {
                shared,
                clock,
                start,
            },
            metronome: metronome.clone(),
            stop,
            threads: vec![audio_thread],
            branches,
            master: master.clone(),
            click: click.clone(),
            programme_warnings: warnings,
            cue_warnings,
            cues,
            fire_cues,
            info,
            programme_device,
        })
    }

    pub fn install_show(
        &mut self,
        plan: Arc<crate::show::ShowPlan>,
        destinations: &[OscDestination],
    ) -> Result<(), OutputError> {
        if !self.info.warnings.is_empty()
            || self.reader.audio_routes().iter().any(|r| {
                matches!(
                    r.status,
                    crate::audio_routing::AudioRouteStatus::Missing
                        | crate::audio_routing::AudioRouteStatus::Conflict
                        | crate::audio_routing::AudioRouteStatus::Lost
                )
            })
        {
            return Err(OutputError::Stream("show.error.outputs".into()));
        }
        self.info.cues.clear();
        for entry in &plan.entries {
            let clock = Clock::new(ENGINE_RATE, entry.timeline.timecode.frame_rate);
            for midi in crate::midi_cue::plan(&entry.timeline, &clock) {
                let info = CueInfo {
                    kind: crate::cue_routes::CueDestination::Midi,
                    target: midi.port.port_name,
                    ok: true,
                };
                if !self.info.cues.contains(&info) {
                    self.info.cues.push(info);
                }
            }
            for osc in crate::osc::plan(&entry.timeline, destinations, &clock) {
                let info = CueInfo {
                    kind: crate::cue_routes::CueDestination::Osc,
                    target: osc.destination.name,
                    ok: true,
                };
                if !self.info.cues.contains(&info) {
                    self.info.cues.push(info);
                }
            }
        }
        let sender = crate::show_events::start(
            Arc::clone(&plan),
            destinations,
            Arc::clone(&self.stop),
            Arc::clone(&self.reader.shared),
        )?;
        self.reader
            .shared
            .show_spans
            .set(sender)
            .map_err(|_| OutputError::Stream("show.error.alreadyPrepared".into()))?;
        self.reader
            .shared
            .show_plan
            .set(plan)
            .map_err(|_| OutputError::Stream("show.error.alreadyPrepared".into()))?;
        Ok(())
    }

    pub fn show_command(&self, command: crate::show::ShowCommand) -> Result<(), OutputError> {
        self.messages
            .send(Message::Show(command))
            .map_err(|_| OutputError::NotRunning)
    }

    /// Follows a change of the project's output settings while the output runs, without touching
    /// the programme's stream: the LTC and click streams on other devices and the MTC threads
    /// that are no longer wanted are closed, the new ones opened, and the rest keep running.
    /// When the change reaches the programme's stream (its device, channels, the LTC on it, the
    /// metronome, the frame rate or start) nothing is touched and the output must be opened again.
    pub fn reconfigure(&mut self, output: &OutputConfig, timeline: &Timeline) -> Reconfigured {
        let driver = output.driver.as_deref();
        let plan = Plan::build(output, timeline, self.metronome.clone(), |a, b| {
            same_device(driver, a, b)
        });
        let ctx = BranchCtx::new(output.driver.clone(), timeline);
        if plan.master != self.master
            || plan.click != self.click
            || output.operator != self.reader.shared.operator_route
            || output.audio_routes != self.reader.shared.audio_config.audio_routes
            || output.track_routes != self.reader.shared.audio_config.track_routes
            || (!output.audio_routes.is_empty()
                && output.ltc != self.reader.shared.audio_config.ltc)
        {
            return Reconfigured::NeedsRestart;
        }
        let shared = Arc::clone(&self.reader.shared);
        let mut secondary_warnings = Vec::new();
        let device = self.info.device.clone();
        let opened = self
            .branches
            .apply(&plan, &ctx, &shared, &device, &mut secondary_warnings);
        self.info.ltc = opened.ltc;
        self.info.mtc = opened.mtc;
        self.info.metronome = opened.metronome;
        self.info.operator = opened.operator;
        self.info.warnings = [
            &self.programme_warnings,
            &secondary_warnings,
            &self.cue_warnings,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        Reconfigured::Applied {
            info: self.info.clone(),
        }
    }

    /// Follows an edit of the cue tracks while the show runs: the schedules are replaced, carrying
    /// on from the position, and a port or destination that is new is opened now. Returns the
    /// reasons for those that could not be.
    pub fn set_events(&mut self, timeline: &Timeline, osc: &[OscDestination]) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.fire_cues {
            self.cues.apply(timeline, osc, &mut warnings);
        }
        warnings
    }

    /// Follows an edit of the active timeline's tempo or click switch while the show runs.
    /// Does nothing when the project has no metronome output.
    pub fn set_tempo(&self, tempo: Tempo) {
        if let Some(metronome) = &self.metronome {
            metronome.set(tempo);
        }
    }

    /// The device the programme plays on, to find out whether it is back after it was unplugged.
    pub fn programme_device(&self) -> ProgrammeDevice {
        self.programme_device.clone()
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    pub fn send(&self, command: Command) -> Result<(), OutputError> {
        let play = matches!(command, Command::Play);
        if play {
            self.reader
                .shared
                .pending_plays
                .fetch_add(1, Ordering::AcqRel);
        }
        self.messages
            .send(Message::Transport(command))
            .map_err(|_| {
                if play {
                    self.reader
                        .shared
                        .pending_plays
                        .fetch_sub(1, Ordering::AcqRel);
                }
                OutputError::NotRunning
            })
    }

    pub fn playing_or_pending(&self) -> bool {
        self.reader.shared.pending_plays.load(Ordering::Acquire) > 0
            || self.reader.shared.load().state == TransportState::Playing
    }

    /// Replaces the audio being played, after the project's clips changed.
    pub fn set_clips(&self, set: ClipSet) -> Result<(), OutputError> {
        let set = Arc::new(set);
        self.reader.shared.update_operator(Arc::clone(&set));
        self.levels.replace(set.levels());
        let mut sets = self
            .clip_sets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        prune_clip_sets(&mut sets);
        sets.push(Arc::clone(&set));
        drop(sets);
        self.messages
            .send(Message::Clips(set))
            .map_err(|_| OutputError::NotRunning)
    }

    pub fn reader(&self) -> PositionReader {
        self.reader.clone()
    }

    /// Where to move a track's fader and read its peak, whichever clip set is playing.
    pub fn levels(&self) -> SharedLevels {
        self.levels.clone()
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.branches.shut_all();
        for handle in self.threads.drain(..).chain(self.cues.threads.drain(..)) {
            handle.thread().unpark();
            let _ = handle.join();
        }
        if let Some(plan) = self.reader.shared.show_plan.get() {
            plan.execution_lock.store(false, Ordering::Release);
        }
    }
}

/// Frees the clip sets the audio callback no longer holds. A set only this list references has
/// been swapped out already, and nothing else can clone it, so dropping it here, on the control
/// side, is the last release: the callback never frees one.
fn prune_clip_sets(sets: &mut Vec<Arc<ClipSet>>) {
    sets.retain(|set| Arc::strong_count(set) > 1);
}

struct AudioSpec {
    /// The audio API `device_hint` is looked up on; `None` is the system's default.
    driver: Option<String>,
    device_hint: String,
    needed: u16,
    /// 1-based interface channels of the programme.
    programme: Vec<u16>,
    clips: Arc<ClipSet>,
    /// The LTC outputs on this stream. Empty when LTC is off or plays on devices of its own.
    ltc: Vec<LtcSend>,
    /// The metronome, when it plays on this stream.
    click: Option<ClickSpec>,
    ltc_config: LtcConfig,
    rate: FrameRate,
    start: u64,
    clock: Clock,
}

struct AudioInfo {
    device: String,
    channels: u16,
}

#[derive(Clone)]
struct LtcSpec {
    driver: Option<String>,
    device_hint: String,
    /// The LTC outputs on this device. Empty when the stream plays only the metronome.
    ltc: Vec<LtcSend>,
    /// The metronome, when it plays on this device.
    click: Option<ClickSpec>,
    config: LtcConfig,
    rate: FrameRate,
    start: u64,
    clock: Clock,
}

/// Which stream the metronome plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClickPlace {
    /// The programme's stream.
    Programme,
    /// The stream of the LTC device at this position of the plan.
    Group(usize),
    /// A stream of its own, for a device that carries nothing else.
    Own,
}

/// The metronome and the 1-based channels it plays on, before the device says how many it has.
#[derive(Debug, Clone)]
struct ClickSpec {
    shared: Arc<SharedMetronome>,
    channels: Vec<u16>,
}

impl ClickSpec {
    /// The click as a stream with `opened` channels plays it: channels the device lacks are left out.
    fn out(&self, opened: u16) -> ClickOut {
        ClickOut {
            shared: Arc::clone(&self.shared),
            channels: self
                .channels
                .iter()
                .map(|&channel| usize::from(channel) - 1)
                .filter(|&channel| channel < usize::from(opened))
                .collect(),
        }
    }
}

/// Whether two device name hints open the same audio device. Different hints can: an empty one is
/// the system's default output, and names match loosely.
fn same_device(driver: Option<&str>, a: &str, b: &str) -> bool {
    let name = |hint: &str| {
        devices::open_audio(hint, driver).and_then(|device| devices::device_name(&device))
    };
    a == b || matches!((name(a), name(b)), (Some(a), Some(b)) if a == b)
}

/// One LTC signal on one interface channel, and the project outputs that asked for it.
#[derive(Debug, PartialEq)]
struct LtcSlot {
    /// 1-based.
    channel: u16,
    /// Linear peak level: the first output that asked for this slot sets it.
    level: f32,
    /// Latency offset in ms, set by the first output like the level.
    offset_ms: i32,
    /// Positions in the project's `output.ltc`. More than one when the same device and channel is
    /// listed twice, which plays once.
    outputs: Vec<usize>,
}

/// Where each LTC output of a project plays.
#[derive(Debug, PartialEq)]
struct LtcPlan {
    /// Channels written into the programme's own stream.
    shared: Vec<LtcSlot>,
    /// The other devices, each with the channels it carries.
    own: Vec<LtcGroup>,
}

#[derive(Debug, PartialEq)]
struct LtcGroup {
    /// The name hint of the first output that named the device.
    device: String,
    slots: Vec<LtcSlot>,
}

/// What each slot sends: its 1-based channel, level and offset.
fn sends_of(slots: &[LtcSlot]) -> Vec<LtcSend> {
    slots
        .iter()
        .map(|slot| LtcSend {
            channel: slot.channel,
            level: slot.level,
            offset_ms: slot.offset_ms,
        })
        .collect()
}

/// Splits the project's LTC outputs between the programme's stream and one stream per other
/// device, so no device is opened twice.
fn plan_ltc(
    outputs: &[LtcOutput],
    programme_device: &str,
    same: impl Fn(&str, &str) -> bool,
) -> LtcPlan {
    let mut plan = LtcPlan {
        shared: Vec::new(),
        own: Vec::new(),
    };
    for (index, output) in outputs.iter().enumerate() {
        if output.device_name == flux_project::UNASSIGNED_DEVICE {
            continue;
        }
        let slots = if same(&output.device_name, programme_device) {
            &mut plan.shared
        } else {
            let at = plan
                .own
                .iter()
                .position(|group| same(&group.device, &output.device_name));
            let at = at.unwrap_or_else(|| {
                plan.own.push(LtcGroup {
                    device: output.device_name.clone(),
                    slots: Vec::new(),
                });
                plan.own.len() - 1
            });
            &mut plan.own[at].slots
        };
        match slots.iter_mut().find(|slot| slot.channel == output.channel) {
            Some(slot) => slot.outputs.push(index),
            None => slots.push(LtcSlot {
                channel: output.channel,
                level: output.level.unwrap_or(DEFAULT_LTC_LEVEL),
                offset_ms: output.offset_ms.unwrap_or(0),
                outputs: vec![index],
            }),
        }
    }
    plan
}

/// How long a stream may go without calling back before its device counts as gone. Hosts that
/// never report an unplugged device just stop calling.
const STREAM_STALL: Duration = Duration::from_millis(1_000);
/// How long a stream just opened has to call back for the first time.
const STREAM_GRACE: Duration = Duration::from_millis(3_000);
/// How often a lost stream or MIDI port is tried again.
const RETRY_INTERVAL: Duration = Duration::from_millis(1_000);

/// Notices a stream that stopped calling back. Fed the callback count and the time.
#[derive(Debug)]
struct Watchdog {
    beats: u64,
    /// When the count last moved (or the stream opened).
    since: Duration,
    started: bool,
}

impl Watchdog {
    fn new(now: Duration) -> Self {
        Self {
            beats: 0,
            since: now,
            started: false,
        }
    }

    /// Whether the stream has been silent for too long as of `now`.
    fn stalled(&mut self, beats: u64, now: Duration) -> bool {
        if beats != self.beats {
            self.beats = beats;
            self.since = now;
            self.started = true;
        }
        let allowed = if self.started {
            STREAM_STALL
        } else {
            STREAM_GRACE
        };
        now.saturating_sub(self.since) > allowed
    }
}

/// What the thread that holds a stream should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Idle,
    /// The device is gone: raise the lost flag, and close the stream when it will be retried.
    Lose,
    /// Try to open the stream again.
    Reopen,
}

/// Decides when a stream is lost and, for a follower, when to try to get it back.
#[derive(Debug)]
struct Supervisor {
    watchdog: Watchdog,
    retry: bool,
    state: StreamState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamState {
    Open,
    /// Closed and waiting until this time to try again.
    Down(Duration),
    /// Lost, and not to be reopened.
    Gone,
}

impl Supervisor {
    fn new(now: Duration, retry: bool) -> Self {
        Self {
            watchdog: Watchdog::new(now),
            retry,
            state: StreamState::Open,
        }
    }

    fn step(&mut self, now: Duration, beats: u64, lost: bool) -> Step {
        match self.state {
            StreamState::Open if lost || self.watchdog.stalled(beats, now) => {
                self.state = if self.retry {
                    StreamState::Down(now + RETRY_INTERVAL)
                } else {
                    StreamState::Gone
                };
                Step::Lose
            }
            StreamState::Down(at) if now >= at => Step::Reopen,
            _ => Step::Idle,
        }
    }

    /// The stream opened again.
    fn reopened(&mut self, now: Duration) {
        self.watchdog = Watchdog::new(now);
        self.state = StreamState::Open;
    }

    /// Opening it again failed.
    fn reopen_failed(&mut self, now: Duration) {
        self.state = StreamState::Down(now + RETRY_INTERVAL);
    }
}

/// What a stream thread watches on the stream it holds.
struct Watch {
    /// Bumped by every callback.
    beat: Arc<AtomicU64>,
    /// Raised when the stream is lost; lowered when a retry opens it again.
    lost: Arc<AtomicBool>,
    /// Whether to open the stream again once it is lost.
    retry: bool,
}

/// Runs `open` on a thread of its own, which keeps the stream it returns for as long as the output
/// runs, and waits until the stream is open or has failed. A cpal stream is not `Send` on every
/// platform, so it is created, played and dropped on that one thread. The same thread notices a
/// stream that is lost without an error and, when `watch.retry`, opens it again when the device
/// comes back.
fn spawn_stream<I: Send + 'static>(
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
fn spawn_follower(
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
struct OpenedDevice {
    device: cpal::Device,
    name: String,
    channels: u16,
    format: SampleFormat,
}

/// Opens the device `hint` names with the fewest channels that cover `needed`, at `rate`.
fn open_device(
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

fn operator_channels(shared: &SharedClock, device: &str, driver: Option<&str>) -> Vec<usize> {
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

fn mark_audio_routes(
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

fn open_stream(
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
fn open_ltc_stream(
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
fn start_stream<F: Fill>(
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

fn build<T, F>(
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

/// Connects to the MIDI output port a project's name hint refers to, the `index`-th one of that
/// name. Returns the port's position in the system's list, its name and the connection. The error
/// is a message for the user, since a missing port is not fatal.
pub(crate) fn connect_midi(
    hint: &str,
    index: usize,
    client: &str,
) -> Result<(usize, String, midir::MidiOutputConnection), String> {
    let output =
        midir::MidiOutput::new(client).map_err(|err| format!("MIDI is unavailable: {err}"))?;
    let ports: Vec<(String, midir::MidiOutputPort)> = output
        .ports()
        .into_iter()
        .filter_map(|port| Some((output.port_name(&port).ok()?, port)))
        .collect();
    let (name, port) = devices::find_by_name(hint, &ports, index, |(name, _)| name)
        .ok_or_else(|| format!("MIDI port not found: {hint}"))?;
    let name = name.clone();
    let at = ports
        .iter()
        .position(|(_, other)| other == port)
        .unwrap_or(0);
    output
        .connect(port, client)
        .map(|connection| (at, name.clone(), connection))
        .map_err(|err| format!("cannot open {name}: {err}"))
}

/// How the MTC thread waits for the next quarter frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MtcWait {
    Park(Duration),
    /// Busy-wait this long: the quarter frame is closer than the OS can promise to wake us.
    Spin(Duration),
}

/// Parks until shortly before the quarter frame is due, then spins the rest, so the message goes
/// out when it falls due and not up to a timer tick later. `fresh` is false when the published
/// position is old enough that waiting for it to reach the deadline could never end.
pub(crate) fn mtc_wait(until_due: Duration, fresh: bool) -> MtcWait {
    if !fresh {
        MtcWait::Park(MTC_TICK)
    } else if until_due <= MTC_SPIN {
        MtcWait::Spin(until_due)
    } else {
        MtcWait::Park((until_due - MTC_SPIN).min(MTC_TICK))
    }
}

/// A MIDI output port as `run_mtc` uses it.
trait MidiLink {
    /// Sends one message. `false` means the port did not take it.
    fn send(&mut self, message: &[u8]) -> bool;
    /// Opens the port again after it was lost. `false` means it is still not there.
    fn reconnect(&mut self) -> bool;
}

/// The connection to a MIDI port and the name it was found by, to find it again after it is
/// unplugged.
struct PortLink {
    connection: midir::MidiOutputConnection,
    hint: String,
    index: usize,
}

impl MidiLink for PortLink {
    fn send(&mut self, message: &[u8]) -> bool {
        self.connection.send(message).is_ok()
    }

    fn reconnect(&mut self) -> bool {
        match connect_midi(&self.hint, self.index, MIDI_CLIENT_NAME) {
            Ok((_, _, connection)) => {
                self.connection = connection;
                true
            }
            Err(_) => false,
        }
    }
}

/// One MIDI port MTC goes to. A message the port refuses marks it lost; nothing is sent to it
/// until it is opened again, which is tried once a second.
struct MtcPort<L> {
    link: L,
    lost: Arc<AtomicBool>,
    shared: Arc<SharedClock>,
    /// When a lost port is tried again, in the clock's nanoseconds.
    retry_at: u64,
}

impl<L: MidiLink> MtcPort<L> {
    fn new(link: L, lost: Arc<AtomicBool>, shared: Arc<SharedClock>) -> Self {
        Self {
            link,
            lost,
            shared,
            retry_at: 0,
        }
    }

    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    fn send(&mut self, message: &[u8], now: u64) {
        if self.is_lost() {
            return;
        }
        if !self.link.send(message) {
            self.shared.mtc_failed();
            self.lost.store(true, Ordering::Relaxed);
            self.retry_at = now + RETRY_INTERVAL.as_nanos() as u64;
        }
    }

    /// Tries to open a lost port again when it is time. Returns whether it is back, in which case
    /// the next message must be a full frame.
    fn retry(&mut self, now: u64) -> bool {
        if !self.is_lost() || now < self.retry_at {
            return false;
        }
        if self.link.reconnect() {
            self.lost.store(false, Ordering::Relaxed);
            return true;
        }
        self.retry_at = now + RETRY_INTERVAL.as_nanos() as u64;
        false
    }
}

/// Sends MTC from the position the audio callback publishes: a full frame whenever playback
/// starts or jumps, and quarter frames as they fall due. A port that goes away is marked lost and
/// opened again when it comes back.
fn run_mtc(
    mut scheduler: MtcScheduler,
    clock: Clock,
    link: PortLink,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
) {
    let mut port = MtcPort::new(link, lost, Arc::clone(&shared));
    let mut seen_epoch = None;
    while !stop.load(Ordering::Relaxed) {
        if port.is_lost() {
            if port.retry(shared.now()) {
                seen_epoch = None;
            } else {
                thread::park_timeout(AUDIO_PARK);
                continue;
            }
        }
        let snapshot = shared.load();
        let now = shared.now();
        let dynamic = shared.show_timecode(&snapshot);
        let active_clock = dynamic.map_or(clock, |(clock, _)| clock);
        let position = snapshot.estimated_position(now, active_clock);
        if seen_epoch != Some(snapshot.epoch) {
            if let Some((clock, start)) = dynamic
                && let (Some(plan), Some(tick)) = (shared.show_plan.get(), snapshot.show)
            {
                scheduler.set_timecode(
                    clock,
                    plan.entries[tick.entry].timeline.timecode.frame_rate,
                    start,
                );
            }
            seen_epoch = Some(snapshot.epoch);
            port.send(&scheduler.resync(position), now);
        }
        let mut wait = MtcWait::Park(MTC_TICK);
        if snapshot.state == TransportState::Playing && !port.is_lost() {
            scheduler.poll(position, |message| port.send(&message, now));
            let samples = scheduler.next_due().saturating_sub(position);
            let until_due = Duration::from_nanos(
                (u128::from(samples) * 1_000_000_000 / u128::from(clock.sample_rate())) as u64,
            );
            let fresh =
                Duration::from_nanos(now.saturating_sub(snapshot.stamp)) < MAX_EXTRAPOLATION;
            wait = mtc_wait(until_due, fresh);
        }
        match wait {
            MtcWait::Park(time) => thread::park_timeout(time),
            MtcWait::Spin(time) => {
                let deadline = shared.now() + time.as_nanos() as u64;
                while shared.now() < deadline && !stop.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ltc::{LtcConfig, LtcStream};

    const RATE: FrameRate = FrameRate::Fps25;

    fn clock() -> Clock {
        Clock::new(48_000, RATE)
    }

    fn config() -> LtcConfig {
        LtcConfig {
            sample_rate: 48_000,
            level: 0.5,
            edge_samples: 0,
        }
    }

    fn tap(channel: usize, gain: f32) -> LtcTap {
        LtcTap { channel, gain }
    }

    /// A renderer for `channels` interface channels with LTC on 0-based channel `ltc_channel`.
    fn renderer(
        channels: usize,
        ltc_channel: usize,
    ) -> (Renderer, Sender<Message>, Arc<SharedClock>) {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(SharedClock::new());
        let ltc = LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0),
            channels: vec![tap(ltc_channel, 1.0)],
        };
        let renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
            Arc::new(ClipSet::default()),
            Vec::new(),
            Some(ltc),
            channels,
            Arc::clone(&shared),
        );
        (renderer, tx, shared)
    }

    fn channel_of(buffer: &[f32], channels: usize, channel: usize) -> Vec<f32> {
        buffer
            .chunks_exact(channels)
            .map(|frame| frame[channel])
            .collect()
    }

    fn spoken_clips() -> Arc<ClipSet> {
        spoken_clips_with(vec![16_384; 96_000])
    }

    fn routed_config() -> OutputConfig {
        use flux_project::routing::{ResolvedAudioBus, TrackRoute};
        let mut config = OutputConfig::default();
        for (id, device, channels) in [
            ("one", "Main", vec![1, 2]),
            ("two", "Main", vec![1, 2]),
            ("three", "Main", vec![1, 2]),
        ] {
            config.audio_routes.push(ResolvedAudioBus {
                id: id.into(),
                route: Some(flux_project::AudioOutput {
                    device_name: device.into(),
                    channels,
                }),
            });
            config.track_routes.push(TrackRoute {
                track_id: id.into(),
                output: Some(id.into()),
            });
        }
        config
    }
    fn routed_clips() -> Arc<ClipSet> {
        use crate::mixer::ClipPlayer;
        let clips = [24_576, 24_576, -24_576, 24_576]
            .into_iter()
            .enumerate()
            .map(|(track, sample)| {
                ClipPlayer::new(
                    Arc::new(flux_audio::Decoded {
                        samples: vec![sample; 4096],
                        source_rate: ENGINE_RATE,
                    }),
                    0,
                    1.0,
                    track,
                )
            })
            .collect();
        Arc::new(ClipSet::new(
            clips,
            ["one", "two", "three", "unassigned"]
                .map(|id| (id.into(), 1.0))
                .into(),
        ))
    }
    #[test]
    fn named_routes_sum_once_before_clamping_and_do_not_play_new_unassigned_tracks() {
        let config = routed_config();
        let (mut renderer, tx, _) = renderer(3, 2);
        renderer.clips = routed_clips();
        renderer.audio_lanes = crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 24];
        renderer.render(&mut buffer, 0);
        assert_eq!(channel_of(&buffer, 3, 0), vec![0.75; 8]);
        assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
        assert!(channel_of(&buffer, 3, 2).iter().all(|s| s.abs() > 0.0));
    }
    #[test]
    fn named_routes_keep_mono_channels_separate_and_faders_work() {
        let mut config = routed_config();
        config.audio_routes[0].route.as_mut().unwrap().channels = vec![1];
        config.audio_routes[1].route.as_mut().unwrap().channels = vec![2];
        config.audio_routes[2].route = None;
        let clips = routed_clips();
        clips.levels().set_gain_db("one", -60.0);
        let (mut renderer, tx, _) = renderer(3, 2);
        renderer.clips = clips;
        renderer.audio_lanes = crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 24];
        renderer.render(&mut buffer, 0);
        assert_eq!(buffer[21], 0.0);
        assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
        assert!(
            renderer
                .clips
                .levels()
                .take_peaks()
                .iter()
                .any(|p| p.id == "two" && p.peak == 0.75)
        );
    }
    #[test]
    fn ltc_collision_silences_the_whole_bus_and_missing_routes_never_fall_back() {
        let mut config = routed_config();
        config.audio_routes[1].route.as_mut().unwrap().device_name = "Gone".into();
        config.audio_routes[2].route = None;
        config.ltc.push(LtcOutput {
            device_name: "Main".into(),
            channel: 2,
            level: None,
            offset_ms: None,
        });
        assert!(
            crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn named_output_devices_share_ltc_click_and_aliases_in_one_stream() {
        let mut config = routed_config();
        config.audio_routes[0].route.as_mut().unwrap().device_name = "Secondary".into();
        config.audio_routes[1].route.as_mut().unwrap().device_name = "Alias".into();
        config.audio = Some(flux_project::AudioOutput {
            device_name: "Main".into(),
            channels: vec![1, 2],
        });
        config.ltc.push(LtcOutput {
            device_name: "Secondary".into(),
            channel: 8,
            level: None,
            offset_ms: None,
        });
        let timeline = flux_project::Project::new("Show", RATE).timelines.remove(0);
        let plan = Plan::build(&config, &timeline, None, |a, b| {
            a == b || (["Secondary", "Alias"].contains(&a) && ["Secondary", "Alias"].contains(&b))
        });
        assert_eq!(plan.followers.len(), 1);
        assert_eq!(plan.followers[0].group.slots.len(), 1);
        assert_eq!(plan.master.device_hint, "Main");
    }
    #[test]
    fn audio_route_health_reports_loss_without_a_bitmask_limit() {
        use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
        let flag = Arc::new(AtomicBool::new(false));
        let states: Vec<_> = (0..48)
            .map(|id| RouteHealth {
                state: AudioRouteState {
                    id: id.to_string(),
                    device: "D".into(),
                    channels: vec![1],
                    status: AudioRouteStatus::Ready,
                },
                lost: Some(Arc::clone(&flag)),
            })
            .collect();
        flag.store(true, Ordering::Relaxed);
        assert!(
            states
                .iter()
                .all(|s| s.state().status == AudioRouteStatus::Lost)
        );
    }

    #[test]
    fn clip_edits_keep_active_routes_until_stationary_reconfiguration() {
        let mut active = routed_config();
        active.audio_routes.truncate(1);
        active.track_routes.truncate(1);
        active.audio_routes[0].route.as_mut().unwrap().channels = vec![1];
        let mut desired = active.clone();
        desired.audio_routes[0].route.as_mut().unwrap().channels = vec![2];
        let (mut renderer, tx, _) = renderer(3, 2);
        renderer.clips = routed_clips();
        renderer.audio_lanes = crate::audio_routing::lanes(&active, "Main", 3, |a, b| a == b);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 24];
        renderer.render(&mut buffer, 0);
        tx.send(Message::Clips(routed_clips())).unwrap();
        renderer.render(&mut buffer, 0);
        assert_eq!(channel_of(&buffer, 3, 0), vec![0.75; 8]);
        assert_eq!(channel_of(&buffer, 3, 1), vec![0.0; 8]);
        tx.send(Message::Transport(Command::Pause)).unwrap();
        renderer.render(&mut buffer, 0);
        let parked = renderer.transport.position_samples();
        assert!(buffer.iter().all(|s| *s == 0.0));
        renderer.audio_lanes = crate::audio_routing::lanes(&desired, "Main", 3, |a, b| a == b);
        renderer.transport.apply(Command::Restore {
            samples: parked,
            paused: true,
        });
        renderer.render(&mut buffer, 0);
        assert_eq!(renderer.transport.state(), TransportState::Paused);
        assert_eq!(renderer.transport.position_samples(), parked);
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut buffer, 0);
        assert_eq!(channel_of(&buffer, 3, 0), vec![0.0; 8]);
        assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
    }

    #[test]
    fn unassigned_bus_health_is_not_mistaken_for_the_default_device() {
        use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
        let shared = SharedClock::new();
        shared.audio_health.write().unwrap().push(RouteHealth {
            state: AudioRouteState {
                id: "operator".into(),
                device: String::new(),
                channels: vec![],
                status: AudioRouteStatus::Missing,
            },
            lost: None,
        });
        mark_audio_routes(&shared, "", None, 2, &shared.lost);
        assert_eq!(
            shared.audio_health.read().unwrap()[0].state().status,
            AudioRouteStatus::Missing
        );
        // A reopened device must re-evaluate conflicts, not promote every available bus to Ready.
        let mut shared = SharedClock::new();
        shared.audio_config = routed_config();
        shared.audio_config.ltc.push(LtcOutput {
            device_name: "Main".into(),
            channel: 2,
            level: None,
            offset_ms: None,
        });
        shared.audio_health.write().unwrap().push(RouteHealth {
            state: AudioRouteState {
                id: "one".into(),
                device: "Main".into(),
                channels: vec![1, 2],
                status: AudioRouteStatus::Missing,
            },
            lost: None,
        });
        mark_audio_routes(&shared, "Main", None, 2, &shared.lost);
        assert_eq!(
            shared.audio_health.read().unwrap()[0].state().status,
            AudioRouteStatus::Conflict
        );
    }

    fn spoken_clips_with(samples: Vec<i16>) -> Arc<ClipSet> {
        use flux_project::{Alignment, AudioAsset, GeneratedVoice, Track, TrackKind, VoiceOrder};
        let mut project = flux_project::Project::new("Show", FrameRate::Fps25);
        project.assets.push(AudioAsset {
            id: "a".into(),
            name: "Voice".into(),
            file: "audio/voice.mp3".into(),
            bpm: None,
        });
        project.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "voice".into(),
            name: "Operator".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![VoiceOrder {
                    mode: Default::default(),
                    synthesis_text: None,
                    trim_start: 0,
                    trim_end: 0,
                    id: "o".into(),
                    at: 0,
                    dialogue: "Go".into(),
                    alignment: Alignment::Start,
                    generated: Some(GeneratedVoice {
                        origin: flux_project::operator::RecordingOrigin::Generated,
                        synthesis_text: None,
                        asset: "a".into(),
                        dialogue: "Go".into(),
                        voice: "v".into(),
                        model: flux_project::operator::VOICE_MODEL.into(),
                        samples: 48_000,
                    }),
                }],
            },
        });
        Arc::new(
            super::super::mixer::build(&project, &project.timelines[0], |_| {
                Ok(Arc::new(flux_audio::Decoded {
                    samples: samples.clone(),
                    source_rate: ENGINE_RATE,
                }))
            })
            .set,
        )
    }

    #[test]
    fn operator_renders_only_on_its_channels_and_obeys_transport() {
        let shared = Arc::new(SharedClock::new());
        let (tx, rx) = mpsc::channel();
        let clock = Clock::new(ENGINE_RATE, FrameRate::Fps25);
        let mut renderer = Renderer::new(
            Transport::new(clock, 0),
            rx,
            spoken_clips(),
            vec![0, 1],
            vec![],
            3,
            shared,
        );
        renderer.operator_channels = vec![2];
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 6];
        renderer.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
        tx.send(Message::Transport(Command::Pause)).unwrap();
        renderer.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0; 6]);
        tx.send(Message::Transport(Command::Seek(1))).unwrap();
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn operator_follower_tracks_seek_and_silences_when_master_is_lost() {
        let shared = Arc::new(SharedClock::new());
        shared.update_operator(spoken_clips());
        let clock = Clock::new(ENGINE_RATE, FrameRate::Fps25);
        let mut follower = LtcFollower::new(vec![], 3, clock, Arc::clone(&shared));
        follower.operator_channels = vec![2];
        shared.publish(Snapshot {
            show: None,
            state: TransportState::Playing,
            epoch: 1,
            position: 100,
            stamp: 0,
        });
        let mut buffer = [0.0f32; 6];
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
        shared.publish(Snapshot {
            show: None,
            state: TransportState::Playing,
            epoch: 2,
            position: 0,
            stamp: 0,
        });
        follower.render(&mut buffer, 0);
        assert_eq!(follower.voice_cursor.next, Some(2.0));
        shared.lost.store(true, Ordering::Relaxed);
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0; 6]);
    }

    #[test]
    fn operator_callback_keeps_playing_while_control_plane_is_locked() {
        let shared = Arc::new(SharedClock::new());
        shared.update_operator(spoken_clips());
        let mut follower = LtcFollower::new(vec![], 1, clock(), Arc::clone(&shared));
        follower.operator_channels = vec![0];
        shared.publish(Snapshot {
            show: None,
            state: TransportState::Playing,
            epoch: 1,
            position: 0,
            stamp: 0,
        });
        let locked = shared.operator_set.lock().unwrap();
        let mut buffer = [0.0f32; 256];
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, [0.5; 256]);
        drop(locked);
        shared.update_operator(Arc::new(ClipSet::default()));
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, [0.0; 256]);
    }

    #[test]
    fn follower_corrects_device_drift_without_jumping_spoken_samples() {
        let mut cursor = VoiceCursor::default();
        let clips = ClipSet::default();
        let mut left = [0.0; MIX_CHUNK];
        let mut right = [0.0; MIX_CHUNK];
        let mut master = 0.0;
        // One hour at 100 ppm clock disagreement, in fixed callback blocks.
        for _ in 0..675_000 {
            let before = cursor.next;
            cursor.follow(master as u64, false);
            if let Some(before) = before {
                assert_eq!(cursor.next, Some(before));
            }
            assert!((cursor.next.unwrap() - master).abs() < 12.0);
            cursor.mix(&clips, &mut left, &mut right);
            master += MIX_CHUNK as f64 * 1.0001;
        }
        cursor.follow(0, true);
        assert_eq!(cursor.next, Some(0.0), "a seek resets immediately");
        assert_eq!(cursor.step, 1.0);
    }

    #[test]
    fn follower_interpolates_stereo_continuously_across_callback_boundaries() {
        let clips = spoken_clips_with(
            (0..48_000)
                .flat_map(|i| {
                    let v = (i % 1000) as i16;
                    [v, -v]
                })
                .collect(),
        );
        let mut cursor = VoiceCursor {
            next: Some(3.25),
            step: 1.001,
        };
        let mut left = [0.0; MIX_CHUNK];
        let mut right = [0.0; MIX_CHUNK];
        for block in 0..2 {
            cursor.mix(&clips, &mut left, &mut right);
            for i in 0..MIX_CHUNK {
                let expected = (3.25 + (block * MIX_CHUNK + i) as f64 * 1.001) as f32 / 32768.0;
                assert!((left[i] - expected).abs() < 1e-7);
                assert!((right[i] + expected).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn consuming_play_does_not_clear_a_later_queued_play() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        shared.pending_plays.store(2, Ordering::Release);
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut [0.0f32; 1], 0);
        assert_eq!(shared.pending_plays.load(Ordering::Acquire), 1);
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut [0.0f32; 1], 0);
        assert_eq!(shared.pending_plays.load(Ordering::Acquire), 0);
    }

    #[test]
    fn operator_route_uses_one_stream_per_device_and_keeps_master_requirements() {
        let project = flux_project::Project::new("Show", FrameRate::Fps25);
        let mut output = OutputConfig {
            audio_routes: Vec::new(),
            track_routes: Vec::new(),
            audio: Some(flux_project::AudioOutput {
                device_name: "Main".into(),
                channels: vec![1, 2],
            }),
            operator: Some(flux_project::AudioOutput {
                device_name: "Main".into(),
                channels: vec![5],
            }),
            ..Default::default()
        };
        let plan = Plan::build(&output, &project.timelines[0], None, |a, b| a == b);
        assert_eq!(plan.master.needed, 5);
        assert!(plan.followers.is_empty());
        output.operator.as_mut().unwrap().device_name = "Booth".into();
        output.metronome = Some(flux_project::MetronomeOutput {
            device_name: "Booth".into(),
            channels: vec![2],
            gain_db: 0.0,
        });
        let plan = Plan::build(&output, &project.timelines[0], None, |a, b| a == b);
        assert_eq!(plan.master.needed, 2);
        assert_eq!(plan.followers.len(), 1);
        assert!(plan.followers[0].click.is_some());
        assert!(plan.own_click.is_none());
    }

    #[test]
    fn unassigned_ltc_should_not_open_a_device_or_shift_later_output_indices() {
        let outputs = [
            LtcOutput {
                device_name: flux_project::UNASSIGNED_DEVICE.into(),
                channel: 1,
                level: None,
                offset_ms: None,
            },
            LtcOutput {
                device_name: "Sync".into(),
                channel: 8,
                level: None,
                offset_ms: None,
            },
        ];
        let plan = plan_ltc(&outputs, "Main", |a, b| a == b);
        assert!(plan.shared.is_empty());
        assert_eq!(plan.own.len(), 1);
        assert_eq!(plan.own[0].slots[0].outputs, vec![1]);
    }

    #[test]
    fn render_should_be_silent_until_played() {
        let (mut renderer, _tx, shared) = renderer(2, 1);
        let mut buffer = [1.0f32; 256];
        renderer.render(&mut buffer, 10);
        assert!(buffer.iter().all(|&s| s == 0.0));
        let snapshot = shared.load();
        assert_eq!(snapshot.state, TransportState::Stopped);
        assert_eq!(snapshot.position, 0);
    }

    #[test]
    fn render_should_put_ltc_on_its_channel_only() {
        let (mut renderer, tx, _shared) = renderer(4, 2);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 4 * 4_000];
        renderer.render(&mut buffer, 10);

        assert!(channel_of(&buffer, 4, 2).iter().any(|&s| s != 0.0));
        for other in [0, 1, 3] {
            assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn render_should_advance_the_position_by_the_frames_rendered() {
        let (mut renderer, tx, shared) = renderer(2, 0);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 480];
        renderer.render(&mut buffer, 10);
        renderer.render(&mut buffer, 20);
        let snapshot = shared.load();
        assert_eq!(snapshot.position, 960);
        assert_eq!(snapshot.state, TransportState::Playing);
        assert_eq!(snapshot.stamp, 20);
    }

    #[test]
    fn render_should_go_silent_and_hold_the_position_when_paused() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 4_000];
        renderer.render(&mut buffer, 10);
        tx.send(Message::Transport(Command::Pause)).unwrap();
        renderer.render(&mut buffer, 20);
        assert!(buffer.iter().all(|&s| s == 0.0));
        assert_eq!(shared.load().position, 4_000);
        assert_eq!(shared.load().state, TransportState::Paused);
    }

    #[test]
    fn render_should_resync_ltc_after_a_seek() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 1_000];
        renderer.render(&mut buffer, 10);
        let epoch = shared.load().epoch;

        // Frame 10 starts at sample 19 200: playing from there must begin LTC at once.
        tx.send(Message::Transport(Command::Seek(10))).unwrap();
        renderer.render(&mut buffer, 20);
        let snapshot = shared.load();
        assert_ne!(snapshot.epoch, epoch);
        assert_eq!(snapshot.position, 19_200 + 1_000);
        assert_ne!(buffer[0], 0.0, "LTC should start on the frame boundary");
    }

    #[test]
    fn render_should_convert_to_the_device_sample_format() {
        let (mut renderer, tx, _shared) = renderer(1, 0);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0i16; 4_000];
        renderer.render(&mut buffer, 10);
        let peak = buffer.iter().map(|s| s.unsigned_abs()).max().unwrap();
        // Level 0.5 of full scale.
        assert!((16_000..=16_500).contains(&peak), "peak {peak}");
    }

    #[test]
    fn render_should_drain_every_pending_command() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        tx.send(Message::Transport(Command::Play)).unwrap();
        tx.send(Message::Transport(Command::Seek(10))).unwrap();
        tx.send(Message::Transport(Command::Pause)).unwrap();
        let mut buffer = [0.0f32; 100];
        renderer.render(&mut buffer, 10);
        assert_eq!(shared.load().state, TransportState::Paused);
        assert_eq!(shared.load().position, 19_200);
    }

    #[test]
    fn estimated_position_should_follow_the_wall_clock_while_playing() {
        let snapshot = Snapshot {
            show: None,
            state: TransportState::Playing,
            epoch: 1,
            position: 48_000,
            stamp: 1_000_000_000,
        };
        // 10 ms later is 480 samples at 48 kHz.
        assert_eq!(snapshot.estimated_position(1_010_000_000, clock()), 48_480);
        assert_eq!(snapshot.estimated_position(1_000_000_000, clock()), 48_000);
        // A clock reading from before the stamp never goes backwards.
        assert_eq!(snapshot.estimated_position(500, clock()), 48_000);
    }

    #[test]
    fn estimated_position_should_not_extrapolate_past_a_stalled_stream() {
        let snapshot = Snapshot {
            show: None,
            state: TransportState::Playing,
            epoch: 1,
            position: 0,
            stamp: 0,
        };
        // After 10 seconds without an update it only trusts the last 100 ms: 4 800 samples.
        assert_eq!(snapshot.estimated_position(10_000_000_000, clock()), 4_800);
    }

    #[test]
    fn estimated_position_should_stand_still_when_not_playing() {
        for state in [TransportState::Stopped, TransportState::Paused] {
            let snapshot = Snapshot {
                show: None,
                state,
                epoch: 1,
                position: 777,
                stamp: 0,
            };
            assert_eq!(snapshot.estimated_position(5_000_000_000, clock()), 777);
        }
    }

    fn range(channels: u16, format: SampleFormat) -> ConfigRange {
        ConfigRange {
            channels,
            min_rate: 44_100,
            max_rate: 96_000,
            format,
        }
    }

    #[test]
    fn pick_config_should_choose_the_fewest_channels_that_cover_the_need() {
        let ranges = [
            range(8, SampleFormat::F32),
            range(2, SampleFormat::F32),
            range(4, SampleFormat::F32),
        ];
        assert_eq!(
            pick_config(&ranges, 3, 48_000),
            Some((4, SampleFormat::F32))
        );
        assert_eq!(
            pick_config(&ranges, 1, 48_000),
            Some((2, SampleFormat::F32))
        );
    }

    #[test]
    fn pick_config_should_prefer_float_then_wider_integers() {
        let ranges = [
            range(2, SampleFormat::I16),
            range(2, SampleFormat::I32),
            range(2, SampleFormat::F32),
        ];
        assert_eq!(
            pick_config(&ranges, 2, 48_000),
            Some((2, SampleFormat::F32))
        );
        assert_eq!(
            pick_config(&ranges[..2], 2, 48_000),
            Some((2, SampleFormat::I32))
        );
    }

    #[test]
    fn pick_config_should_skip_formats_it_cannot_write() {
        let ranges = [range(2, SampleFormat::U8), range(2, SampleFormat::F64)];
        assert_eq!(pick_config(&ranges, 2, 48_000), None);
    }

    #[test]
    fn pick_config_should_require_the_sample_rate() {
        let mut only_44k = range(2, SampleFormat::F32);
        only_44k.max_rate = 44_100;
        assert_eq!(pick_config(&[only_44k], 2, 48_000), None);
        assert_eq!(pick_config(&[], 1, 48_000), None);
    }

    #[test]
    fn pick_config_should_fail_when_the_device_has_too_few_channels() {
        assert_eq!(pick_config(&[range(2, SampleFormat::F32)], 8, 48_000), None);
    }

    use crate::mixer::ClipPlayer;
    use flux_audio::Decoded;

    /// A clip of constant stereo samples, as 16-bit values, that starts at the timeline origin.
    fn clip(left: i16, right: i16, frames: usize) -> ClipSet {
        ClipSet::new(
            vec![ClipPlayer::new(
                Arc::new(Decoded {
                    samples: (0..frames).flat_map(|_| [left, right]).collect(),
                    source_rate: ENGINE_RATE,
                }),
                0,
                1.0,
                0,
            )],
            vec![("t1".into(), 1.0)],
        )
    }

    /// A renderer playing `clips` on the given 0-based channels, with LTC on `ltc` if any.
    fn programme_renderer(
        channels: usize,
        programme: Vec<usize>,
        ltc: Option<usize>,
        clips: ClipSet,
    ) -> (Renderer, Sender<Message>, Arc<SharedClock>) {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(SharedClock::new());
        let ltc = ltc.map(|channel| LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0),
            channels: vec![tap(channel, 1.0)],
        });
        let renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
            Arc::new(clips),
            programme,
            ltc,
            channels,
            Arc::clone(&shared),
        );
        (renderer, tx, shared)
    }

    fn show_entry(
        id: &str,
        level: i16,
        end: u64,
        transition: flux_project::Transition,
    ) -> crate::show::PreparedEntry {
        let mut timeline = Timeline::new(id, FrameRate::Fps25);
        timeline.id = id.into();
        let project = flux_project::Project::new("Test", FrameRate::Fps25);
        crate::show::PreparedEntry {
            cues: crate::cues::CueList::build(&project, &timeline, |_| None),
            timeline,
            entry: flux_project::SetlistEntry {
                id: id.into(),
                timeline_id: id.into(),
                name: String::new(),
                notes: String::new(),
                omitted: false,
                transition,
            },
            clips: Arc::new(clip(level, level, 10_000)),
            end,
        }
    }

    fn show_plan(entries: Vec<crate::show::PreparedEntry>) -> Arc<crate::show::ShowPlan> {
        Arc::new(crate::show::ShowPlan {
            overview: flux_project::show::SetlistOverview {
                id: "s".into(),
                name: "Show".into(),
                entries: vec![],
            },
            mix: Default::default(),
            execution_lock: Arc::new(AtomicBool::new(false)),
            setlist_id: "test".into(),
            entries,
        })
    }

    #[test]
    fn show_switches_timelines_and_frame_rates_inside_a_buffer_without_a_gap() {
        let (mut renderer, tx, shared) =
            programme_renderer(2, vec![0, 1], None, ClipSet::default());
        let first = show_entry("one", 8192, 101, flux_project::Transition::Continue);
        let mut next = show_entry("two", 16384, 5000, flux_project::Transition::Wait);
        next.timeline.timecode.frame_rate = FrameRate::Fps30;
        next.timeline.timecode.start = 108_000;
        shared.show_plan.set(show_plan(vec![first, next])).unwrap();
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        let mut buffer = [0.0f32; 600];
        renderer.render(&mut buffer, 0);
        let left = channel_of(&buffer, 2, 0);
        assert_eq!(&left[..101], &[0.25; 101]);
        assert_eq!(&left[101..], &[0.5; 199]);
        assert_eq!(shared.load().show.unwrap().entry, 1);
        assert_eq!(renderer.transport.position_samples(), 199);
        assert_eq!(renderer.transport.start(), 108_000);
        assert_eq!(
            renderer.transport.clock(),
            Clock::new(48_000, FrameRate::Fps30)
        );
    }

    #[test]
    fn show_journal_keeps_each_loop_interval_and_devamp_finishes_the_turn() {
        let (mut renderer, tx, shared) =
            programme_renderer(2, vec![0, 1], None, ClipSet::default());
        let mut entry = show_entry("vamp", 8192, 9000, flux_project::Transition::Wait);
        entry.timeline.controls.push(flux_project::TimelineControl {
            id: "loop".into(),
            name: "Vamp".into(),
            at: 0,
            kind: flux_project::ControlKind::Loop {
                end: 1,
                turns: None,
            },
        });
        shared.show_plan.set(show_plan(vec![entry])).unwrap();
        let (journal, intervals) = mpsc::sync_channel(32);
        shared.show_spans.set(journal).unwrap();
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        renderer.render(&mut vec![0.0f32; 4000 * 2], 0);
        let spans: Vec<_> = intervals.try_iter().collect();
        assert_eq!(
            spans
                .iter()
                .map(|s| (s.from, s.to, s.turn))
                .collect::<Vec<_>>(),
            vec![(0, 1920, 1), (0, 1920, 2), (0, 160, 3)]
        );
        tx.send(Message::Show(crate::show::ShowCommand::Devamp))
            .unwrap();
        renderer.render(&mut vec![0.0f32; 2000 * 2], 0);
        assert_eq!(renderer.transport.position_samples(), 2160);
        assert!(!shared.load().show.unwrap().devamp_pending);
    }

    #[test]
    fn show_waits_at_an_exact_sample_and_duplicate_go_does_not_skip_entries() {
        let (mut renderer, tx, shared) =
            programme_renderer(2, vec![0, 1], None, ClipSet::default());
        let mut entry = show_entry("wait", 8192, 9000, flux_project::Transition::Wait);
        entry.timeline.controls.push(flux_project::TimelineControl {
            id: "wait".into(),
            name: "Hold".into(),
            at: 1,
            kind: flux_project::ControlKind::Wait,
        });
        shared.show_plan.set(show_plan(vec![entry])).unwrap();
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        let mut buffer = vec![0.0f32; 2000 * 2];
        renderer.render(&mut buffer, 0);
        assert!(buffer[..1920 * 2].iter().all(|s| *s == 0.25));
        assert!(buffer[1920 * 2..].iter().all(|s| *s == 0.0));
        assert_eq!(renderer.transport.position_samples(), 1920);
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        renderer.render(&mut buffer[..200], 0);
        assert_eq!(renderer.transport.position_samples(), 2020);
    }

    #[test]
    fn show_journal_includes_terminal_events_once_without_adding_silence() {
        let (mut renderer, tx, shared) =
            programme_renderer(2, vec![0, 1], None, ClipSet::default());
        let first = show_entry("one", 8192, 100, flux_project::Transition::Continue);
        let second = show_entry("two", 16384, 100, flux_project::Transition::Wait);
        shared
            .show_plan
            .set(show_plan(vec![first, second]))
            .unwrap();
        let (journal, intervals) = mpsc::sync_channel(32);
        shared.show_spans.set(journal).unwrap();
        tx.send(Message::Show(crate::show::ShowCommand::Go))
            .unwrap();
        let mut buffer = [0.0f32; 600];
        renderer.render(&mut buffer, 0);
        let spans: Vec<_> = intervals.try_iter().collect();
        assert_eq!(
            spans
                .iter()
                .filter(|s| s.include_end)
                .map(|s| (s.entry, s.to))
                .collect::<Vec<_>>(),
            vec![(0, 100), (1, 100)]
        );
        assert!(spans.iter().filter(|s| s.release).all(|s| !s.include_end));
        assert_eq!(
            &channel_of(&buffer, 2, 0)[..200],
            &[vec![0.25; 100], vec![0.5; 100]].concat()
        );
    }

    #[test]
    fn render_should_play_the_programme_on_its_channels_beside_ltc() {
        let (mut renderer, tx, _) =
            programme_renderer(4, vec![0, 1], Some(3), clip(16_384, -16_384, 1_000));
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 4 * 400];

        renderer.render(&mut buffer, 10);

        assert!(channel_of(&buffer, 4, 0).iter().all(|&s| s == 0.5));
        assert!(channel_of(&buffer, 4, 1).iter().all(|&s| s == -0.5));
        assert!(channel_of(&buffer, 4, 2).iter().all(|&s| s == 0.0));
        assert!(channel_of(&buffer, 4, 3).iter().any(|&s| s != 0.0));
    }

    #[test]
    fn render_should_play_music_with_no_ltc_at_all() {
        let (mut renderer, tx, _) =
            programme_renderer(2, vec![0, 1], None, clip(8_192, 8_192, 100));
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 100];

        renderer.render(&mut buffer, 10);

        assert!(buffer.iter().all(|&s| s == 0.25));
    }

    #[test]
    fn render_should_send_a_single_programme_channel_the_mono_sum() {
        let (mut renderer, tx, _) = programme_renderer(3, vec![2], None, clip(16_384, 0, 10));
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 3 * 10];

        renderer.render(&mut buffer, 10);

        assert!(channel_of(&buffer, 3, 2).iter().all(|&s| s == 0.25));
        assert!(channel_of(&buffer, 3, 0).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn render_should_not_play_the_programme_while_paused_or_stopped() {
        let (mut renderer, tx, _) =
            programme_renderer(2, vec![0, 1], None, clip(16_384, 16_384, 100));
        let mut buffer = [1.0f32; 2 * 100];
        renderer.render(&mut buffer, 10);
        assert!(buffer.iter().all(|&s| s == 0.0));

        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut buffer, 20);
        tx.send(Message::Transport(Command::Pause)).unwrap();
        renderer.render(&mut buffer, 30);
        assert!(buffer.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn render_should_clamp_a_programme_that_is_too_loud() {
        let loud = ClipSet::new(
            vec![
                ClipPlayer::new(
                    Arc::new(Decoded {
                        samples: vec![i16::MAX; 20],
                        source_rate: ENGINE_RATE,
                    }),
                    0,
                    1.0,
                    0,
                );
                3
            ],
            vec![("t1".into(), 1.0)],
        );
        let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, loud);
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 10];

        renderer.render(&mut buffer, 10);

        assert!(buffer.iter().all(|&s| s == 1.0));
    }

    #[test]
    fn render_should_switch_to_new_clips_sent_while_playing() {
        let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 10];
        renderer.render(&mut buffer, 10);
        assert!(buffer.iter().all(|&s| s == 0.0));

        // The transport is at sample 10 now; the clip starts at 0 and is long enough to cover it.
        tx.send(Message::Clips(Arc::new(clip(16_384, 16_384, 100))))
            .unwrap();
        renderer.render(&mut buffer, 20);

        assert!(buffer.iter().all(|&s| s == 0.5));
    }

    #[test]
    fn render_should_stop_the_transport_at_the_end_of_the_show() {
        let (mut renderer, tx, shared) = programme_renderer(
            1,
            vec![0],
            Some(0),
            clip(8_192, 8_192, 100).with_end(Some(100)),
        );
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 60];
        renderer.render(&mut buffer, 10);
        assert_eq!(shared.load().state, TransportState::Playing);

        renderer.render(&mut buffer, 20);
        let snapshot = shared.load();
        assert_eq!(snapshot.state, TransportState::Stopped);
        assert_eq!(snapshot.position, 0);

        // Stopped for good: nothing sounds and the position stays at the start.
        renderer.render(&mut buffer, 30);
        assert!(buffer.iter().all(|&s| s == 0.0));
        assert_eq!(shared.load().position, 0);
    }

    #[test]
    fn render_should_follow_an_end_sent_while_playing() {
        let (mut renderer, tx, shared) =
            programme_renderer(1, vec![0], None, ClipSet::default().with_end(Some(100)));
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 60];
        renderer.render(&mut buffer, 10);

        // The clips were edited: the show now ends later, so the first end passes unnoticed.
        tx.send(Message::Clips(Arc::new(
            ClipSet::default().with_end(Some(1_000)),
        )))
        .unwrap();
        renderer.render(&mut buffer, 20);
        assert_eq!(shared.load().state, TransportState::Playing);
        assert_eq!(shared.load().position, 120);
    }

    #[test]
    fn render_should_not_stop_a_show_that_has_no_end() {
        let (mut renderer, tx, shared) =
            programme_renderer(1, vec![0], Some(0), ClipSet::default());
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 60];
        for tick in 0..100 {
            renderer.render(&mut buffer, tick);
        }
        assert_eq!(shared.load().state, TransportState::Playing);
    }

    #[test]
    fn render_should_mix_a_buffer_longer_than_one_chunk() {
        let (mut renderer, tx, _) =
            programme_renderer(2, vec![0, 1], None, clip(8_192, 8_192, 1_000));
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 700];

        renderer.render(&mut buffer, 10);

        assert!(buffer.iter().all(|&s| s == 0.25));
    }

    #[test]
    fn pace_watch_should_stay_quiet_for_a_device_that_keeps_time() {
        let mut watch = PaceWatch::default();
        let mut verdict = None;
        // 480 frames every 10 ms is exactly real time at 48 kHz.
        for tick in 0..=100u64 {
            verdict = watch.observe(tick * 10_000_000, 480).or(verdict);
        }
        assert_eq!(verdict, Some(false));
    }

    #[test]
    fn pace_watch_should_flag_a_device_that_runs_far_faster_than_real_time() {
        let mut watch = PaceWatch::default();
        let mut verdict = None;
        // 300 times too fast, as ALSA's null device does.
        for tick in 0..=100u64 {
            verdict = watch.observe(tick * 10_000_000, 480 * 300).or(verdict);
        }
        assert_eq!(verdict, Some(true));
    }

    #[test]
    fn pace_watch_should_say_nothing_before_a_second_has_passed() {
        let mut watch = PaceWatch::default();
        assert_eq!(watch.observe(0, 480), None);
        assert_eq!(watch.observe(500_000_000, 480 * 300), None);
    }

    #[test]
    fn stream_failed_should_count_an_xrun_without_losing_the_device() {
        let clock = SharedClock::new();

        let flag = AtomicBool::new(false);
        clock.stream_failed(cpal::ErrorKind::Xrun, None);
        clock.stream_failed(cpal::ErrorKind::Xrun, Some(&flag));

        assert_eq!(clock.faults().xruns, 2);
        assert_eq!(clock.faults().stream_errors, 0);
        assert!(!clock.is_lost() && !flag.load(Ordering::Relaxed));
    }

    #[test]
    fn stream_failed_should_lose_the_device_that_failed() {
        let clock = SharedClock::new();

        let flag = AtomicBool::new(false);

        clock.stream_failed(cpal::ErrorKind::DeviceNotAvailable, Some(&flag));

        assert_eq!(clock.faults().stream_errors, 1);
        assert!(flag.load(Ordering::Relaxed));
        assert!(!clock.is_lost());
    }

    fn lamps() -> (Lamps, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
        let [ltc, mtc, click] = std::array::from_fn(|_| Arc::new(AtomicBool::new(false)));
        let lamps = Lamps {
            programme_ltc: 0b001,
            programme_click: false,
            // Two LTC outputs share the second stream; the click has a stream of its own.
            ltc: vec![Lamp::new([1, 3], &ltc)],
            mtc: vec![Lamp::new([0, 2], &mtc)],
            click: Some(Arc::clone(&click)),
        };
        (lamps, ltc, mtc, click)
    }

    #[test]
    fn health_should_report_nothing_lost_while_everything_runs() {
        let health = Health::default();
        health.replace(lamps().0);

        assert_eq!(health.ltc_lost(false), 0);
        assert_eq!(health.mtc_lost(), 0);
        assert!(!health.click_lost(false));
    }

    #[test]
    fn health_should_mark_only_the_outputs_of_the_stream_that_failed() {
        let health = Health::default();
        let (all, ltc, mtc, click) = lamps();
        health.replace(all);

        ltc.store(true, Ordering::Relaxed);
        assert_eq!((health.ltc_lost(false), health.mtc_lost()), (0b1010, 0));

        mtc.store(true, Ordering::Relaxed);
        assert_eq!(health.mtc_lost(), 0b101);
        assert!(!health.click_lost(false));

        click.store(true, Ordering::Relaxed);
        assert!(health.click_lost(false));
    }

    #[test]
    fn health_should_lose_the_outputs_on_the_programme_stream_with_it() {
        let health = Health::default();
        health.replace(Lamps {
            programme_ltc: 0b01,
            programme_click: true,
            ..lamps().0
        });

        assert_eq!(health.ltc_lost(true), 0b01);
        assert_eq!(health.ltc_lost(false), 0);
        let health = Health::default();
        health.replace(Lamps {
            programme_click: true,
            ..Lamps::default()
        });
        assert!(health.click_lost(true));
    }

    #[test]
    fn health_should_forget_the_old_flags_when_replaced() {
        let health = Health::default();
        let (all, ltc, ..) = lamps();
        health.replace(all);
        ltc.store(true, Ordering::Relaxed);

        health.replace(Lamps::default());

        assert_eq!(health.ltc_lost(false), 0);
    }

    #[test]
    fn lamp_should_give_no_bit_to_the_outputs_past_the_32nd() {
        let lost = Arc::new(AtomicBool::new(true));

        assert_eq!(Lamp::new([31, 32, 100], &lost).outputs, 1 << 31);
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn watchdog_should_stay_quiet_while_the_stream_keeps_calling_back() {
        let mut dog = Watchdog::new(ms(0));

        for step in 1..=100 {
            assert!(!dog.stalled(step, ms(step * 500)));
        }
    }

    #[test]
    fn watchdog_should_flag_a_stream_that_stops_calling_back() {
        let mut dog = Watchdog::new(ms(0));
        assert!(!dog.stalled(10, ms(100)));

        assert!(!dog.stalled(10, ms(1_100)));
        assert!(dog.stalled(10, ms(1_101)));
    }

    #[test]
    fn watchdog_should_give_a_stream_that_has_not_started_time_to_do_so() {
        let mut dog = Watchdog::new(ms(0));

        assert!(!dog.stalled(0, ms(2_900)));
        assert!(dog.stalled(0, ms(3_100)));
    }

    #[test]
    fn supervisor_should_close_a_lost_follower_and_reopen_it_a_second_later() {
        let mut supervisor = Supervisor::new(ms(0), true);
        assert_eq!(supervisor.step(ms(100), 5, false), Step::Idle);

        assert_eq!(supervisor.step(ms(200), 6, true), Step::Lose);
        assert_eq!(supervisor.step(ms(900), 6, true), Step::Idle);
        assert_eq!(supervisor.step(ms(1_200), 6, true), Step::Reopen);

        // Still not there: it waits another second.
        supervisor.reopen_failed(ms(1_200));
        assert_eq!(supervisor.step(ms(2_000), 6, true), Step::Idle);
        assert_eq!(supervisor.step(ms(2_300), 6, true), Step::Reopen);

        supervisor.reopened(ms(2_300));
        assert_eq!(supervisor.step(ms(2_400), 7, false), Step::Idle);
    }

    #[test]
    fn supervisor_should_lose_a_stream_that_went_silent_without_an_error() {
        let mut supervisor = Supervisor::new(ms(0), true);
        supervisor.step(ms(100), 5, false);

        assert_eq!(supervisor.step(ms(1_200), 5, false), Step::Lose);
    }

    #[test]
    fn supervisor_should_report_the_programme_stream_once_and_never_reopen_it() {
        let mut supervisor = Supervisor::new(ms(0), false);

        assert_eq!(supervisor.step(ms(100), 0, true), Step::Lose);
        assert_eq!(supervisor.step(ms(5_000), 0, true), Step::Idle);
    }

    /// A MIDI port that takes messages until it is told to go away, and comes back when told to.
    struct FakePort {
        present: Arc<AtomicBool>,
        sent: Sent,
        reconnects: u32,
    }

    impl MidiLink for FakePort {
        fn send(&mut self, message: &[u8]) -> bool {
            let present = self.present.load(Ordering::Relaxed);
            if present {
                self.sent.lock().unwrap().push(message.to_vec());
            }
            present
        }

        fn reconnect(&mut self) -> bool {
            self.reconnects += 1;
            self.present.load(Ordering::Relaxed)
        }
    }

    type Sent = Arc<Mutex<Vec<Vec<u8>>>>;

    fn fake_port() -> (MtcPort<FakePort>, Arc<AtomicBool>, Sent) {
        let present = Arc::new(AtomicBool::new(true));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let link = FakePort {
            present: Arc::clone(&present),
            sent: Arc::clone(&sent),
            reconnects: 0,
        };
        let port = MtcPort::new(
            link,
            Arc::new(AtomicBool::new(false)),
            Arc::new(SharedClock::new()),
        );
        (port, present, sent)
    }

    const SECOND: u64 = 1_000_000_000;

    #[test]
    fn mtc_port_should_send_while_the_port_is_there() {
        let (mut port, _, sent) = fake_port();

        port.send(&[1, 2], 0);

        assert_eq!(*sent.lock().unwrap(), [vec![1, 2]]);
        assert!(!port.is_lost());
    }

    #[test]
    fn mtc_port_should_be_lost_when_the_port_refuses_a_message_and_send_nothing_more() {
        let (mut port, present, sent) = fake_port();
        present.store(false, Ordering::Relaxed);

        port.send(&[1], 0);
        port.send(&[2], 1);

        assert!(port.is_lost());
        // Only the first refusal is counted: a lost port is not tried per message.
        assert_eq!(port.shared.faults().mtc_errors, 1);
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn mtc_port_should_try_again_once_a_second_until_the_port_is_back() {
        let (mut port, present, sent) = fake_port();
        present.store(false, Ordering::Relaxed);
        port.send(&[1], 0);

        assert!(!port.retry(SECOND / 2));
        assert_eq!(port.link.reconnects, 0);
        assert!(!port.retry(SECOND));
        assert!(!port.retry(SECOND + 1));
        assert_eq!(port.link.reconnects, 1);

        present.store(true, Ordering::Relaxed);
        assert!(port.retry(2 * SECOND));
        assert!(!port.is_lost());
        port.send(&[3], 2 * SECOND);
        assert_eq!(*sent.lock().unwrap(), [vec![3]]);
    }

    #[test]
    fn mtc_port_should_have_nothing_to_retry_while_it_works() {
        let (mut port, _, _) = fake_port();

        assert!(!port.retry(10 * SECOND));
        assert_eq!(port.link.reconnects, 0);
    }

    #[test]
    fn render_should_publish_that_the_clock_is_unpaced() {
        let (mut renderer, tx, shared) =
            programme_renderer(2, vec![0, 1], None, ClipSet::default());
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 4_800];
        assert!(!shared.is_unpaced());

        // 100 ms of audio rendered every millisecond.
        for tick in 0..=1_000u64 {
            renderer.render(&mut buffer, tick * 1_000_000);
        }

        assert!(shared.is_unpaced());
    }

    /// One LTC lane running `offset` samples ahead of the transport, on `taps`.
    fn lane(offset: i64, taps: Vec<LtcTap>) -> Vec<LtcChannel> {
        if taps.is_empty() {
            return Vec::new();
        }
        vec![LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0).with_offset(offset),
            channels: taps,
        }]
    }

    /// A follower writing mono LTC on channel 0, and the clock it follows.
    fn follower() -> (LtcFollower, Arc<SharedClock>) {
        let shared = Arc::new(SharedClock::new());
        let follower =
            LtcFollower::new(lane(0, vec![tap(0, 1.0)]), 1, clock(), Arc::clone(&shared));
        (follower, shared)
    }

    fn master_at(shared: &SharedClock, state: TransportState, epoch: u64, position: u64) {
        shared.publish(Snapshot {
            show: None,
            state,
            epoch,
            position,
            stamp: 0,
        });
    }

    /// `len` samples of LTC started at transport position `position`.
    fn ltc_from(position: u64, len: usize) -> Vec<f32> {
        let mut stream = LtcStream::new(config(), RATE, 0, 0);
        stream.resync(position);
        let mut out = vec![0.0; len];
        stream.fill(&mut out);
        out
    }

    #[test]
    fn follower_should_be_silent_while_the_master_is_not_playing() {
        let (mut follower, shared) = follower();
        let mut buffer = [1.0f32; 4_000];
        follower.render(&mut buffer, 0);
        assert!(buffer.iter().all(|&s| s == 0.0));

        master_at(&shared, TransportState::Paused, 1, 9_000);
        follower.render(&mut buffer, 0);
        assert!(buffer.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn follower_should_play_the_masters_position_on_its_channel_only() {
        let shared = Arc::new(SharedClock::new());
        let mut follower =
            LtcFollower::new(lane(0, vec![tap(2, 1.0)]), 4, clock(), Arc::clone(&shared));
        master_at(&shared, TransportState::Playing, 1, 5_000);
        let mut buffer = [0.0f32; 4 * 4_000];
        follower.render(&mut buffer, 0);

        assert_eq!(channel_of(&buffer, 4, 2), ltc_from(5_000, 4_000));
        for other in [0, 1, 3] {
            assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn follower_should_keep_its_frames_whole_while_in_step_with_the_master() {
        let (mut follower, shared) = follower();
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut first = [0.0f32; 1_000];
        follower.render(&mut first, 0);

        // The master is 1 000 samples further, and the follower is where it should be.
        master_at(&shared, TransportState::Playing, 1, 1_000);
        let mut second = [0.0f32; 1_000];
        follower.render(&mut second, 0);

        let expected = ltc_from(0, 2_000);
        assert_eq!(first, expected[..1_000]);
        assert_eq!(second, expected[1_000..]);
    }

    #[test]
    fn follower_should_ignore_a_difference_of_less_than_a_frame() {
        let (mut follower, shared) = follower();
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut first = [0.0f32; 1_000];
        follower.render(&mut first, 0);

        // A frame is 1 920 samples at 25 fps: the master is 900 ahead of where the follower is.
        master_at(&shared, TransportState::Playing, 1, 1_900);
        let mut second = [0.0f32; 1_000];
        follower.render(&mut second, 0);

        assert_eq!(second, ltc_from(0, 2_000)[1_000..]);
    }

    #[test]
    fn follower_should_start_over_when_the_clocks_drift_a_frame_apart() {
        let (mut follower, shared) = follower();
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = [0.0f32; 1_000];
        follower.render(&mut buffer, 0);

        // The follower is at 1 000; the master reports 1 920 samples (one frame) beyond that.
        master_at(&shared, TransportState::Playing, 1, 2_920);
        follower.render(&mut buffer, 0);

        assert_eq!(buffer, ltc_from(2_920, 1_000)[..]);
    }

    #[test]
    fn follower_should_start_over_after_a_seek() {
        let (mut follower, shared) = follower();
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = [0.0f32; 1_000];
        follower.render(&mut buffer, 0);

        // A seek lands where the follower would have been, but it is a new start all the same.
        master_at(&shared, TransportState::Playing, 2, 1_000);
        follower.render(&mut buffer, 0);

        assert_eq!(buffer, ltc_from(1_000, 1_000)[..]);
    }

    #[test]
    fn follower_should_start_over_when_playing_resumes_after_a_pause() {
        let (mut follower, shared) = follower();
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = [0.0f32; 1_000];
        follower.render(&mut buffer, 0);
        master_at(&shared, TransportState::Paused, 1, 1_000);
        follower.render(&mut buffer, 0);

        master_at(&shared, TransportState::Playing, 1, 1_000);
        follower.render(&mut buffer, 0);

        assert_eq!(buffer, ltc_from(1_000, 1_000)[..]);
    }

    /// 1 ms is 48 samples at the engine rate.
    const AHEAD: i64 = 480;

    #[test]
    fn follower_should_play_the_position_plus_its_offset() {
        let shared = Arc::new(SharedClock::new());
        let mut follower = LtcFollower::new(
            lane(AHEAD, vec![tap(0, 1.0)]),
            1,
            clock(),
            Arc::clone(&shared),
        );
        let mut buffer = [0.0f32; 4_000];
        master_at(&shared, TransportState::Playing, 1, 5_000);
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, ltc_from(5_000 + AHEAD as u64, 4_000)[..]);

        // A seek starts over from the new position, still shifted.
        master_at(&shared, TransportState::Playing, 2, 30_000);
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, ltc_from(30_000 + AHEAD as u64, 4_000)[..]);

        // So does resuming after a pause.
        master_at(&shared, TransportState::Paused, 2, 31_000);
        follower.render(&mut buffer, 0);
        master_at(&shared, TransportState::Playing, 2, 31_000);
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, ltc_from(31_000 + AHEAD as u64, 4_000)[..]);
    }

    #[test]
    fn follower_should_play_a_negative_offset_behind_the_position() {
        let shared = Arc::new(SharedClock::new());
        let mut follower = LtcFollower::new(
            lane(-AHEAD, vec![tap(0, 1.0)]),
            1,
            clock(),
            Arc::clone(&shared),
        );
        let mut buffer = [0.0f32; 4_000];
        master_at(&shared, TransportState::Playing, 1, 5_000);
        follower.render(&mut buffer, 0);
        assert_eq!(buffer, ltc_from(5_000 - AHEAD as u64, 4_000)[..]);
    }

    #[test]
    fn render_should_shift_ltc_by_the_offset_across_a_seek_and_a_stop() {
        let (tx, rx) = mpsc::channel();
        let ltc = LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0).with_offset(AHEAD),
            channels: vec![tap(0, 1.0)],
        };
        let mut renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
            Arc::new(ClipSet::default()),
            Vec::new(),
            Some(ltc),
            1,
            Arc::new(SharedClock::new()),
        );
        let mut buffer = [0.0f32; 1_000];
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut buffer, 10);
        assert_eq!(buffer, ltc_from(AHEAD as u64, 1_000)[..]);

        // Frame 10 starts at sample 19 200.
        tx.send(Message::Transport(Command::Seek(10))).unwrap();
        renderer.render(&mut buffer, 20);
        assert_eq!(buffer, ltc_from(19_200 + AHEAD as u64, 1_000)[..]);

        // Stop goes back to the origin, and playing again starts the shifted signal over.
        tx.send(Message::Transport(Command::Stop)).unwrap();
        renderer.render(&mut buffer, 30);
        tx.send(Message::Transport(Command::Play)).unwrap();
        renderer.render(&mut buffer, 40);
        assert_eq!(buffer, ltc_from(AHEAD as u64, 1_000)[..]);
    }

    #[test]
    fn ltc_lanes_should_share_a_generator_between_outputs_with_the_same_offset() {
        let send = |channel, offset_ms| LtcSend {
            channel,
            level: 1.0,
            offset_ms,
        };
        let lanes = ltc_lanes(
            &[send(1, 0), send(2, 5), send(3, 0)],
            config(),
            RATE,
            0,
            clock(),
        );
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].channels.len(), 2);
        assert_eq!(lanes[1].channels.len(), 1);
        assert!(ltc_lanes(&[], config(), RATE, 0, clock()).is_empty());
    }

    #[test]
    fn plan_ltc_should_take_the_offset_of_the_first_output_on_a_slot() {
        let mut early = ltc_out("Main", 3);
        early.offset_ms = Some(20);
        let mut late = ltc_out("Main", 3);
        late.offset_ms = Some(-20);
        let plan = plan_ltc(&[early, late, ltc_out("Main", 4)], "Main", loosely);
        let offsets: Vec<_> = sends_of(&plan.shared).iter().map(|s| s.offset_ms).collect();
        assert_eq!(offsets, vec![20, 0]);
    }

    #[test]
    fn prune_should_free_only_the_sets_nothing_else_holds() {
        let playing = Arc::new(ClipSet::default());
        let old = Arc::new(ClipSet::default());
        // The callback holds `playing` through a clone; `old` was swapped out and released.
        let callback_copy = Arc::clone(&playing);
        let mut sets = vec![Arc::clone(&old), Arc::clone(&playing)];
        drop(old);

        prune_clip_sets(&mut sets);

        assert_eq!(sets.len(), 1);
        assert!(Arc::ptr_eq(&sets[0], &callback_copy));
    }

    #[test]
    fn mtc_wait_should_park_until_the_spin_window_and_spin_inside_it() {
        let ms = Duration::from_millis;
        assert_eq!(mtc_wait(ms(8), true), MtcWait::Park(MTC_TICK));
        assert_eq!(mtc_wait(MTC_TICK, true), MtcWait::Park(MTC_TICK - MTC_SPIN));
        assert_eq!(mtc_wait(MTC_SPIN, true), MtcWait::Spin(MTC_SPIN));
    }

    #[test]
    fn mtc_wait_should_never_spin_on_a_stale_position() {
        assert_eq!(mtc_wait(Duration::ZERO, false), MtcWait::Park(MTC_TICK));
    }

    #[test]
    fn follower_should_follow_the_wall_clock_between_the_masters_updates() {
        let (mut follower, shared) = follower();
        // Published at 0 ns; 10 ms later the master has played 480 more samples.
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = [0.0f32; 1_000];
        follower.render(&mut buffer, 10_000_000);

        assert_eq!(buffer, ltc_from(480, 1_000)[..]);
    }

    #[test]
    fn render_should_put_one_ltc_signal_on_every_channel_it_is_given() {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(SharedClock::new());
        let ltc = LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0),
            channels: vec![tap(1, 1.0), tap(3, 1.0)],
        };
        let mut renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
            Arc::new(ClipSet::default()),
            Vec::new(),
            Some(ltc),
            4,
            shared,
        );
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 4 * 400];
        renderer.render(&mut buffer, 10);

        let first = channel_of(&buffer, 4, 1);
        assert!(first.iter().any(|&s| s != 0.0));
        assert_eq!(first, channel_of(&buffer, 4, 3));
        for other in [0, 2] {
            assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
        }
    }

    #[test]
    fn render_should_scale_each_ltc_channel_by_its_level() {
        let (tx, rx) = mpsc::channel();
        let ltc = LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0),
            channels: vec![tap(0, 1.0), tap(1, 0.25)],
        };
        let mut renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
            Arc::new(ClipSet::default()),
            Vec::new(),
            Some(ltc),
            2,
            Arc::new(SharedClock::new()),
        );
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = [0.0f32; 2 * 400];
        renderer.render(&mut buffer, 10);

        let peak = |channel| {
            channel_of(&buffer, 2, channel)
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
        };
        assert_eq!(peak(0), config().level);
        assert_eq!(peak(1), config().level * 0.25);
    }

    #[test]
    fn plan_ltc_should_take_the_level_of_the_first_output_on_a_slot() {
        let mut quiet = ltc_out("Main", 3);
        quiet.level = Some(0.25);
        let mut loud = ltc_out("Main", 3);
        loud.level = Some(1.0);
        let plan = plan_ltc(&[quiet, loud, ltc_out("Main", 4)], "Main", loosely);
        let levels: Vec<_> = sends_of(&plan.shared)
            .iter()
            .map(|s| (s.channel, s.level))
            .collect();
        assert_eq!(levels, vec![(3, 0.25), (4, DEFAULT_LTC_LEVEL)]);
    }

    #[test]
    fn follower_should_put_one_ltc_signal_on_every_channel_it_is_given() {
        let shared = Arc::new(SharedClock::new());
        let mut follower = LtcFollower::new(
            lane(0, vec![tap(0, 1.0), tap(2, 1.0)]),
            3,
            clock(),
            Arc::clone(&shared),
        );
        master_at(&shared, TransportState::Playing, 1, 5_000);
        let mut buffer = [0.0f32; 3 * 4_000];
        follower.render(&mut buffer, 0);

        assert_eq!(channel_of(&buffer, 3, 0), ltc_from(5_000, 4_000));
        assert_eq!(channel_of(&buffer, 3, 2), ltc_from(5_000, 4_000));
        assert!(channel_of(&buffer, 3, 1).iter().all(|&s| s == 0.0));
    }

    fn ltc_out(device: &str, channel: u16) -> LtcOutput {
        LtcOutput {
            device_name: device.into(),
            channel,
            level: None,
            offset_ms: None,
        }
    }

    /// Hints that differ only in case name the same device.
    fn loosely(a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }

    fn slot(channel: u16, outputs: &[usize]) -> LtcSlot {
        LtcSlot {
            channel,
            level: DEFAULT_LTC_LEVEL,
            offset_ms: 0,
            outputs: outputs.to_vec(),
        }
    }

    use flux_project::{AudioOutput, MetronomeOutput};

    #[derive(Clone)]
    struct RoutedProject {
        output: OutputConfig,
        timelines: Vec<Timeline>,
    }

    fn project_with(edit: impl FnOnce(&mut OutputConfig)) -> RoutedProject {
        let mut output = OutputConfig::default();
        edit(&mut output);
        RoutedProject {
            output,
            timelines: flux_project::Project::new("p", FrameRate::Fps25).timelines,
        }
    }

    fn plan_of(project: &RoutedProject) -> Plan {
        Plan::build(&project.output, &project.timelines[0], None, loosely)
    }

    fn main_device(name: &str) -> Option<AudioOutput> {
        Some(AudioOutput {
            device_name: name.into(),
            channels: vec![1, 2],
        })
    }

    fn mtc_out(port: &str) -> MtcOutput {
        MtcOutput {
            port_name: port.into(),
            port_index: 0,
            offset_ms: None,
        }
    }

    fn click_on(device: &str) -> Option<MetronomeOutput> {
        Some(MetronomeOutput {
            device_name: device.into(),
            channels: vec![1, 2],
            gain_db: 0.0,
        })
    }

    #[test]
    fn unassigned_signals_stay_off_without_opening_branches() {
        let project = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out(flux_project::UNASSIGNED_DEVICE, 1)];
            o.mtc = vec![mtc_out("")];
        });
        let plan = plan_of(&project);
        let mut branches = Branches::default();
        let mut warnings = Vec::new();
        let opened = branches.apply(
            &plan,
            &BranchCtx::new(None, &project.timelines[0]),
            &Arc::new(SharedClock::new()),
            "Main",
            &mut warnings,
        );
        assert_eq!(opened.ltc.len(), 1);
        assert!(!opened.ltc[0].ok && !opened.mtc[0].ok);
        assert!(branches.followers.is_empty() && branches.mtc.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn pair_should_match_each_wanted_item_with_a_running_one_once() {
        let (paired, unused) = pair(&[1, 2, 2, 4], &[2, 5, 1, 2], |want, running| {
            want == running
        });

        assert_eq!(paired, [Some(2), Some(0), Some(3), None]);
        assert_eq!(unused, [1]);
    }

    #[test]
    fn pair_should_leave_everything_unused_when_nothing_is_wanted() {
        let (paired, unused) = pair::<u8, u8>(&[], &[1, 2], |a, b| a == b);

        assert!(paired.is_empty());
        assert_eq!(unused, [0, 1]);
    }

    #[test]
    fn plan_should_not_touch_the_programme_for_an_ltc_or_mtc_output_on_another_device() {
        let base = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Main", 3)];
        });
        let more = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Main", 3), ltc_out("Booth", 1)];
            o.mtc = vec![mtc_out("Port")];
        });

        let (before, after) = (plan_of(&base), plan_of(&more));

        assert_eq!(before.master, after.master);
        assert_eq!(before.click, after.click);
        assert_eq!(after.followers.len(), 1);
        assert_eq!(after.mtc.len(), 1);
    }

    #[test]
    fn plan_should_not_mind_where_in_the_list_an_other_device_output_sits() {
        let first = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Main", 3), ltc_out("Booth", 1)];
        });
        let moved = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Stage", 1), ltc_out("Main", 3), ltc_out("Booth", 1)];
        });

        assert_eq!(plan_of(&first).master, plan_of(&moved).master);
    }

    #[test]
    fn plan_should_ask_for_the_programme_to_reopen_when_ltc_on_its_device_changes() {
        let base = project_with(|o| o.audio = main_device("Main"));
        let with_ltc = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Main", 3)];
        });

        assert_ne!(plan_of(&base).master, plan_of(&with_ltc).master);
    }

    #[test]
    fn plan_should_ask_for_the_programme_to_reopen_when_its_device_channels_or_clock_change() {
        let base = project_with(|o| o.audio = main_device("Main"));
        let device = project_with(|o| o.audio = main_device("Other"));
        let channels = project_with(|o| {
            o.audio = Some(AudioOutput {
                device_name: "Main".into(),
                channels: vec![3, 4],
            })
        });
        let mut rate = base.clone();
        rate.timelines[0].timecode.frame_rate = FrameRate::Fps30;
        let mut start = base.clone();
        start.timelines[0].timecode.start = 90_000;

        let base = plan_of(&base).master;
        for changed in [&device, &channels, &rate, &start] {
            assert_ne!(base, plan_of(changed).master);
        }
    }

    #[test]
    fn plan_should_ask_for_a_new_output_when_the_metronome_changes() {
        let off = project_with(|o| o.audio = main_device("Main"));
        let on = project_with(|o| {
            o.audio = main_device("Main");
            o.metronome = click_on("Booth");
        });
        let louder = project_with(|o| {
            o.audio = main_device("Main");
            o.metronome = Some(MetronomeOutput {
                gain_db: -6.0,
                ..click_on("Booth").unwrap()
            });
        });

        assert_ne!(plan_of(&off).click, plan_of(&on).click);
        assert_ne!(plan_of(&on).click, plan_of(&louder).click);
        assert_eq!(plan_of(&on).click, plan_of(&on).click);
    }

    #[test]
    fn plan_should_follow_the_metronome_to_the_stream_it_plays_on() {
        let project = project_with(|o| {
            o.audio = main_device("Main");
            o.ltc = vec![ltc_out("Booth", 1)];
            o.metronome = click_on("booth");
        });

        let plan = plan_of(&project);

        assert_eq!(
            plan.click.map(|click| click.place),
            Some(ClickPlaceKey::Group("Booth".into()))
        );
        assert!(plan.followers[0].click.is_some());
        assert!(plan.own_click.is_none() && plan.programme_click.is_none());
    }

    #[test]
    fn plan_should_keep_the_running_metronome_for_the_streams_it_opens() {
        let project = project_with(|o| {
            o.audio = main_device("Main");
            o.metronome = click_on("Booth");
        });
        let running = plan_of(&project).metronome.unwrap();

        let plan = Plan::build(
            &project.output,
            &project.timelines[0],
            Some(Arc::clone(&running)),
            loosely,
        );

        assert!(Arc::ptr_eq(&plan.metronome.unwrap(), &running));
        assert!(Arc::ptr_eq(&plan.own_click.unwrap().1.shared, &running));
    }

    #[test]
    fn plan_ltc_should_split_outputs_between_the_programme_stream_and_other_devices() {
        let outputs = [
            ltc_out("Main", 3),
            ltc_out("Booth", 1),
            ltc_out("MAIN", 4),
            ltc_out("booth", 2),
            ltc_out("Stage", 1),
        ];
        let plan = plan_ltc(&outputs, "main", loosely);
        assert_eq!(plan.shared, vec![slot(3, &[0]), slot(4, &[2])]);
        assert_eq!(
            plan.own,
            vec![
                LtcGroup {
                    device: "Booth".into(),
                    slots: vec![slot(1, &[1]), slot(2, &[3])],
                },
                LtcGroup {
                    device: "Stage".into(),
                    slots: vec![slot(1, &[4])],
                },
            ]
        );
    }

    #[test]
    fn plan_ltc_should_play_a_repeated_device_and_channel_once_for_all_its_outputs() {
        let outputs = [
            ltc_out("Main", 3),
            ltc_out("Main", 3),
            ltc_out("Booth", 1),
            ltc_out("Booth", 1),
        ];
        let plan = plan_ltc(&outputs, "Main", loosely);
        assert_eq!(plan.shared, vec![slot(3, &[0, 1])]);
        assert_eq!(plan.own.len(), 1);
        assert_eq!(plan.own[0].slots, vec![slot(1, &[2, 3])]);
        assert_eq!(sends_of(&plan.shared).len(), 1);
    }

    #[test]
    fn plan_ltc_should_be_empty_without_outputs() {
        let plan = plan_ltc(&[], "Main", loosely);
        assert!(plan.shared.is_empty() && plan.own.is_empty());
    }

    #[test]
    fn output_info_should_serialise_the_outputs_as_lists() {
        let info = OutputInfo {
            operator: None,
            device: "Main".into(),
            sample_rate: 48_000,
            channels: 4,
            ltc: vec![LtcInfo {
                device: "Main".into(),
                channel: 3,
                ok: true,
            }],
            mtc: vec![MtcInfo {
                port: "IAC".into(),
                ok: false,
            }],
            cues: vec![],
            metronome: Some(MetronomeInfo {
                device: "Band".into(),
                ok: true,
            }),
            warnings: Vec::new(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["ltc"][0]["channel"], 3);
        assert_eq!(json["ltc"][0]["ok"], true);
        assert_eq!(json["mtc"][0]["port"], "IAC");
        assert_eq!(json["mtc"][0]["ok"], false);
        assert_eq!(json["metronome"]["device"], "Band");
    }

    fn click_out(bpm: f64, click: bool, channels: Vec<usize>) -> ClickOut {
        ClickOut {
            shared: Arc::new(SharedMetronome::new(
                Tempo {
                    bpm,
                    beats_per_bar: 4,
                    click,
                },
                0.0,
            )),
            channels,
        }
    }

    /// A programme renderer that also plays a metronome, over `clips`.
    fn clicking_renderer(
        channels: usize,
        programme: Vec<usize>,
        click: ClickOut,
        clips: ClipSet,
    ) -> (Renderer, Sender<Message>) {
        let (renderer, tx, _) = programme_renderer(channels, programme, None, clips);
        (renderer.with_click(Some(click)), tx)
    }

    #[test]
    fn render_should_click_on_its_own_channel_beside_the_programme() {
        let (mut renderer, tx) = clicking_renderer(
            4,
            vec![0, 1],
            click_out(120.0, true, vec![2]),
            clip(16_384, -16_384, 24_000),
        );
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = vec![0.0f32; 4 * 4_000];

        renderer.render(&mut buffer, 10);

        assert!(channel_of(&buffer, 4, 0).iter().all(|&s| s == 0.5));
        assert!(channel_of(&buffer, 4, 2).iter().any(|&s| s.abs() > 0.1));
        assert!(channel_of(&buffer, 4, 3).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn render_should_add_the_click_to_a_programme_channel_it_shares() {
        let (mut renderer, tx) = clicking_renderer(
            2,
            vec![0, 1],
            click_out(120.0, true, vec![0]),
            clip(8_192, 8_192, 24_000),
        );
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = vec![0.0f32; 2 * 2_000];

        renderer.render(&mut buffer, 10);

        let shared = channel_of(&buffer, 2, 0);
        assert!(
            shared.iter().any(|&s| (s - 0.25).abs() > 0.05),
            "no click on it"
        );
        assert!(channel_of(&buffer, 2, 1).iter().all(|&s| s == 0.25));
    }

    #[test]
    fn render_should_not_click_while_the_click_is_off_or_the_show_is_not_playing() {
        let click = click_out(120.0, false, vec![1]);
        let shared = Arc::clone(&click.shared);
        let (mut renderer, tx) = clicking_renderer(2, vec![0], click, ClipSet::default());
        tx.send(Message::Transport(Command::Play)).unwrap();
        let mut buffer = vec![0.0f32; 2 * 2_000];
        renderer.render(&mut buffer, 10);
        assert!(channel_of(&buffer, 2, 1).iter().all(|&s| s == 0.0));

        // Switching it on while the show runs makes it sound without rebuilding anything.
        shared.set(Tempo {
            bpm: 120.0,
            beats_per_bar: 4,
            click: true,
        });
        tx.send(Message::Transport(Command::Seek(0))).unwrap();
        renderer.render(&mut buffer, 20);
        assert!(channel_of(&buffer, 2, 1).iter().any(|&s| s.abs() > 0.1));

        tx.send(Message::Transport(Command::Pause)).unwrap();
        renderer.render(&mut buffer, 30);
        assert!(buffer.iter().all(|&s| s == 0.0));
    }

    /// A follower on a 2-channel device with LTC on channel 0 (or none) and the click on channel 1.
    fn clicking_follower(ltc: bool) -> (LtcFollower, Arc<SharedClock>) {
        let shared = Arc::new(SharedClock::new());
        let follower = LtcFollower::new(
            lane(0, if ltc { vec![tap(0, 1.0)] } else { Vec::new() }),
            2,
            clock(),
            Arc::clone(&shared),
        )
        .with_click(Some(click_out(120.0, true, vec![1])));
        (follower, shared)
    }

    #[test]
    fn follower_should_click_in_step_with_the_masters_position() {
        let (mut follower, shared) = clicking_follower(true);
        // The master is exactly on beat 1 of 120 BPM (24 000 samples).
        master_at(&shared, TransportState::Playing, 1, 24_000);
        let mut buffer = vec![0.0f32; 2 * 2_000];

        follower.render(&mut buffer, 0);

        let click = channel_of(&buffer, 2, 1);
        assert!(
            click[..200].iter().any(|&s| s.abs() > 0.1),
            "no click on the beat"
        );
        assert!(
            channel_of(&buffer, 2, 0).iter().any(|&s| s != 0.0),
            "no LTC beside it"
        );
    }

    #[test]
    fn follower_with_no_ltc_should_play_only_the_click() {
        let (mut follower, shared) = clicking_follower(false);
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = vec![0.0f32; 2 * 2_000];

        follower.render(&mut buffer, 0);

        assert!(channel_of(&buffer, 2, 0).iter().all(|&s| s == 0.0));
        assert!(channel_of(&buffer, 2, 1).iter().any(|&s| s.abs() > 0.1));
    }

    #[test]
    fn follower_should_keep_its_own_count_across_buffers_while_in_step() {
        let (mut follower, shared) = clicking_follower(false);
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut first = vec![0.0f32; 2 * 4_000];
        follower.render(&mut first, 0);
        // The master has not published again, but 4 000 samples of time (83.3 ms) have gone by: the
        // follower is where it expects to be, so it carries on instead of starting over.
        let mut second = vec![0.0f32; 2 * 4_000];
        follower.render(&mut second, 83_333_333);

        let expected = SharedMetronome::new(
            Tempo {
                bpm: 120.0,
                beats_per_bar: 4,
                click: true,
            },
            0.0,
        )
        .grid(ENGINE_RATE)
        .unwrap();
        let mut whole = vec![0.0f32; 8_000];
        expected.add(0, &mut whole);
        let mut heard = channel_of(&first, 2, 1);
        heard.extend(channel_of(&second, 2, 1));
        assert_eq!(heard, whole);
    }

    #[test]
    fn follower_should_be_silent_when_the_master_stops() {
        let (mut follower, shared) = clicking_follower(false);
        master_at(&shared, TransportState::Playing, 1, 0);
        let mut buffer = vec![0.0f32; 2 * 2_000];
        follower.render(&mut buffer, 0);
        master_at(&shared, TransportState::Stopped, 2, 0);
        follower.render(&mut buffer, 0);
        assert!(buffer.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn click_spec_should_leave_out_the_channels_a_device_does_not_have() {
        let spec = ClickSpec {
            shared: click_out(120.0, true, Vec::new()).shared,
            channels: vec![1, 3, 9],
        };
        assert_eq!(spec.out(4).channels, vec![0, 2]);
    }
}
