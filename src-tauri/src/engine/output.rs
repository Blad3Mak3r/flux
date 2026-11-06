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
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::{Serialize, Serializer};
use thiserror::Error;

use super::ltc::{LtcConfig, LtcStream};
use super::metronome::{ClickOut, SharedMetronome};
use super::mixer::{ClipSet, MIX_CHUNK, Programme, SharedLevels};
use super::mtc::MtcScheduler;
use super::transport::{Clock, Command, Transport, TransportState};
use crate::audio::ENGINE_RATE;
use crate::devices;
use crate::project::{
    DEFAULT_LTC_LEVEL, FrameRate, LtcOutput, Project, ProjectError, Tempo, Timeline,
};

const MIDI_CLIENT_NAME: &str = "Flux Timecode";
/// The longest the MTC thread sleeps before it looks at the transport again.
const MTC_TICK: Duration = Duration::from_millis(1);
/// The last stretch before a quarter frame that the MTC thread spins instead of sleeping, because a
/// sleep can overshoot by more than this.
const MTC_SPIN: Duration = Duration::from_micros(200);
/// The audio thread keeps its stream alive and wakes up this often to see whether to stop.
const AUDIO_PARK: Duration = Duration::from_millis(50);
/// The published position is one buffer old at most; extrapolating further than this would only
/// hide a stalled stream.
const MAX_EXTRAPOLATION: Duration = Duration::from_millis(100);
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
    #[error("the output is not running")]
    NotRunning,
    #[error(transparent)]
    Project(#[from] ProjectError),
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for OutputError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// What the control side tells the audio callback.
#[derive(Debug)]
pub enum Message {
    Transport(Command),
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
    snapshot: Mutex<Snapshot>,
    base: Instant,
    lost: AtomicBool,
    /// The LTC stream, when it runs on a device of its own, failed or went away.
    ltc_lost: AtomicBool,
    unpaced: AtomicBool,
    /// How many times the show ran to its end by itself (not stopped by the user).
    shows_ended: AtomicU64,
}

impl SharedClock {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(Snapshot {
                state: TransportState::Stopped,
                epoch: 0,
                position: 0,
                stamp: 0,
            }),
            base: Instant::now(),
            lost: AtomicBool::new(false),
            ltc_lost: AtomicBool::new(false),
            unpaced: AtomicBool::new(false),
            shows_ended: AtomicU64::new(0),
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

    /// Whether the LTC device, when it is not the programme's, failed or went away while running.
    pub fn is_ltc_lost(&self) -> bool {
        self.ltc_lost.load(Ordering::Relaxed)
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
    fn new((channel, level): (u16, f32)) -> Self {
        Self {
            channel: usize::from(channel) - 1,
            gain: level,
        }
    }
}

/// The LTC generator and the interface channels it goes to. One signal feeds every channel, so
/// two LTC outputs on one device cost a single generator; each channel has a level of its own.
#[derive(Debug)]
pub struct LtcChannel {
    pub stream: LtcStream,
    pub channels: Vec<LtcTap>,
}

/// Fills audio buffers from the transport. Owned by the audio callback, so it must not allocate
/// or wait.
#[derive(Debug)]
pub struct Renderer {
    transport: Transport,
    messages: Receiver<Message>,
    clips: Arc<ClipSet>,
    /// Zero-based interface channels the programme goes to: left then right, or one for both.
    programme: Vec<usize>,
    ltc: Option<LtcChannel>,
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
        ltc: Option<LtcChannel>,
        channels: usize,
        shared: Arc<SharedClock>,
    ) -> Self {
        transport.set_end(clips.end());
        Self {
            seen_epoch: transport.epoch(),
            transport,
            messages,
            clips,
            programme,
            ltc,
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
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Transport(command) => self.transport.apply(command),
                Message::Clips(clips) => {
                    self.transport.set_end(clips.end());
                    self.clips = clips;
                }
            }
        }
        if self.transport.epoch() != self.seen_epoch {
            self.seen_epoch = self.transport.epoch();
            if let Some(ltc) = &mut self.ltc {
                ltc.stream.resync(self.transport.position_samples());
            }
        }

        data.fill(T::EQUILIBRIUM);
        if self.transport.state() == TransportState::Playing {
            self.mix(data);
        }

        let frames = (data.len() / self.channels) as u64;
        if self.transport.advance(frames) {
            self.shared.shows_ended.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(unpaced) = self.pace.observe(now, frames) {
            self.shared.unpaced.store(unpaced, Ordering::Relaxed);
        }
        self.shared.publish(Snapshot {
            state: self.transport.state(),
            epoch: self.transport.epoch(),
            position: self.transport.position_samples(),
            stamp: now,
        });
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
            self.clips
                .mix(at_chunk, &mut left[..frames], &mut right[..frames]);
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
                if let Some(ltc) = &mut self.ltc {
                    let sample = ltc.stream.next_sample();
                    for tap in &ltc.channels {
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

/// Plays LTC on an audio device that is not the master's. The two sample clocks are independent
/// and drift apart, so this does not count samples against the transport: it reads where the
/// master is, plays LTC from there, and starts over from the master's position whenever it is a
/// frame or more away, or the master seeks.
#[derive(Debug)]
pub struct LtcFollower {
    stream: LtcStream,
    outputs: Vec<LtcTap>,
    channels: usize,
    clock: Clock,
    shared: Arc<SharedClock>,
    /// The last position read; kept when the master is publishing and the read is skipped.
    snapshot: Snapshot,
    /// The transport position of the next sample `stream` writes. `None` until synchronised, and
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
        stream: LtcStream,
        outputs: Vec<LtcTap>,
        channels: usize,
        clock: Clock,
        shared: Arc<SharedClock>,
    ) -> Self {
        let snapshot = shared.load();
        Self {
            stream,
            outputs,
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
        if let Some(snapshot) = self.shared.try_load() {
            self.snapshot = snapshot;
        }
        let snapshot = self.snapshot;
        if snapshot.state != TransportState::Playing {
            self.next = None;
            self.click_next = None;
            self.seen_epoch = snapshot.epoch;
            return;
        }

        let target = snapshot.estimated_position(now, self.clock);
        let one_frame = self.clock.frames_to_samples(1);
        let in_step = self.seen_epoch == snapshot.epoch
            && self
                .next
                .is_some_and(|next| next.abs_diff(target) < one_frame);
        if !in_step {
            self.seen_epoch = snapshot.epoch;
            self.stream.resync(target);
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
            click_at += frames as u64;
            for (at, frame) in chunk.chunks_exact_mut(self.channels).enumerate() {
                // LTC is written after the click, so it wins a channel both ask for.
                if click[at] != 0.0 {
                    for &channel in click_channels {
                        frame[channel] = T::from_sample(click[at].clamp(-1.0, 1.0));
                    }
                }
                let sample = self.stream.next_sample();
                for tap in &self.outputs {
                    frame[tap.channel] = T::from_sample(sample * tap.gain);
                }
            }
        }
        self.click_next = grid.map(|_| click_at);
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
    pub device_lost: bool,
    /// The LTC device, when it is not the programme's, failed or went away.
    pub ltc_lost: bool,
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
    /// How many times the show has run to its end by itself: a change says it just ended, which
    /// the position alone cannot (a stop from the user looks the same).
    pub fn shows_ended(&self) -> u64 {
        self.shared.shows_ended()
    }

    pub fn position(&self) -> Position {
        let snapshot = self.shared.load();
        let position = snapshot.estimated_position(self.shared.now(), self.clock);
        Position {
            state: snapshot.state,
            frame: self.start + self.clock.samples_to_frames(position),
            position_samples: position,
            device_lost: self.shared.is_lost(),
            ltc_lost: self.shared.is_ltc_lost(),
            clock_unpaced: self.shared.is_unpaced(),
        }
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
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    info: OutputInfo,
}

impl Output {
    /// Opens the devices a project's `output` section names and starts the stream, stopped at
    /// the timeline start. The audio device is required because its clock drives everything; a
    /// missing MIDI port only produces a warning.
    pub fn start(
        project: &Project,
        timeline: &Timeline,
        programme: Programme,
    ) -> Result<Self, OutputError> {
        let output = &project.output;
        // An empty device name is the system's default output, so a project without any device
        // chosen still plays. A project written before `output.audio` existed carries the
        // programme on the first LTC device.
        let device_hint = match (&output.audio, output.ltc.first()) {
            (Some(audio), _) => audio.device_name.clone(),
            (None, Some(ltc)) => ltc.device_name.clone(),
            (None, None) => String::new(),
        };
        let programme_channels: Vec<u16> = output
            .audio
            .as_ref()
            .map(|audio| audio.channels.clone())
            .filter(|channels| !channels.is_empty())
            .unwrap_or_else(|| vec![1, 2]);
        // LTC shares the programme's stream unless it names another device, which then gets a
        // stream of its own.
        let LtcPlan {
            shared: shared_ltc,
            own: own_ltc,
        } = plan_ltc(&output.ltc, &device_hint, same_device);
        // The metronome has an output of its own. It plays on the programme's stream when it names
        // the programme's device, on the stream of an LTC device it shares, or else on a stream
        // of its own: no device is opened twice.
        let click = output.metronome.as_ref().map(|metronome| {
            let channels = if metronome.channels.is_empty() {
                vec![1, 2]
            } else {
                metronome.channels.clone()
            };
            let place = if same_device(&metronome.device_name, &device_hint) {
                ClickPlace::Programme
            } else if let Some(group) = own_ltc
                .iter()
                .position(|group| same_device(&group.device, &metronome.device_name))
            {
                ClickPlace::Group(group)
            } else {
                ClickPlace::Own
            };
            let spec = ClickSpec {
                shared: Arc::new(SharedMetronome::new(timeline.tempo, metronome.gain_db)),
                channels,
            };
            (metronome.device_name.clone(), place, spec)
        });
        let click_for = |wanted: ClickPlace| {
            click
                .as_ref()
                .filter(|(_, place, _)| *place == wanted)
                .map(|(_, _, spec)| spec.clone())
        };
        let needed = shared_ltc
            .iter()
            .map(|slot| slot.channel)
            .chain(programme_channels.iter().copied())
            .chain(
                click_for(ClickPlace::Programme)
                    .into_iter()
                    .flat_map(|spec| spec.channels),
            )
            .max()
            .unwrap_or(2);

        // The generator runs at full scale and every channel applies its own level.
        let ltc_config = LtcConfig {
            level: 1.0,
            ..LtcConfig::default()
        };
        let rate = timeline.timecode.frame_rate;
        let start = timeline.timecode.start;
        let clock = Clock::new(ltc_config.sample_rate, rate);
        let shared = Arc::new(SharedClock::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (messages, message_rx) = mpsc::channel();
        let Programme { set, warnings } = programme;
        let clips = Arc::new(set);
        let levels = SharedLevels::new(clips.levels());

        let spec = AudioSpec {
            device_hint,
            needed,
            programme: programme_channels,
            clips: Arc::clone(&clips),
            ltc: levels_of(&shared_ltc),
            click: click_for(ClickPlace::Programme),
            ltc_config,
            rate,
            start,
            clock,
        };
        let (audio_thread, audio) = spawn_stream("flux-audio", Arc::clone(&stop), {
            let shared = Arc::clone(&shared);
            move || open_stream(spec, message_rx, shared)
        })?;

        let mut threads = vec![audio_thread];
        let mut warnings = warnings;
        // One entry per project output, in the project's order, whatever stream it landed on.
        let mut ltc_info: Vec<Option<LtcInfo>> = vec![None; output.ltc.len()];
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
        place(&shared_ltc, &audio.device, true);
        let mut metronome_info = click.as_ref().map(|(device, place, _)| MetronomeInfo {
            device: if *place == ClickPlace::Programme {
                audio.device.clone()
            } else {
                device.clone()
            },
            ok: *place == ClickPlace::Programme,
        });
        for (index, group) in own_ltc.into_iter().enumerate() {
            let spec = LtcSpec {
                device_hint: group.device.clone(),
                ltc: levels_of(&group.slots),
                click: click_for(ClickPlace::Group(index)),
                config: ltc_config,
                rate,
                start,
                clock,
            };
            let with_click = spec.click.is_some();
            let opened = spawn_stream("flux-ltc", Arc::clone(&stop), {
                let shared = Arc::clone(&shared);
                move || open_ltc_stream(spec, shared)
            });
            match opened {
                Ok((handle, device)) => {
                    threads.push(handle);
                    place(&group.slots, &device, true);
                    if let (true, Some(info)) = (with_click, &mut metronome_info) {
                        *info = MetronomeInfo { device, ok: true };
                    }
                }
                Err(err) => {
                    warnings.push(format!("LTC is off on {}, {err}", group.device));
                    place(&group.slots, &group.device, false);
                    if with_click {
                        warnings.push(format!("The metronome is off on {}, {err}", group.device));
                    }
                }
            }
        }
        // A device that carries nothing but the metronome gets a stream that plays only the click.
        if let Some(spec) = click_for(ClickPlace::Own) {
            let device = click.as_ref().map(|(device, _, _)| device.clone());
            let device = device.unwrap_or_default();
            let spec = LtcSpec {
                device_hint: device.clone(),
                ltc: Vec::new(),
                click: Some(spec),
                config: ltc_config,
                rate,
                start,
                clock,
            };
            let opened = spawn_stream("flux-click", Arc::clone(&stop), {
                let shared = Arc::clone(&shared);
                move || open_ltc_stream(spec, shared)
            });
            match opened {
                Ok((handle, name)) => {
                    threads.push(handle);
                    metronome_info = Some(MetronomeInfo {
                        device: name,
                        ok: true,
                    });
                }
                Err(err) => warnings.push(format!("The metronome is off on {device}, {err}")),
            }
        }
        let ltc_info: Vec<LtcInfo> = ltc_info.into_iter().flatten().collect();

        // Likewise for MTC: one thread per port, and a port listed twice (or named two ways)
        // shares the first one's result.
        let mut mtc_info: Vec<MtcInfo> = Vec::with_capacity(output.mtc.len());
        let mut by_hint: HashMap<(&str, usize), usize> = HashMap::new();
        // The system's own position of each port taken, since two ports can share a name.
        let mut by_port: HashMap<usize, usize> = HashMap::new();
        for mtc in &output.mtc {
            let hint = mtc.port_name.as_str();
            if let Some(&first) = by_hint.get(&(hint, mtc.port_index)) {
                mtc_info.push(mtc_info[first].clone());
                continue;
            }
            let here = mtc_info.len();
            by_hint.insert((hint, mtc.port_index), here);
            let result = match connect_midi(hint, mtc.port_index) {
                Ok((at, _, _)) if by_port.contains_key(&at) => mtc_info[by_port[&at]].clone(),
                Ok((at, name, connection)) => {
                    by_port.insert(at, here);
                    let scheduler = MtcScheduler::new(clock, rate, start);
                    let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
                    let spawned = thread::Builder::new()
                        .name("flux-mtc".into())
                        .spawn(move || run_mtc(scheduler, clock, connection, shared, stop));
                    match spawned {
                        Ok(handle) => {
                            threads.push(handle);
                            MtcInfo {
                                port: name,
                                ok: true,
                            }
                        }
                        Err(err) => {
                            warnings.push(format!("MTC is off on {name}: {err}"));
                            MtcInfo {
                                port: name,
                                ok: false,
                            }
                        }
                    }
                }
                Err(warning) => {
                    warnings.push(warning);
                    MtcInfo {
                        port: hint.to_owned(),
                        ok: false,
                    }
                }
            };
            mtc_info.push(result);
        }

        let info = OutputInfo {
            device: audio.device,
            sample_rate: ltc_config.sample_rate,
            channels: audio.channels,
            ltc: ltc_info,
            mtc: mtc_info,
            metronome: metronome_info,
            warnings,
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
            metronome: click.map(|(_, _, spec)| spec.shared),
            stop,
            threads,
            info,
        })
    }

    /// Follows an edit of the active timeline's tempo or click switch while the show runs.
    /// Does nothing when the project has no metronome output.
    pub fn set_tempo(&self, tempo: Tempo) {
        if let Some(metronome) = &self.metronome {
            metronome.set(tempo);
        }
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    pub fn send(&self, command: Command) -> Result<(), OutputError> {
        self.messages
            .send(Message::Transport(command))
            .map_err(|_| OutputError::NotRunning)
    }

    /// Replaces the audio being played, after the project's clips changed.
    pub fn set_clips(&self, set: ClipSet) -> Result<(), OutputError> {
        let set = Arc::new(set);
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
        for handle in self.threads.drain(..) {
            handle.thread().unpark();
            let _ = handle.join();
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
    device_hint: String,
    needed: u16,
    /// 1-based interface channels of the programme.
    programme: Vec<u16>,
    clips: Arc<ClipSet>,
    /// 1-based channels and their levels. Empty when LTC is off or plays on devices of its own.
    ltc: Vec<(u16, f32)>,
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

struct LtcSpec {
    device_hint: String,
    /// 1-based channels and their levels. Empty when the stream plays only the metronome.
    ltc: Vec<(u16, f32)>,
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
fn same_device(a: &str, b: &str) -> bool {
    let name =
        |hint: &str| devices::open_audio(hint).and_then(|device| devices::device_name(&device));
    a == b || matches!((name(a), name(b)), (Some(a), Some(b)) if a == b)
}

/// One LTC signal on one interface channel, and the project outputs that asked for it.
#[derive(Debug, PartialEq)]
struct LtcSlot {
    /// 1-based.
    channel: u16,
    /// Linear peak level: the first output that asked for this slot sets it.
    level: f32,
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

/// Each slot's 1-based channel and its level.
fn levels_of(slots: &[LtcSlot]) -> Vec<(u16, f32)> {
    slots
        .iter()
        .map(|slot| (slot.channel, slot.level))
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
                outputs: vec![index],
            }),
        }
    }
    plan
}

/// Runs `open` on a thread of its own, which keeps the stream it returns for as long as the output
/// runs, and waits until the stream is open or has failed. A cpal stream is not `Send` on every
/// platform, so it is created, played and dropped on that one thread.
fn spawn_stream<I: Send + 'static>(
    name: &str,
    stop: Arc<AtomicBool>,
    open: impl FnOnce() -> Result<(cpal::Stream, I), OutputError> + Send + 'static,
) -> Result<(JoinHandle<()>, I), OutputError> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let (stream, info) = match open() {
                Ok(opened) => opened,
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(info));
            while !stop.load(Ordering::Relaxed) {
                thread::park_timeout(AUDIO_PARK);
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

/// An output device and the stream format chosen for it.
struct OpenedDevice {
    device: cpal::Device,
    name: String,
    channels: u16,
    format: SampleFormat,
}

/// Opens the device `hint` names with the fewest channels that cover `needed`, at `rate`.
fn open_device(hint: &str, needed: u16, rate: u32) -> Result<OpenedDevice, OutputError> {
    let device = devices::open_audio(hint).ok_or_else(|| {
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

fn open_stream(
    spec: AudioSpec,
    messages: Receiver<Message>,
    shared: Arc<SharedClock>,
) -> Result<(cpal::Stream, AudioInfo), OutputError> {
    let rate = spec.ltc_config.sample_rate;
    let opened = open_device(&spec.device_hint, spec.needed, rate)?;
    let channels = opened.channels;

    let ltc = (!spec.ltc.is_empty()).then(|| LtcChannel {
        stream: LtcStream::new(spec.ltc_config, spec.rate, spec.start, 0),
        channels: spec.ltc.iter().copied().map(LtcTap::new).collect(),
    });
    // Channels beyond what the device opened cannot be written to.
    let programme = spec
        .programme
        .iter()
        .map(|&channel| usize::from(channel) - 1)
        .filter(|&channel| channel < usize::from(channels))
        .collect();
    let renderer = Renderer::new(
        Transport::new(spec.clock, spec.start),
        messages,
        spec.clips,
        programme,
        ltc,
        usize::from(channels),
        Arc::clone(&shared),
    )
    .with_click(spec.click.as_ref().map(|click| click.out(channels)));
    let lost = Arc::clone(&shared);
    let stream = start_stream(&opened, rate, renderer, shared, move |err| {
        eprintln!("flux: audio stream error: {err}");
        lost.lost.store(true, Ordering::Relaxed);
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
) -> Result<(cpal::Stream, String), OutputError> {
    let rate = spec.config.sample_rate;
    let needed = spec
        .ltc
        .iter()
        .map(|&(channel, _)| channel)
        .chain(
            spec.click
                .iter()
                .flat_map(|click| click.channels.iter().copied()),
        )
        .max()
        .unwrap_or(1);
    let opened = open_device(&spec.device_hint, needed, rate)?;
    let follower = LtcFollower::new(
        LtcStream::new(spec.config, spec.rate, spec.start, 0),
        spec.ltc.iter().copied().map(LtcTap::new).collect(),
        usize::from(opened.channels),
        spec.clock,
        Arc::clone(&shared),
    )
    .with_click(spec.click.as_ref().map(|click| click.out(opened.channels)));
    let lost = Arc::clone(&shared);
    let stream = start_stream(&opened, rate, follower, shared, move |err| {
        eprintln!("flux: LTC stream error: {err}");
        lost.ltc_lost.store(true, Ordering::Relaxed);
    })?;
    Ok((stream, opened.name))
}

/// Builds the stream in the format chosen for the device, and starts it.
fn start_stream<F: Fill>(
    opened: &OpenedDevice,
    rate: u32,
    filler: F,
    shared: Arc<SharedClock>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, OutputError> {
    let config = cpal::StreamConfig {
        channels: opened.channels,
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let device = &opened.device;
    let stream = match opened.format {
        SampleFormat::F32 => build::<f32, _>(device, config, filler, shared, on_error),
        SampleFormat::I32 => build::<i32, _>(device, config, filler, shared, on_error),
        SampleFormat::I16 => build::<i16, _>(device, config, filler, shared, on_error),
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
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, OutputError>
where
    T: SizedSample + FromSample<f32>,
    F: Fill,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| filler.fill(data, shared.now()),
            on_error,
            None,
        )
        .map_err(|err| OutputError::Stream(err.to_string()))
}

/// Connects to the MIDI output port a project's name hint refers to, the `index`-th one of that
/// name. Returns the port's position in the system's list, its name and the connection. The error
/// is a message for the user, since a missing port is not fatal.
fn connect_midi(
    hint: &str,
    index: usize,
) -> Result<(usize, String, midir::MidiOutputConnection), String> {
    let output = midir::MidiOutput::new(MIDI_CLIENT_NAME)
        .map_err(|err| format!("MTC is off, MIDI is unavailable: {err}"))?;
    let ports: Vec<(String, midir::MidiOutputPort)> = output
        .ports()
        .into_iter()
        .filter_map(|port| Some((output.port_name(&port).ok()?, port)))
        .collect();
    let (name, port) = devices::find_by_name(hint, &ports, index, |(name, _)| name)
        .ok_or_else(|| format!("MTC is off, MIDI port not found: {hint}"))?;
    let name = name.clone();
    let at = ports
        .iter()
        .position(|(_, other)| other == port)
        .unwrap_or(0);
    output
        .connect(port, "Flux Timecode MTC")
        .map(|connection| (at, name.clone(), connection))
        .map_err(|err| format!("MTC is off, cannot open {name}: {err}"))
}

/// How the MTC thread waits for the next quarter frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MtcWait {
    Park(Duration),
    /// Busy-wait this long: the quarter frame is closer than the OS can promise to wake us.
    Spin(Duration),
}

/// Parks until shortly before the quarter frame is due, then spins the rest, so the message goes
/// out when it falls due and not up to a timer tick later. `fresh` is false when the published
/// position is old enough that waiting for it to reach the deadline could never end.
fn mtc_wait(until_due: Duration, fresh: bool) -> MtcWait {
    if !fresh {
        MtcWait::Park(MTC_TICK)
    } else if until_due <= MTC_SPIN {
        MtcWait::Spin(until_due)
    } else {
        MtcWait::Park((until_due - MTC_SPIN).min(MTC_TICK))
    }
}

/// Sends MTC from the position the audio callback publishes: a full frame whenever playback
/// starts or jumps, and quarter frames as they fall due.
fn run_mtc(
    mut scheduler: MtcScheduler,
    clock: Clock,
    mut connection: midir::MidiOutputConnection,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
) {
    let mut seen_epoch = None;
    while !stop.load(Ordering::Relaxed) {
        let snapshot = shared.load();
        let now = shared.now();
        let position = snapshot.estimated_position(now, clock);
        if seen_epoch != Some(snapshot.epoch) {
            seen_epoch = Some(snapshot.epoch);
            let _ = connection.send(&scheduler.resync(position));
        }
        let mut wait = MtcWait::Park(MTC_TICK);
        if snapshot.state == TransportState::Playing {
            scheduler.poll(position, |message| {
                let _ = connection.send(&message);
            });
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
    use crate::engine::ltc::{LtcConfig, LtcStream};

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

    use crate::audio::Decoded;
    use crate::engine::mixer::ClipPlayer;

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

    /// A follower writing mono LTC on channel 0, and the clock it follows.
    fn follower() -> (LtcFollower, Arc<SharedClock>) {
        let shared = Arc::new(SharedClock::new());
        let follower = LtcFollower::new(
            LtcStream::new(config(), RATE, 0, 0),
            vec![tap(0, 1.0)],
            1,
            clock(),
            Arc::clone(&shared),
        );
        (follower, shared)
    }

    fn master_at(shared: &SharedClock, state: TransportState, epoch: u64, position: u64) {
        shared.publish(Snapshot {
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
        let mut follower = LtcFollower::new(
            LtcStream::new(config(), RATE, 0, 0),
            vec![tap(2, 1.0)],
            4,
            clock(),
            Arc::clone(&shared),
        );
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
        assert_eq!(
            levels_of(&plan.shared),
            vec![(3, 0.25), (4, DEFAULT_LTC_LEVEL)]
        );
    }

    #[test]
    fn follower_should_put_one_ltc_signal_on_every_channel_it_is_given() {
        let shared = Arc::new(SharedClock::new());
        let mut follower = LtcFollower::new(
            LtcStream::new(config(), RATE, 0, 0),
            vec![tap(0, 1.0), tap(2, 1.0)],
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
            outputs: outputs.to_vec(),
        }
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
        assert_eq!(levels_of(&plan.shared), vec![(3, DEFAULT_LTC_LEVEL)]);
    }

    #[test]
    fn plan_ltc_should_be_empty_without_outputs() {
        let plan = plan_ltc(&[], "Main", loosely);
        assert!(plan.shared.is_empty() && plan.own.is_empty());
    }

    #[test]
    fn output_info_should_serialise_the_outputs_as_lists() {
        let info = OutputInfo {
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
            LtcStream::new(config(), RATE, 0, 0),
            if ltc { vec![tap(0, 1.0)] } else { Vec::new() },
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
