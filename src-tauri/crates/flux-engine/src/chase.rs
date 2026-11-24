//! Reading an external timecode: MTC from a MIDI input and LTC from an audio input. Flux does not
//! chase it with its own transport; the VJ screen shows where the show that sends it is.
//!
//! The decoders are pure and tested against Flux's own encoders. The readers open the port or the
//! device and keep the latest reading in atomics. Nothing here logs: the MIDI and audio callbacks
//! run on the drivers' threads.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use flux_core::Timecode;
use flux_project::FrameRate;
use midir::{Ignore, MidiInput, MidiInputConnection};
use serde::Serialize;

use crate::control_in::ListenError;
use crate::devices::find_by_name;
use crate::ltc::BITS_PER_FRAME;
use crate::recording::{self, Error as DeviceError};

const RATES: [FrameRate; 4] = [
    FrameRate::Fps24,
    FrameRate::Fps25,
    FrameRate::Fps2997Drop,
    FrameRate::Fps30,
];

/// The rate an MTC rate code stands for.
fn rate_of(code: u8) -> FrameRate {
    RATES[usize::from(code & 3)]
}

/// `timecode` plus `frames`, as a frame count since midnight, wrapping after 24 hours. `None` when
/// the timecode does not exist at `rate`.
fn frame_after(timecode: Timecode, rate: FrameRate, frames: u64) -> Option<u64> {
    let at = timecode.to_frames(rate).ok()?;
    Some((at + frames) % rate.frames_per_day())
}

/// Assembles MTC: eight quarter frames, or a full-frame SysEx.
#[derive(Debug, Clone, Default)]
pub struct MtcDecoder {
    pieces: [u8; 8],
    /// Which pieces arrived since piece 0, one bit each.
    have: u8,
}

impl MtcDecoder {
    /// Takes one MIDI message and answers with the frame it places the sender at (counted since
    /// midnight) and the rate, when it completes one.
    pub fn feed(&mut self, bytes: &[u8]) -> Option<(u64, FrameRate)> {
        match bytes {
            [0xF1, data] => {
                let piece = data >> 4 & 7;
                if piece == 0 {
                    self.have = 0;
                }
                self.pieces[usize::from(piece)] = data & 0xF;
                self.have |= 1 << piece;
                if piece != 7 || self.have != 0xFF {
                    return None;
                }
                let p = self.pieces;
                let rate = rate_of(p[7] >> 1);
                let timecode = Timecode {
                    frames: p[1] << 4 | p[0],
                    seconds: p[3] << 4 | p[2],
                    minutes: p[5] << 4 | p[4],
                    hours: (p[7] & 1) << 4 | p[6],
                };
                // The group described the frame at which piece 0 went out, two frames ago.
                Some((frame_after(timecode, rate, 2)?, rate))
            }
            [0xF0, 0x7F, _, 0x01, 0x01, hh, mm, ss, ff, 0xF7] => {
                self.have = 0;
                let rate = rate_of(hh >> 5);
                let timecode = Timecode {
                    hours: hh & 0x1F,
                    minutes: *mm,
                    seconds: *ss,
                    frames: *ff,
                };
                Some((frame_after(timecode, rate, 0)?, rate))
            }
            _ => None,
        }
    }
}

/// Sync word as it sits in bits 64-79 of a word read in transmission order (bit 0 first).
const SYNC: u128 = {
    let mut sync = 0u128;
    let mut i = 0;
    while i < 16 {
        if 0x3FFD & (0x8000 >> i) != 0 {
            sync |= 1 << i;
        }
        i += 1;
    }
    sync
};
/// A level change smaller than this is noise, not an edge.
const HYSTERESIS: f32 = 0.01;

