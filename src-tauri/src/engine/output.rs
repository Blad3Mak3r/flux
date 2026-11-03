//! Real-time output: the audio stream that carries LTC and is the master clock, and the MIDI
//! thread that sends MTC from that same clock.
//!
//! What can be tested without hardware is kept apart from the device code: [`Renderer`] fills a
//! buffer for any sample type, [`pick_config`] chooses a stream format, and [`Snapshot`] turns the
//! last published position into a current one. [`Output`] only wires them to cpal and midir.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::{Serialize, Serializer};
use thiserror::Error;

use super::ltc::{LtcConfig, LtcStream};
use super::mtc::MtcScheduler;
use super::transport::{Clock, Command, Transport, TransportState};
use crate::devices;
use crate::project::{FrameRate, Project};

const MIDI_CLIENT_NAME: &str = "Flux Timecode";
/// How often the MTC thread checks whether a quarter frame is due.
const MTC_TICK: Duration = Duration::from_millis(1);
/// The audio thread keeps its stream alive and wakes up this often to see whether to stop.
const AUDIO_PARK: Duration = Duration::from_millis(50);
/// The published position is one buffer old at most; extrapolating further than this would only
/// hide a stalled stream.
const MAX_EXTRAPOLATION: Duration = Duration::from_millis(100);

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("choose an audio output device (for LTC or for the programme) before starting")]
    NoAudioDevice,
    #[error(
        "LTC is on {ltc} but the programme audio is on {audio}: one device per project for now"
    )]
    DifferentDevices { ltc: String, audio: String },
    #[error("audio device not found: {0}")]
    DeviceMissing(String),
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
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for OutputError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
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

    /// Whether the audio device failed or went away while running.
    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

impl Default for SharedClock {
    fn default() -> Self {
        Self::new()
    }
}

/// The LTC generator and the interface channel it goes to.
#[derive(Debug)]
pub struct LtcChannel {
    pub stream: LtcStream,
    /// Zero-based index into each frame of the interleaved buffer.
    pub channel: usize,
}

/// Fills audio buffers from the transport. Owned by the audio callback, so it must not allocate
/// or wait.
#[derive(Debug)]
pub struct Renderer {
    transport: Transport,
    commands: Receiver<Command>,
    ltc: Option<LtcChannel>,
    channels: usize,
    shared: Arc<SharedClock>,
    seen_epoch: u64,
}

impl Renderer {
    pub fn new(
        transport: Transport,
        commands: Receiver<Command>,
        ltc: Option<LtcChannel>,
        channels: usize,
        shared: Arc<SharedClock>,
    ) -> Self {
        Self {
            seen_epoch: transport.epoch(),
            transport,
            commands,
            ltc,
            channels,
            shared,
        }
    }

