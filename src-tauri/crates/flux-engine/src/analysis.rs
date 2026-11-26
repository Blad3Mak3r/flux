//! Listening to the sound for the VJ grid's modulation: how loud its low, mid and high bands are,
//! and how loud it is as a whole, 0 to 1, for an audio source to follow (see
//! `flux_project::vj_modulation`).
//!
//! The sound comes from the grid's own mix, which the output callback copies into a lock-free
//! [`Ring`] ([`tap_grid`]), or from an audio input ([`InputTap`]). A thread of the app drains the
//! ring into an [`Analyzer`] and leaves the bands in [`SharedBands`], which the video frame step
//! reads without a lock. Nothing here allocates, locks or logs in an audio callback.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use flux_project::vj_modulation::Bands;
use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use crate::recording::{self, Error as DeviceError};

/// The samples one analysis looks at, and how many arrive between two: a window of about 43 ms
/// at 48 kHz, read every 11 ms.
const FRAME: usize = 2048;
const HOP: usize = 512;
/// Where the low band ends and the high one begins, in Hz.
const LOW_EDGE: f32 = 250.0;
const HIGH_EDGE: f32 = 4000.0;
/// How slowly the automatic gain forgets a peak, in seconds.
const AGC_SECONDS: f32 = 10.0;
/// The quietest peak the automatic gain scales up from; below it the sound is taken as silence.
const FLOOR: f32 = 0.01;

/// A queue of samples from one thread to another, without locks or allocation after it is made.
/// One thread may push and one may pop; a push that finds it full drops its samples.
#[derive(Debug)]
pub struct Ring {
    data: Box<[AtomicU32]>,
    /// Samples pushed and popped since the start; the difference is what is queued.
    head: AtomicUsize,
    tail: AtomicUsize,
    /// The sample rate of what is pushed, as the producer last said.
    rate: AtomicU32,
}

impl Ring {
    /// A ring holding `capacity` samples, rounded up to a power of two.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(2).next_power_of_two();
        Self {
            data: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            rate: AtomicU32::new(48_000),
        }
    }

    pub fn capacity(&self) -> usize {
        self.data.len()
    }

    pub fn set_rate(&self, rate: u32) {
        self.rate.store(rate, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Queues the samples `from` yields, all or none: `false` and nothing queued when they do not
    /// fit. Only the one producer may call it.
    pub fn push_with(&self, count: usize, mut from: impl FnMut(usize) -> f32) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if self.data.len() - head.wrapping_sub(tail) < count {
            return false;
        }
        let mask = self.data.len() - 1;
        for at in 0..count {
            self.data[head.wrapping_add(at) & mask].store(from(at).to_bits(), Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(count), Ordering::Release);
        true
    }

    /// Queues `samples`; see [`Self::push_with`].
    pub fn push(&self, samples: &[f32]) -> bool {
        self.push_with(samples.len(), |at| samples[at])
    }

    /// Moves what is queued, at most `out.len()` samples, into `out`; how many it moved. Only the
    /// one consumer may call it.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let count = head.wrapping_sub(tail).min(out.len());
        let mask = self.data.len() - 1;
        for (at, slot) in out.iter_mut().enumerate().take(count) {
            *slot = f32::from_bits(self.data[tail.wrapping_add(at) & mask].load(Ordering::Relaxed));
        }
        self.tail.store(tail.wrapping_add(count), Ordering::Release);
        count
    }

    /// Throws away what is queued. Only the consumer may call it.
    pub fn clear(&self) {
        self.tail
            .store(self.head.load(Ordering::Acquire), Ordering::Release);
    }
}

/// The ring the output's callback copies the grid's mix into, made by the control side before the
/// callback can reach it.
static GRID_TAP: OnceLock<Arc<Ring>> = OnceLock::new();
static GRID_TAP_ON: AtomicBool = AtomicBool::new(false);

/// The grid's tap, made on first use. Not for the audio callback, which only reads it.
pub fn grid_tap() -> Arc<Ring> {
    Arc::clone(GRID_TAP.get_or_init(|| Arc::new(Ring::new(1 << 16))))
}