/// Reads LTC from audio: finds the edges of the biphase-mark signal, turns them into bits and
/// decodes a word each time the sync word goes by. It keeps no buffers, so it runs in the audio
/// callback.
#[derive(Debug, Clone)]
pub struct LtcDecoder {
    sample_rate: u32,
    high: bool,
    /// Samples since the last edge, and since the end of the last word.
    since_edge: u32,
    since_word: u32,
    /// The length of a bit cell, in samples, as the signal runs.
    cell: f32,
    shortest: f32,
    longest: f32,
    /// A half cell arrived and waits for its second half.
    half: bool,
    /// The last 80 bits, the oldest in bit 0.
    word: u128,
    /// Bits since the last word, so a word is only read once it is whole.
    bits: u32,
}

impl LtcDecoder {
    pub fn new(sample_rate: u32) -> Self {
        let cell_at = |fps: f32| sample_rate as f32 / (BITS_PER_FRAME as f32 * fps);
        Self {
            sample_rate,
            high: false,
            since_edge: 0,
            since_word: 0,
            cell: cell_at(27.0),
            shortest: cell_at(32.0),
            longest: cell_at(22.0),
            half: false,
            word: 0,
            bits: 0,
        }
    }

    /// Takes the next sample and answers with the timecode of the word that ends with it, and its
    /// rate, when one does.
    pub fn push(&mut self, sample: f32) -> Option<(Timecode, FrameRate)> {
        self.since_edge = self.since_edge.saturating_add(1);
        self.since_word = self.since_word.saturating_add(1);
        let edge = if self.high {
            sample < -HYSTERESIS
        } else {
            sample > HYSTERESIS
        };
        if !edge {
            return None;
        }
        self.high = !self.high;
        let gap = self.since_edge as f32;
        self.since_edge = 0;
        if gap > 1.5 * self.longest {
            // Silence or noise: start over.
            self.half = false;
            self.bits = 0;
            return None;
        }
        if gap > 0.75 * self.cell {
            self.half = false;
            self.cell = (0.9 * self.cell + 0.1 * gap).clamp(self.shortest, self.longest);
            self.bit(false)
        } else if self.half {
            self.half = false;
            self.bit(true)
        } else {
            self.half = true;
            None
        }
    }

    fn bit(&mut self, bit: bool) -> Option<(Timecode, FrameRate)> {
        self.word = self.word >> 1 | u128::from(bit) << (BITS_PER_FRAME - 1);
        self.bits = self.bits.saturating_add(1);
        if self.bits < BITS_PER_FRAME as u32 || self.word >> 64 != SYNC {
            return None;
        }
        self.bits = 0;
        let length = std::mem::take(&mut self.since_word);
        let field = |at: u32, width: u32| (self.word >> at) as u8 & ((1 << width) - 1);
        let timecode = Timecode {
            frames: field(8, 2) * 10 + field(0, 4),
            seconds: field(24, 3) * 10 + field(16, 4),
            minutes: field(40, 3) * 10 + field(32, 4),
            hours: field(56, 2) * 10 + field(48, 4),
        };
        let rate = if field(10, 1) == 1 {
            FrameRate::Fps2997Drop
        } else {
            self.rate(length)
        };
        timecode.to_frames(rate).ok().map(|_| (timecode, rate))
    }

    /// The rate the signal runs at, from the length of the last word (or the cell when it is the
    /// first one); 29.97 without drop-frame counts as 30.
    fn rate(&self, length: u32) -> FrameRate {
        let samples = if (self.shortest * 80.0..self.longest * 80.0).contains(&(length as f32)) {
            length as f32
        } else {
            self.cell * BITS_PER_FRAME as f32
        };
        let fps = self.sample_rate as f32 / samples;
        [FrameRate::Fps24, FrameRate::Fps25, FrameRate::Fps30]
            .into_iter()
            .min_by(|a, b| {
                let off = |rate: &FrameRate| (f32::from(rate.nominal_fps()) - fps).abs();
                off(a).total_cmp(&off(b))
            })
            .unwrap_or(FrameRate::Fps25)
    }
}

