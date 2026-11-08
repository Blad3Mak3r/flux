//! Mono microphone capture. The callback only sends stack buffers to a bounded writer queue.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::Serialize;
use std::{
    fs::File,
    io::{self, BufWriter, Seek, SeekFrom, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const BLOCK: usize = 512;
const MAX_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("microphone is unavailable or permission was denied")]
    Device,
    #[error("microphone disconnected or capture overran; repeat the take")]
    Capture,
    #[error("could not store microphone recording")]
    Storage,
    #[error("recording is empty")]
    Empty,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub name: String,
    pub channels: Vec<u16>,
}

fn host(driver: Option<&str>) -> Result<cpal::Host, Error> {
    match driver {
        None => Ok(cpal::default_host()),
        Some(name) => cpal::available_hosts()
            .into_iter()
            .find(|h| h.name() == name)
            .and_then(|h| cpal::host_from_id(h).ok())
            .ok_or(Error::Device),
    }
}

pub fn inputs(driver: Option<&str>) -> Result<Vec<Input>, Error> {
    Ok(host(driver)?
        .input_devices()
        .map_err(|_| Error::Device)?
        .filter_map(|d| {
            let name = d.description().ok()?.name().to_owned();
            let count = d.default_input_config().ok()?.channels();
            Some(Input {
                name,
                channels: (1..=count).collect(),
            })
        })
        .collect::<Vec<_>>())
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    failed: AtomicU8,
    frames: AtomicU64,
    rate: AtomicU32,
    peak: AtomicU32,
    done: AtomicBool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub duration: String,
    pub peak: f32,
    pub done: bool,
    pub error: Option<&'static str>,
}

pub struct Capture {
    shared: Arc<Shared>,
    writer: Option<JoinHandle<Result<(), Error>>>,
    path: PathBuf,
}

impl Capture {
    pub fn start(
        driver: Option<String>,
        device: String,
        channel: u16,
        path: PathBuf,
    ) -> Result<Self, Error> {
        let shared = Arc::new(Shared::default());
        let state = Arc::clone(&shared);
        let target = path.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let writer = thread::spawn(move || {
            let result = record(driver.as_deref(), &device, channel, &target, &state, &ready);
            if let Err(error) = result {
                state.failed.store(
                    if error == Error::Storage { 2 } else { 1 },
                    Ordering::Release,
                );
                let _ = ready.try_send(Err(error));
            }
            state.done.store(true, Ordering::Release);
            result
        });
        let mut capture = Self {
            shared,
            writer: Some(writer),
            path,
        };
        match started.recv().unwrap_or(Err(Error::Capture)) {
            Ok(()) => Ok(capture),
            Err(error) => {
                capture.cancel();
                Err(error)
            }
        }
    }

    pub fn status(&self) -> Status {
        let rate = self.shared.rate.load(Ordering::Relaxed).max(1);
        Status {
            duration: format!(
                "{:.1}",
                self.shared.frames.load(Ordering::Relaxed) as f64 / f64::from(rate)
            ),
            peak: f32::from_bits(self.shared.peak.swap(0, Ordering::Relaxed)),
            done: self.shared.done.load(Ordering::Acquire),
            error: match self.shared.failed.load(Ordering::Acquire) {
                0 => None,
                2 => Some("storage"),
                _ => Some("capture"),
            },
        }
    }

    pub fn finish(mut self) -> Result<Vec<u8>, Error> {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            writer.join().map_err(|_| Error::Capture)??;
        }
        std::fs::read(&self.path).map_err(|_| Error::Storage)
    }

    pub fn cancel(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Chunk {
    samples: [f32; BLOCK],
    len: usize,
}

fn stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channel: u16,
    tx: SyncSender<Chunk>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    let failed = Arc::clone(&shared);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                enqueue(data, channels, channel, &tx, &shared);
            },
            move |_| {
                failed.failed.store(1, Ordering::Release);
            },
            None,
        )
        .map_err(|_| Error::Device)
}

fn enqueue<T: Sample>(
    data: &[T],
    channels: usize,
    channel: u16,
    tx: &SyncSender<Chunk>,
    shared: &Shared,
) where
    f32: FromSample<T>,
{
    if shared.stop.load(Ordering::Relaxed) || shared.failed.load(Ordering::Relaxed) != 0 {
        return;
    }
    let mut chunk = Chunk {
        samples: [0.0; BLOCK],
        len: 0,
    };
    let mut peak = 0.0_f32;
    for frame in data.chunks_exact(channels) {
        let sample = f32::from_sample(frame[usize::from(channel - 1)]);
        if !sample.is_finite() {
            shared.failed.store(1, Ordering::Release);
            return;
        }
        peak = peak.max(sample.abs().min(1.0));
        chunk.samples[chunk.len] = sample;
        chunk.len += 1;
        if chunk.len == BLOCK {
            if tx.try_send(chunk).is_err() {
                shared.failed.store(1, Ordering::Release);
                return;
            }
            chunk = Chunk {
                samples: [0.0; BLOCK],
                len: 0,
            };
        }
    }
    if chunk.len > 0 && tx.try_send(chunk).is_err() {
        shared.failed.store(1, Ordering::Release);
    }
    shared.peak.fetch_max(peak.to_bits(), Ordering::Relaxed);
}

fn header(out: &mut impl Write, rate: u32, frames: u32) -> io::Result<()> {
    let bytes = frames * 2;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + bytes).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16_u32.to_le_bytes())?;
    out.write_all(&1_u16.to_le_bytes())?;
    out.write_all(&1_u16.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * 2).to_le_bytes())?;
    out.write_all(&2_u16.to_le_bytes())?;
    out.write_all(&16_u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&bytes.to_le_bytes())
}