/// Whether the callback copies the grid's mix into the tap: only while it is analysed.
pub fn set_grid_tap(on: bool) {
    GRID_TAP_ON.store(on, Ordering::Relaxed);
}

/// Whether the output's callback is copying the grid's mix into the tap.
pub fn grid_tap_on() -> bool {
    GRID_TAP_ON.load(Ordering::Relaxed)
}

/// Called by the output's callback with the grid's own mix of a chunk: queues it as mono when the
/// tap is on. No allocation, no lock.
pub fn tap_grid(left: &[f32], right: &[f32], rate: u32) {
    if !grid_tap_on() {
        return;
    }
    if let Some(ring) = GRID_TAP.get() {
        let frames = left.len().min(right.len());
        ring.set_rate(rate);
        ring.push_with(frames, |at| (left[at] + right[at]) * 0.5);
    }
}

/// The bands the analysis last read, left for any thread to read without a lock.
#[derive(Debug, Default)]
pub struct SharedBands {
    low: AtomicU32,
    mid: AtomicU32,
    high: AtomicU32,
    level: AtomicU32,
}

impl SharedBands {
    pub fn store(&self, bands: Bands) {
        let put = |slot: &AtomicU32, value: f32| slot.store(value.to_bits(), Ordering::Relaxed);
        put(&self.low, bands.low);
        put(&self.mid, bands.mid);
        put(&self.high, bands.high);
        put(&self.level, bands.level);
    }

    pub fn load(&self) -> Bands {
        let get = |slot: &AtomicU32| f32::from_bits(slot.load(Ordering::Relaxed));
        Bands {
            low: get(&self.low),
            mid: get(&self.mid),
            high: get(&self.high),
            level: get(&self.level),
        }
    }
}

/// Reads the bands of a stream of mono samples. Each band is scaled by a slow peak follower of its
/// own, so typical music sits near 0 to 1 whatever its loudness, and silence reads 0.
pub struct Analyzer {
    rate: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// The last [`FRAME`] samples, the oldest at `write`.
    samples: Vec<f32>,
    write: usize,
    since: usize,
    input: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    /// The bins the bands start at: low from 1, mid from `edges.0`, high from `edges.1`.
    edges: (usize, usize),
    peaks: [f32; 4],
    decay: f32,
    bands: Bands,
}

impl Analyzer {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(8_000) as f32;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FRAME);
        let bin = |hz: f32| ((hz * FRAME as f32 / rate).round() as usize).clamp(1, FRAME / 2);
        Self {
            rate,
            window: (0..FRAME)
                .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / FRAME as f32).cos())
                .collect(),
            samples: vec![0.0; FRAME],
            write: 0,
            since: 0,
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            edges: (bin(LOW_EDGE), bin(HIGH_EDGE)),
            peaks: [FLOOR; 4],
            decay: (-(HOP as f32) / rate / AGC_SECONDS).exp(),
            bands: Bands::default(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    /// The bands as of the last hop.
    pub fn bands(&self) -> Bands {
        self.bands
    }

    /// Takes the next samples; whether the bands were read again.
    pub fn push(&mut self, block: &[f32]) -> bool {
        let mut read = false;
        for &sample in block {
            self.samples[self.write] = if sample.is_finite() { sample } else { 0.0 };
            self.write = (self.write + 1) % FRAME;
            self.since += 1;
            if self.since >= HOP {
                self.since = 0;
                self.read();
                read = true;
            }
        }
        read
    }

    fn read(&mut self) {
        let mut square = 0.0;
        for (at, slot) in self.input.iter_mut().enumerate() {
            let sample = self.samples[(self.write + at) % FRAME];
            square += sample * sample;
            *slot = sample * self.window[at];
        }
        let rms = (square / FRAME as f32).sqrt();
        let spectrum_ok = self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_ok();
        let amplitude = |from: usize, to: usize, spectrum: &[Complex<f32>]| {
            let power: f32 = spectrum[from..to].iter().map(|c| c.norm_sqr()).sum();
            // A sine of amplitude A reads about A (the Hann window's gain is one half, and the
            // transform's N / 2).
            power.sqrt() / (FRAME as f32 / 4.0)
        };
        let (mid, high) = self.edges;
        let measured = if spectrum_ok {
            [
                amplitude(1, mid, &self.spectrum),
                amplitude(mid, high, &self.spectrum),
                amplitude(high, FRAME / 2 + 1, &self.spectrum),
                rms,
            ]
        } else {
            [0.0; 4]
        };
        let mut out = [0.0; 4];
        for (at, value) in measured.into_iter().enumerate() {
            self.peaks[at] = value.max(self.peaks[at] * self.decay).max(FLOOR);
            out[at] = (value / self.peaks[at]).clamp(0.0, 1.0);
        }
        self.bands = Bands {
            low: out[0],
            mid: out[1],
            high: out[2],
            level: out[3],
        };
    }
}