/// How long without a new frame before the signal counts as stopped.
const STALE: Duration = Duration::from_millis(250);

/// The latest frame a reader saw, shared between its callback and whoever asks.
#[derive(Debug)]
pub struct Latest {
    epoch: Instant,
    frame: AtomicU64,
    /// 0 before the first reading, else the rate's place in [`RATES`] plus 1.
    rate: AtomicU8,
    /// Microseconds from `epoch` to the reading.
    at: AtomicU64,
    /// Whether the reading followed the one before it, a frame or two on: the sender runs.
    moving: AtomicBool,
}

impl Default for Latest {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            frame: AtomicU64::new(0),
            rate: AtomicU8::new(0),
            at: AtomicU64::new(0),
            moving: AtomicBool::new(false),
        }
    }
}

/// What a reader saw last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    /// Frames since midnight, at `rate`.
    pub frame: u64,
    pub rate: FrameRate,
    /// Whether the timecode is running, rather than parked or gone.
    pub running: bool,
}

impl Latest {
    pub fn store(&self, frame: u64, rate: FrameRate) {
        let before = self.frame.swap(frame, Ordering::Relaxed);
        let code = RATES.iter().position(|r| *r == rate).unwrap_or(0) as u8 + 1;
        let same_rate = self.rate.swap(code, Ordering::Relaxed) == code;
        self.moving.store(
            same_rate && frame > before && frame - before <= 3,
            Ordering::Relaxed,
        );
        let micros = self.epoch.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        self.at.store(micros, Ordering::Release);
    }

    /// The frame the sender is at now: the last reading, carried on by the time since while the
    /// timecode runs, so the display does not step two frames at a time on MTC.
    pub fn read(&self) -> Option<Reading> {
        let code = self.rate.load(Ordering::Relaxed);
        let rate = *RATES.get(usize::from(code).checked_sub(1)?)?;
        let at = Duration::from_micros(self.at.load(Ordering::Acquire));
        let since = self.epoch.elapsed().saturating_sub(at);
        let running = self.moving.load(Ordering::Relaxed) && since < STALE;
        let mut frame = self.frame.load(Ordering::Relaxed);
        if running {
            let (num, den) = rate.fraction();
            let ahead = since.as_micros() as u64 * num / (den * 1_000_000);
            frame = (frame + ahead) % rate.frames_per_day();
        }
        Some(Reading {
            frame,
            rate,
            running,
        })
    }
}

const MIDI_CLIENT_NAME: &str = "FLUX timecode in";

/// An MTC input that is open. Dropping it closes the port.
pub struct MtcReader {
    _connection: MidiInputConnection<MtcDecoder>,
}

impl MtcReader {
    /// Opens the port a stored name hint refers to and keeps what it reads in `latest`.
    pub fn open(hint: &str, latest: Arc<Latest>) -> Result<Self, ListenError> {
        let mut client = MidiInput::new(MIDI_CLIENT_NAME)
            .map_err(|err| ListenError::Unavailable(err.to_string()))?;
        // Quarter frames are time messages and the full frame is a SysEx: keep both.
        client.ignore(Ignore::ActiveSense);
        let ports: Vec<_> = client
            .ports()
            .into_iter()
            .filter_map(|port| Some((client.port_name(&port).ok()?, port)))
            .collect();
        let (_, port) = find_by_name(hint, &ports, 0, |(name, _)| name)
            .cloned()
            .ok_or(ListenError::Missing)?;
        let connection = client
            .connect(
                &port,
                "Flux timecode in",
                move |_, bytes, decoder| {
                    if let Some((frame, rate)) = decoder.feed(bytes) {
                        latest.store(frame, rate);
                    }
                },
                MtcDecoder::default(),
            )
            .map_err(|err| ListenError::Unavailable(err.to_string()))?;
        Ok(Self {
            _connection: connection,
        })
    }
}