    /// Fills one interleaved buffer. `now` is [`SharedClock::now`] at the time of the call.
    pub fn render<T: Sample + FromSample<f32>>(&mut self, data: &mut [T], now: u64) {
        while let Ok(command) = self.commands.try_recv() {
            self.transport.apply(command);
        }
        if self.transport.epoch() != self.seen_epoch {
            self.seen_epoch = self.transport.epoch();
            if let Some(ltc) = &mut self.ltc {
                ltc.stream.resync(self.transport.position_samples());
            }
        }

        data.fill(T::EQUILIBRIUM);
        if let (TransportState::Playing, Some(ltc)) = (self.transport.state(), &mut self.ltc) {
            for frame in data.chunks_exact_mut(self.channels) {
                frame[ltc.channel] = T::from_sample(ltc.stream.next_sample());
            }
        }

        self.transport.advance((data.len() / self.channels) as u64);
        self.shared.publish(Snapshot {
            state: self.transport.state(),
            epoch: self.transport.epoch(),
            position: self.transport.position_samples(),
            stamp: now,
        });
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
    /// 1-based interface channel carrying LTC.
    pub ltc_channel: Option<u16>,
    pub mtc_port: Option<String>,
    /// Things that did not stop the output but the user should know, e.g. a missing MIDI port.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub state: TransportState,
    /// Frame count of the timecode being played, including the project's start offset.
    pub frame: u64,
    pub position_samples: u64,
    pub device_lost: bool,
}

/// Reads where the transport is. It does not borrow the [`Output`], so a UI thread can hold one.
#[derive(Debug, Clone)]
pub struct PositionReader {
    shared: Arc<SharedClock>,
    clock: Clock,
    start: u64,
}

impl PositionReader {
    pub fn position(&self) -> Position {
        let snapshot = self.shared.load();
        let position = snapshot.estimated_position(self.shared.now(), self.clock);
        Position {
            state: snapshot.state,
            frame: self.start + self.clock.samples_to_frames(position),
            position_samples: position,
            device_lost: self.shared.is_lost(),
        }
    }
}

/// A running output. Dropping it stops the threads and closes the devices.
pub struct Output {
    commands: Sender<Command>,
    reader: PositionReader,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    info: OutputInfo,
}

impl Output {
    /// Opens the devices a project's `output` section names and starts the stream, stopped at
    /// the timeline start. The audio device is required because its clock drives everything; a
    /// missing MIDI port only produces a warning.
    pub fn start(project: &Project) -> Result<Self, OutputError> {
        let output = &project.output;
        let device_hint = match (&output.ltc, &output.audio) {
            (Some(ltc), Some(audio)) if ltc.device_name != audio.device_name => {
                return Err(OutputError::DifferentDevices {
                    ltc: ltc.device_name.clone(),
                    audio: audio.device_name.clone(),
                });
            }
            (Some(ltc), _) => ltc.device_name.clone(),
            (None, Some(audio)) => audio.device_name.clone(),
            (None, None) => return Err(OutputError::NoAudioDevice),
        };
        let needed = output
            .ltc
            .iter()
            .map(|ltc| ltc.channel)
            .chain(
                output
                    .audio
                    .iter()
                    .flat_map(|audio| audio.channels.iter().copied()),
            )
            .max()
            .unwrap_or(1);

        let ltc_config = LtcConfig::default();
        let rate = project.timecode.frame_rate;
        let start = project.timecode.start;
        let clock = Clock::new(ltc_config.sample_rate, rate);
        let shared = Arc::new(SharedClock::new());
        let stop = Arc::new(AtomicBool::new(false));
        let (commands, command_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        let audio_thread = {
            let spec = AudioSpec {
                device_hint,
                needed,
                ltc_channel: output.ltc.as_ref().map(|ltc| ltc.channel),
                ltc_config,
                rate,
                start,
                clock,
            };
            let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
            thread::Builder::new()
                .name("flux-audio".into())
                .spawn(move || run_audio(spec, command_rx, shared, stop, ready_tx))
                .map_err(|err| OutputError::Stream(err.to_string()))?
        };
        let audio = match ready_rx.recv() {
            Ok(Ok(audio)) => audio,
            Ok(Err(err)) => {
                let _ = audio_thread.join();
                return Err(err);
            }
            Err(_) => {
                let _ = audio_thread.join();
                return Err(OutputError::Stream("the audio thread stopped".into()));
            }
        };

        let mut threads = vec![audio_thread];
        let mut warnings = Vec::new();
        let mut mtc_port = None;
        if let Some(mtc) = &output.mtc {
            match connect_midi(&mtc.port_name) {
                Ok((name, connection)) => {
                    mtc_port = Some(name);
                    let scheduler = MtcScheduler::new(clock, rate, start);
                    let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
                    let spawned = thread::Builder::new()
                        .name("flux-mtc".into())
                        .spawn(move || run_mtc(scheduler, clock, connection, shared, stop));
                    match spawned {
                        Ok(handle) => threads.push(handle),
                        Err(err) => warnings.push(format!("MTC is off: {err}")),
                    }
                }
                Err(warning) => warnings.push(warning),
            }
        }

        let info = OutputInfo {
            device: audio.device,
            sample_rate: ltc_config.sample_rate,
            channels: audio.channels,
            ltc_channel: output.ltc.as_ref().map(|ltc| ltc.channel),
            mtc_port,
            warnings,
        };
        Ok(Self {
            commands,
            reader: PositionReader {
                shared,
                clock,
                start,
            },
            stop,
            threads,
            info,
        })
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    pub fn send(&self, command: Command) -> Result<(), OutputError> {
        self.commands
            .send(command)
            .map_err(|_| OutputError::NotRunning)
    }

    pub fn reader(&self) -> PositionReader {
        self.reader.clone()
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

struct AudioSpec {
    device_hint: String,
    needed: u16,
    /// 1-based.
    ltc_channel: Option<u16>,
    ltc_config: LtcConfig,
    rate: FrameRate,
    start: u64,
    clock: Clock,
}

struct AudioInfo {
    device: String,
    channels: u16,
}

/// Owns the cpal stream for as long as the output runs. A cpal stream is not `Send` on every
/// platform, so it is created, played and dropped on this one thread.
fn run_audio(
    spec: AudioSpec,
    commands: Receiver<Command>,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
    ready: Sender<Result<AudioInfo, OutputError>>,
) {
    let (stream, info) = match open_stream(spec, commands, shared) {
        Ok(opened) => opened,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let _ = ready.send(Ok(info));
    while !stop.load(Ordering::Relaxed) {
        thread::park_timeout(AUDIO_PARK);
    }
    drop(stream);
}

fn open_stream(
    spec: AudioSpec,
    commands: Receiver<Command>,
    shared: Arc<SharedClock>,
) -> Result<(cpal::Stream, AudioInfo), OutputError> {
    let device = devices::open_audio(&spec.device_hint)
        .ok_or_else(|| OutputError::DeviceMissing(spec.device_hint.clone()))?;
    let name = device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| spec.device_hint.clone());
    let rate = spec.ltc_config.sample_rate;

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
        pick_config(&ranges, spec.needed, rate).ok_or_else(|| OutputError::Unsupported {
            device: name.clone(),
            channels: spec.needed,
            rate,
        })?;

    let ltc = spec.ltc_channel.map(|channel| LtcChannel {
        stream: LtcStream::new(spec.ltc_config, spec.rate, spec.start, 0),
        channel: usize::from(channel) - 1,
    });
    let renderer = Renderer::new(
        Transport::new(spec.clock, spec.start),
        commands,
        ltc,
        usize::from(channels),
        Arc::clone(&shared),
    );
    let config = cpal::StreamConfig {
        channels,
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, renderer, shared),
        SampleFormat::I32 => build::<i32>(&device, config, renderer, shared),
        SampleFormat::I16 => build::<i16>(&device, config, renderer, shared),
        other => Err(OutputError::Stream(format!(
            "unsupported sample format {other}"
        ))),
    }?;
    stream
        .play()
        .map_err(|err| OutputError::Stream(err.to_string()))?;
    Ok((
        stream,
        AudioInfo {
            device: name,
            channels,
        },
    ))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut renderer: Renderer,
    shared: Arc<SharedClock>,
) -> Result<cpal::Stream, OutputError>
where
    T: SizedSample + FromSample<f32>,
{
    let callback_clock = Arc::clone(&shared);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| renderer.render(data, callback_clock.now()),
            move |err| {
                eprintln!("flux: audio stream error: {err}");
                shared.lost.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|err| OutputError::Stream(err.to_string()))
}

/// Connects to the MIDI output port a project's name hint refers to. The error is a message for
/// the user, since a missing port is not fatal.
fn connect_midi(hint: &str) -> Result<(String, midir::MidiOutputConnection), String> {
    let output = midir::MidiOutput::new(MIDI_CLIENT_NAME)
        .map_err(|err| format!("MTC is off, MIDI is unavailable: {err}"))?;
    let ports: Vec<(String, midir::MidiOutputPort)> = output
        .ports()
        .into_iter()
        .filter_map(|port| Some((output.port_name(&port).ok()?, port)))
        .collect();
    let (name, port) = devices::find_by_name(hint, &ports, |(name, _)| name)
        .ok_or_else(|| format!("MTC is off, MIDI port not found: {hint}"))?;
    let name = name.clone();
    output
        .connect(port, "Flux Timecode MTC")
        .map(|connection| (name.clone(), connection))
        .map_err(|err| format!("MTC is off, cannot open {name}: {err}"))
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
        let position = snapshot.estimated_position(shared.now(), clock);
        if seen_epoch != Some(snapshot.epoch) {
            seen_epoch = Some(snapshot.epoch);
            let _ = connection.send(&scheduler.resync(position));
        }
        if snapshot.state == TransportState::Playing {
            scheduler.poll(position, |message| {
                let _ = connection.send(&message);
            });
        }
        thread::park_timeout(MTC_TICK);
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

    /// A renderer for `channels` interface channels with LTC on 0-based channel `ltc_channel`.
    fn renderer(
        channels: usize,
        ltc_channel: usize,
    ) -> (Renderer, Sender<Command>, Arc<SharedClock>) {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(SharedClock::new());
        let ltc = LtcChannel {
            stream: LtcStream::new(config(), RATE, 0, 0),
            channel: ltc_channel,
        };
        let renderer = Renderer::new(
            Transport::new(clock(), 0),
            rx,
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
        tx.send(Command::Play).unwrap();
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
        tx.send(Command::Play).unwrap();
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
        tx.send(Command::Play).unwrap();
        let mut buffer = [0.0f32; 4_000];
        renderer.render(&mut buffer, 10);
        tx.send(Command::Pause).unwrap();
        renderer.render(&mut buffer, 20);
        assert!(buffer.iter().all(|&s| s == 0.0));
        assert_eq!(shared.load().position, 4_000);
        assert_eq!(shared.load().state, TransportState::Paused);
    }

    #[test]
    fn render_should_resync_ltc_after_a_seek() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        tx.send(Command::Play).unwrap();
        let mut buffer = [0.0f32; 1_000];
        renderer.render(&mut buffer, 10);
        let epoch = shared.load().epoch;

        // Frame 10 starts at sample 19 200: playing from there must begin LTC at once.
        tx.send(Command::Seek(10)).unwrap();
        renderer.render(&mut buffer, 20);
        let snapshot = shared.load();
        assert_ne!(snapshot.epoch, epoch);
        assert_eq!(snapshot.position, 19_200 + 1_000);
        assert_ne!(buffer[0], 0.0, "LTC should start on the frame boundary");
    }

    #[test]
    fn render_should_convert_to_the_device_sample_format() {
        let (mut renderer, tx, _shared) = renderer(1, 0);
        tx.send(Command::Play).unwrap();
        let mut buffer = [0i16; 4_000];
        renderer.render(&mut buffer, 10);
        let peak = buffer.iter().map(|s| s.unsigned_abs()).max().unwrap();
        // Level 0.5 of full scale.
        assert!((16_000..=16_500).contains(&peak), "peak {peak}");
    }

    #[test]
    fn render_should_drain_every_pending_command() {
        let (mut renderer, tx, shared) = renderer(1, 0);
        tx.send(Command::Play).unwrap();
        tx.send(Command::Seek(10)).unwrap();
        tx.send(Command::Pause).unwrap();
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
}