/// An audio input that is open for analysis: a thread holds the stream (it cannot move between
/// threads on every system) and its callback pushes one channel into a ring. Dropping it closes
/// the device.
pub struct InputTap {
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl InputTap {
    /// Opens channel `channel` (from 1) of the input `device` of the driver `driver` (the system's
    /// when `None`) and queues what it hears in `ring`.
    pub fn open(
        driver: Option<String>,
        device: String,
        channel: u16,
        ring: Arc<Ring>,
    ) -> Result<Self, DeviceError> {
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let (ready, started) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("flux-analysis-in".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let failed = Arc::clone(&failed);
                move || {
                    let stream = match listen(driver.as_deref(), &device, channel, ring, &failed) {
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
        let tap = Self {
            stop,
            failed,
            thread: Some(thread),
        };
        started.recv().unwrap_or(Err(DeviceError::Device))?;
        Ok(tap)
    }

    /// Whether the device stopped delivering audio, as when it is unplugged.
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
}

impl Drop for InputTap {
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
    ring: Arc<Ring>,
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
    ring.set_rate(config.sample_rate);
    let failed = Arc::clone(failed);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, config, channel, ring, failed),
        SampleFormat::F64 => stream::<f64>(&device, config, channel, ring, failed),
        SampleFormat::I16 => stream::<i16>(&device, config, channel, ring, failed),
        SampleFormat::I32 => stream::<i32>(&device, config, channel, ring, failed),
        SampleFormat::U16 => stream::<u16>(&device, config, channel, ring, failed),
        _ => Err(DeviceError::Device),
    }?;
    stream.play().map_err(|_| DeviceError::Device)?;
    Ok(stream)
}

fn stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channel: u16,
    ring: Arc<Ring>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, DeviceError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let pick = usize::from(channel - 1);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let frames = data.len() / channels;
                ring.push_with(frames, |at| f32::from_sample(data[at * channels + pick]));
            },
            move |_| failed.store(true, Ordering::Release),
            None,
        )
        .map_err(|_| DeviceError::Device)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        (0..(RATE as f32 * seconds) as usize)
            .map(|n| amplitude * (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin())
            .collect()
    }

    fn analyse(samples: &[f32]) -> Bands {
        let mut analyzer = Analyzer::new(RATE);
        analyzer.push(samples);
        analyzer.bands()
    }

    #[test]
    fn a_low_sine_should_read_mostly_in_the_low_band() {
        let bands = analyse(&sine(100.0, 0.5, 1.0));
        assert!(bands.low > 0.9, "{bands:?}");
        assert!(bands.mid < 0.2 && bands.high < 0.2, "{bands:?}");
        assert!(bands.level > 0.9, "{bands:?}");
    }

    #[test]
    fn a_mid_sine_should_read_mostly_in_the_mid_band() {
        let bands = analyse(&sine(1000.0, 0.5, 1.0));
        assert!(bands.mid > 0.9, "{bands:?}");
        assert!(bands.low < 0.2 && bands.high < 0.2, "{bands:?}");
    }

    #[test]
    fn a_high_sine_should_read_mostly_in_the_high_band() {
        let bands = analyse(&sine(8000.0, 0.5, 1.0));
        assert!(bands.high > 0.9, "{bands:?}");
        assert!(bands.low < 0.2 && bands.mid < 0.2, "{bands:?}");
    }

    #[test]
    fn silence_should_read_nothing_and_a_sound_that_stops_should_fall_away() {
        assert_eq!(analyse(&vec![0.0; 24_000]), Bands::default());
        let mut analyzer = Analyzer::new(RATE);
        analyzer.push(&sine(100.0, 0.5, 0.5));
        assert!(analyzer.bands().low > 0.9);
        analyzer.push(&vec![0.0; RATE as usize / 4]);
        assert_eq!(analyzer.bands().low, 0.0);
    }

    #[test]
    fn the_automatic_gain_should_bring_a_quiet_sound_up_to_the_same_reading() {
        let quiet = analyse(&sine(100.0, 0.05, 1.0));
        let loud = analyse(&sine(100.0, 0.8, 1.0));
        assert!(quiet.low > 0.9 && loud.low > 0.9, "{quiet:?} {loud:?}");
        // A sound below the floor is silence, not boosted noise.
        assert!(analyse(&sine(100.0, 0.001, 1.0)).low < 0.2);
    }

    #[test]
    fn a_bad_sample_should_not_poison_the_bands() {
        let mut samples = sine(1000.0, 0.5, 0.5);
        samples[100] = f32::NAN;
        samples[200] = f32::INFINITY;
        let bands = analyse(&samples);
        assert!(bands.mid.is_finite() && bands.mid > 0.5, "{bands:?}");
    }

    #[test]
    fn a_ring_should_hand_samples_over_in_order_and_never_grow() {
        let ring = Ring::new(8);
        assert_eq!(ring.capacity(), 8);
        let mut out = [0.0; 8];
        assert_eq!(ring.pop(&mut out), 0);
        assert!(ring.push(&[1.0, 2.0, 3.0]));
        assert_eq!(ring.pop(&mut out[..2]), 2);
        assert_eq!(out[..2], [1.0, 2.0]);
        // It wraps round the end, and holds exactly its capacity.
        assert!(ring.push(&[4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]));
        assert!(!ring.push(&[0.0]), "full: the push is dropped, whole");
        assert_eq!(ring.pop(&mut out), 8);
        assert_eq!(out, [3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
        assert_eq!(ring.capacity(), 8);
        // A push that does not fit leaves nothing of itself behind.
        assert!(ring.push(&[1.0; 6]));
        assert!(!ring.push(&[2.0; 3]));
        assert_eq!(ring.pop(&mut out), 6);
        assert!(out[..6].iter().all(|v| *v == 1.0));
    }

    #[test]
    fn a_ring_should_carry_samples_between_threads() {
        let ring = Arc::new(Ring::new(1 << 10));
        let producer = Arc::clone(&ring);
        let thread = std::thread::spawn(move || {
            let mut next = 0u32;
            while next < 20_000 {
                if producer.push(&[next as f32]) {
                    next += 1;
                }
            }
        });
        let (mut expected, mut out) = (0u32, [0.0f32; 64]);
        while expected < 20_000 {
            let count = ring.pop(&mut out);
            for sample in &out[..count] {
                assert_eq!(*sample, expected as f32);
                expected += 1;
            }
        }
        thread.join().unwrap();
    }

    #[test]
    fn the_grid_tap_should_queue_the_mono_mix_only_while_on() {
        let ring = grid_tap();
        ring.clear();
        set_grid_tap(false);
        tap_grid(&[1.0; 4], &[1.0; 4], RATE);
        let mut out = [0.0; 8];
        assert_eq!(ring.pop(&mut out), 0);
        set_grid_tap(true);
        tap_grid(&[1.0, 0.0], &[0.0, 1.0], RATE);
        set_grid_tap(false);
        assert_eq!(ring.pop(&mut out), 2);
        assert_eq!(out[..2], [0.5, 0.5]);
        assert_eq!(ring.rate(), RATE);
    }

    #[test]
    fn shared_bands_should_carry_the_reading() {
        let shared = SharedBands::default();
        let bands = Bands {
            low: 0.1,
            mid: 0.2,
            high: 0.3,
            level: 0.4,
        };
        shared.store(bands);
        assert_eq!(shared.load(), bands);
    }
}