/// An LTC input that is open: a thread holds the audio stream, as it cannot move between
/// threads on every system. Dropping it closes the device.
pub struct LtcReader {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LtcReader {
    /// Opens channel `channel` (from 1) of the input `device` of the driver `driver` (the
    /// system's when `None`) and keeps what it reads in `latest`.
    pub fn open(
        driver: Option<String>,
        device: String,
        channel: u16,
        latest: Arc<Latest>,
    ) -> Result<Self, DeviceError> {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("flux-ltc-in".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let failed = Arc::clone(&failed);
                move || {
                    let stream = match listen(driver.as_deref(), &device, channel, latest, &failed)
                    {
                        Ok(stream) => stream,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    };
                    let _ = ready.send(Ok(()));
                    while !stop.load(Ordering::Acquire) {
                        thread::park_timeout(Duration::from_millis(100));
                    }
                    drop(stream);
                }
            })
            .map_err(|_| DeviceError::Device)?;
        let reader = Self {
            stop,
            failed,
            thread: Some(thread),
        };
        started.recv().unwrap_or(Err(DeviceError::Device))?;
        Ok(reader)
    }

    /// Whether the device stopped delivering audio, as when it is unplugged.
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

impl Drop for LtcReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn listen(
    driver: Option<&str>,
    name: &str,
    channel: u16,
    latest: Arc<Latest>,
    failed: &Arc<AtomicBool>,
) -> Result<cpal::Stream, DeviceError> {
    let device = recording::host(driver)?
        .input_devices()
        .map_err(|_| DeviceError::Device)?
        .find(|d| d.description().is_ok_and(|d| d.name() == name))
        .ok_or(DeviceError::Device)?;
    let supported = device
        .default_input_config()
        .map_err(|_| DeviceError::Device)?;
    if channel == 0 || channel > supported.channels() {
        return Err(DeviceError::Device);
    }
    let config = supported.config();
    let failed = Arc::clone(failed);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, config, channel, latest, failed),
        SampleFormat::F64 => stream::<f64>(&device, config, channel, latest, failed),
        SampleFormat::I16 => stream::<i16>(&device, config, channel, latest, failed),
        SampleFormat::I32 => stream::<i32>(&device, config, channel, latest, failed),
        SampleFormat::U16 => stream::<u16>(&device, config, channel, latest, failed),
        _ => Err(DeviceError::Device),
    }?;
    stream.play().map_err(|_| DeviceError::Device)?;
    Ok(stream)
}