fn write_chunk(out: &mut impl Write, samples: &[f32]) -> io::Result<()> {
    for sample in samples {
        out.write_all(
            &((*sample).clamp(-1.0, 1.0).mul_add(32767.0, 0.0).round() as i16).to_le_bytes(),
        )?;
    }
    Ok(())
}

fn record(
    driver: Option<&str>,
    name: &str,
    channel: u16,
    path: &std::path::Path,
    shared: &Arc<Shared>,
    ready: &SyncSender<Result<(), Error>>,
) -> Result<(), Error> {
    let host = host(driver)?;
    let device = host
        .input_devices()
        .map_err(|_| Error::Device)?
        .find(|d| d.description().is_ok_and(|d| d.name() == name))
        .ok_or(Error::Device)?;
    let supported = device.default_input_config().map_err(|_| Error::Device)?;
    let rate = supported.sample_rate();
    if channel == 0 || channel > supported.channels() || !(8_000..=192_000).contains(&rate) {
        return Err(Error::Device);
    }
    let mut file = BufWriter::new(File::create(path).map_err(|_| Error::Storage)?);
    header(&mut file, rate, 0).map_err(|_| Error::Storage)?;
    let (tx, rx) = mpsc::sync_channel(64);
    let config = supported.config();
    let state = Arc::clone(shared);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => stream::<f32>(&device, config, channel, tx, state),
        SampleFormat::F64 => stream::<f64>(&device, config, channel, tx, state),
        SampleFormat::I16 => stream::<i16>(&device, config, channel, tx, state),
        SampleFormat::I32 => stream::<i32>(&device, config, channel, tx, state),
        SampleFormat::U16 => stream::<u16>(&device, config, channel, tx, state),
        _ => Err(Error::Device),
    }?;
    shared.rate.store(rate, Ordering::Relaxed);
    stream.play().map_err(|_| Error::Device)?;
    let _ = ready.send(Ok(()));
    let limit = u64::from(rate) * MAX_SECONDS;
    let mut frames = 0_u64;
    loop {
        if shared.failed.load(Ordering::Acquire) != 0 {
            return Err(Error::Capture);
        }
        if shared.stop.load(Ordering::Acquire) {
            break;
        }
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(20)) {
            let len = chunk.len.min((limit - frames) as usize);
            write_chunk(&mut file, &chunk.samples[..len]).map_err(|_| Error::Storage)?;
            frames += len as u64;
            shared.frames.store(frames, Ordering::Relaxed);
            if frames == limit {
                break;
            }
        }
    }
    drop(stream);
    if shared.failed.load(Ordering::Acquire) != 0 {
        return Err(Error::Capture);
    }
    for chunk in rx.try_iter() {
        let len = chunk.len.min((limit - frames) as usize);
        write_chunk(&mut file, &chunk.samples[..len]).map_err(|_| Error::Storage)?;
        frames += len as u64;
    }
    if frames == 0 {
        return Err(Error::Empty);
    }
    shared.frames.store(frames, Ordering::Relaxed);
    file.seek(SeekFrom::Start(0)).map_err(|_| Error::Storage)?;
    header(&mut file, rate, frames as u32).map_err(|_| Error::Storage)?;
    file.flush().map_err(|_| Error::Storage)?;
    file.get_ref().sync_all().map_err(|_| Error::Storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wav_is_mono_pcm_and_clamps_samples() {
        let mut wav = Vec::new();
        header(&mut wav, 48_000, 3).unwrap();
        write_chunk(&mut wav, &[-2.0, 0.0, 2.0]).unwrap();
        assert_eq!(&wav[44..], &[1, 128, 0, 0, 255, 127]);
        let audio = flux_audio::decode_limited(wav, Some("wav"), 48_000).unwrap();
        assert_eq!(audio.frames(), 3);
        assert!(audio.samples[0] < -32_000 && audio.samples[4] > 32_000);
    }

    #[test]
    fn callback_selects_one_channel_and_publishes_its_peak() {
        let (tx, rx) = mpsc::sync_channel(1);
        let shared = Shared::default();
        enqueue(&[0.9_f32, 0.25, -0.8, -0.5], 2, 2, &tx, &shared);
        let chunk = rx.try_recv().unwrap();
        assert_eq!(&chunk.samples[..chunk.len], &[0.25, -0.5]);
        assert_eq!(f32::from_bits(shared.peak.load(Ordering::Relaxed)), 0.5);
    }

    #[test]
    fn overflow_and_invalid_samples_fail_instead_of_silently_losing_audio() {
        let (tx, _rx) = mpsc::sync_channel(1);
        let shared = Shared::default();
        enqueue(&[0.0_f32; BLOCK * 2], 1, 1, &tx, &shared);
        assert_eq!(shared.failed.load(Ordering::Acquire), 1);
        let shared = Shared::default();
        enqueue(&[f32::NAN], 1, 1, &tx, &shared);
        assert_eq!(shared.failed.load(Ordering::Acquire), 1);
    }

    #[test]
    fn cancelled_capture_does_not_enqueue_more_samples() {
        let (tx, rx) = mpsc::sync_channel(1);
        let shared = Shared::default();
        shared.stop.store(true, Ordering::Release);
        enqueue(&[0.5_f32], 1, 1, &tx, &shared);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn captured_wav_resamples_to_engine_rate_without_changing_duration() {
        let mut wav = Vec::new();
        header(&mut wav, 44_100, 4_410).unwrap();
        write_chunk(&mut wav, &[0.25; 4_410]).unwrap();
        let audio = flux_audio::decode_limited(wav, Some("wav"), 48_000).unwrap();
        assert_eq!(audio.frames(), 4_800);
    }
}