fn stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channel: u16,
    latest: Arc<Latest>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, DeviceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let pick = usize::from(channel - 1);
    let mut decoder = LtcDecoder::new(config.sample_rate);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                for frame in data.chunks_exact(channels) {
                    if let Some((timecode, rate)) = decoder.push(f32::from_sample(frame[pick])) {
                        // The word ends where the next frame starts.
                        if let Some(frame) = frame_after(timecode, rate, 1) {
                            latest.store(frame, rate);
                        }
                    }
                }
            },
            move |_| failed.store(true, Ordering::Release),
            None,
        )
        .map_err(|_| DeviceError::Device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ltc::{LtcConfig, LtcStream};
    use crate::mtc::{full_frame, quarter_frames};

    fn tc(hours: u8, minutes: u8, seconds: u8, frames: u8) -> Timecode {
        Timecode {
            hours,
            minutes,
            seconds,
            frames,
        }
    }

    #[test]
    fn mtc_quarter_frames_place_the_sender_two_frames_on() {
        for rate in RATES {
            let at = tc(13, 59, 58, 20);
            let mut decoder = MtcDecoder::default();
            let mut read = None;
            for message in quarter_frames(at, rate) {
                read = decoder.feed(&message);
            }
            assert_eq!(read, Some((at.to_frames(rate).unwrap() + 2, rate)));
        }
    }

    #[test]
    fn mtc_needs_a_whole_group() {
        let mut decoder = MtcDecoder::default();
        let pieces = quarter_frames(tc(1, 0, 0, 0), FrameRate::Fps25);
        // Joined halfway through a group: nothing until the next one is whole.
        for message in &pieces[4..] {
            assert_eq!(decoder.feed(message), None);
        }
        let next = quarter_frames(tc(1, 0, 0, 2), FrameRate::Fps25);
        let reads: Vec<_> = next.iter().filter_map(|m| decoder.feed(m)).collect();
        assert_eq!(reads.len(), 1);
    }

    #[test]
    fn mtc_full_frame_places_the_sender_exactly() {
        let at = tc(23, 1, 2, 3);
        let mut decoder = MtcDecoder::default();
        assert_eq!(
            decoder.feed(&full_frame(at, FrameRate::Fps30)),
            Some((at.to_frames(FrameRate::Fps30).unwrap(), FrameRate::Fps30))
        );
        assert_eq!(decoder.feed(&[0x90, 60, 100]), None);
    }

    /// Plays `frames` frames of LTC from `start` and reads them back.
    fn ltc_round_trip(rate: FrameRate, sample_rate: u32, level: f32) -> Vec<(u64, FrameRate)> {
        let start = tc(10, 0, 59, 0).to_frames(rate).unwrap();
        let config = LtcConfig {
            sample_rate,
            level,
            edge_samples: 4,
        };
        let mut stream = LtcStream::new(config, rate, start, 0x1234_5678);
        stream.resync(0);
        let mut samples = vec![0.0; sample_rate as usize];
        stream.fill(&mut samples);
        let mut decoder = LtcDecoder::new(sample_rate);
        samples
            .into_iter()
            .filter_map(|s| decoder.push(s))
            .map(|(timecode, rate)| (timecode.to_frames(rate).unwrap() - start, rate))
            .collect()
    }

    #[test]
    fn ltc_reads_every_frame_at_every_rate() {
        for rate in RATES {
            for sample_rate in [44_100, 48_000, 96_000] {
                let reads = ltc_round_trip(rate, sample_rate, 0.5);
                let fps = usize::from(rate.nominal_fps());
                // The first word is read whole; a second of signal holds one word per frame.
                assert!(
                    reads.len() >= fps - 2,
                    "{rate:?} at {sample_rate}: {reads:?}"
                );
                for (n, (frame, read_rate)) in reads.iter().enumerate() {
                    assert_eq!(*frame, n as u64, "{rate:?} at {sample_rate}");
                    assert_eq!(*read_rate, rate, "{rate:?} at {sample_rate}");
                }
            }
        }
    }

    #[test]
    fn ltc_reads_a_quiet_inverted_signal() {
        let start = tc(0, 0, 0, 0).to_frames(FrameRate::Fps25).unwrap();
        let mut stream = LtcStream::new(LtcConfig::default(), FrameRate::Fps25, start, 0);
        stream.resync(0);
        let mut samples = vec![0.0; 48_000];
        stream.fill(&mut samples);
        let mut decoder = LtcDecoder::new(48_000);
        let reads = samples
            .into_iter()
            .filter_map(|s| decoder.push(-s * 0.1))
            .count();
        assert!(reads >= 23);
    }

    #[test]
    fn latest_runs_on_between_readings_and_stops_when_they_do() {
        let latest = Latest::default();
        assert_eq!(latest.read(), None);
        latest.store(100, FrameRate::Fps25);
        assert!(!latest.read().unwrap().running);
        latest.store(102, FrameRate::Fps25);
        let now = latest.read().unwrap();
        assert!(now.running && now.frame >= 102);
        // A jump is a locate, not a run.
        latest.store(5_000, FrameRate::Fps25);
        assert_eq!(
            latest.read(),
            Some(Reading {
                frame: 5_000,
                rate: FrameRate::Fps25,
                running: false
            })
        );
    }
}
