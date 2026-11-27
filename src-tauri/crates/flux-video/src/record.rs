//! Recording the composition to a HAP QuickTime file. The caller hands over RGBA frames, each with
//! the number of its tick on a constant frame rate, and the sound as 16-bit PCM; a thread of its
//! own compresses and writes them, so the caller's loop never waits for the encoder. A frame that
//! did not arrive (the encoder fell behind, or the caller had none) leaves a gap that is filled by
//! repeating the frame before, so the file keeps its rate and its length, and the gap is counted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use ffmpeg_next::format::context::Output;
use ffmpeg_next::format::{self, Pixel};
use ffmpeg_next::{Packet, Rational, frame};

use crate::decode::VideoError;
use crate::hap::{pcm_stream, video_encoder, write_packets};

/// Frames queued for the encoder at most; a frame offered past it is dropped.
const QUEUE: usize = 4;

/// What a recording is made of.
#[derive(Debug, Clone)]
pub struct Config {
    pub path: PathBuf,
    /// The size of the frames handed over. The file is that size rounded up to a multiple of 4,
    /// which HAP's textures need ([`encoded_size`]); the extra pixels are black.
    pub size: (u32, u32),
    /// Frames a second, as numerator and denominator.
    pub rate: (u32, u32),
    /// Sound in the file: its rate and channel count (the samples come interleaved).
    pub audio: Option<(u32, u16)>,
}

/// The size of the picture in the file for frames of `size`: each side up to a multiple of 4.
pub fn encoded_size(size: (u32, u32)) -> (u32, u32) {
    (size.0.max(1).div_ceil(4) * 4, size.1.max(1).div_ceil(4) * 4)
}

/// What the encoder thread counts, for anyone to read.
#[derive(Debug, Default)]
pub struct Counters {
    /// Frames written to the file, repeats included.
    pub written: AtomicU64,
    /// Frames that were not delivered in time, and so are repeats of the one before.
    pub dropped: AtomicU64,
    /// The first error the encoder met; it writes nothing more after it.
    pub error: Mutex<Option<String>>,
}

impl Counters {
    pub fn error(&self) -> Option<String> {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Which tick each frame lands on: frames come with their tick number, in order, and a number
/// skipped is a gap to fill.
#[derive(Debug, Default, PartialEq, Eq)]
struct Ticks {
    next: u64,
}

impl Ticks {
    /// The ticks before `index` that no frame came for, to fill with the frame before; `None` when
    /// the frame is late (its tick is written already) and is left out.
    fn gap(&mut self, index: u64) -> Option<std::ops::Range<u64>> {
        if index < self.next {
            return None;
        }
        let gap = self.next..index;
        self.next = index + 1;
        Some(gap)
    }
}

enum Message {
    Frame { index: u64, rgba: Vec<u8> },
    Audio(Vec<i16>),
    End { frames: u64 },
}

/// A recording being written by its thread.
pub struct Recorder {
    tx: Sender<Message>,
    /// Frames sent and not yet taken by the thread.
    queued: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
    counters: Arc<Counters>,
    path: PathBuf,
}

impl Recorder {
    /// Opens the file and starts the thread that writes it. Fails when this FFmpeg cannot write HAP
    /// or the file cannot be made; nothing is left at the path then.
    pub fn start(config: Config) -> Result<Self, VideoError> {
        let counters = Arc::new(Counters::default());
        let queued = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel::<Message>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let path = config.path.clone();
        let thread = {
            let (counters, queued) = (Arc::clone(&counters), Arc::clone(&queued));
            thread::Builder::new()
                .name("flux-video-record".into())
                .spawn(move || match Writer::open(&config) {
                    Ok(mut writer) => {
                        let _ = ready_tx.send(Ok(()));
                        writer.run(&rx, &counters, &queued);
                    }
                    Err(err) => {
                        let _ = std::fs::remove_file(&config.path);
                        let _ = ready_tx.send(Err(err));
                    }
                })?
        };
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx,
                queued,
                thread: Some(thread),
                counters,
                path,
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => Err(VideoError::Encode("the encoder thread stopped".into())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn counters(&self) -> &Arc<Counters> {
        &self.counters
    }

    /// Hands over the frame of tick `index` (`rgba`, the configured size). `false` when the encoder
    /// is too far behind to take it: the tick is then filled by repeating the frame before.
    pub fn push_frame(&self, index: u64, rgba: Vec<u8>) -> bool {
        if self.queued.load(Ordering::Acquire) >= QUEUE {
            return false;
        }
        self.queued.fetch_add(1, Ordering::AcqRel);
        self.tx.send(Message::Frame { index, rgba }).is_ok()
    }

    /// Hands over interleaved samples, which follow the ones before.
    pub fn push_audio(&self, samples: Vec<i16>) {
        let _ = self.tx.send(Message::Audio(samples));
    }

    /// Writes what is queued and the end of the file, which lasts `frames` ticks. The error is
    /// the first one the encoder met, if any; the file is removed then.
    pub fn finish(mut self, frames: u64) -> Result<(), VideoError> {
        let _ = self.tx.send(Message::End { frames });
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        match self.counters.error() {
            Some(err) => {
                let _ = std::fs::remove_file(&self.path);
                Err(VideoError::Encode(err))
            }
            None => Ok(()),
        }
    }
}

/// The open file and its encoder, on the thread that writes.
struct Writer {
    output: Output,
    encoder: ffmpeg_next::encoder::Video,
    time_base: Rational,
    video_base: Rational,
    picture: frame::Video,
    size: (u32, u32),
    /// The sound's time base (a tick a sample) and the samples written so far; the channel count.
    sound: Option<(Rational, i64, u16)>,
    ticks: Ticks,
}

impl Writer {
    fn open(config: &Config) -> Result<Self, VideoError> {
        crate::init()?;
        let (width, height) = encoded_size(config.size);
        let mut output = format::output_as(&config.path, "mov")?;
        let chunks = thread::available_parallelism().map_or(1, |n| n.get().min(8) as u32);
        let (encoder, time_base) = video_encoder(
            &mut output,
            (width, height),
            (config.rate.0 as i32, config.rate.1 as i32),
            "hap",
            Some(chunks),
        )?;
        let sound = match config.audio {
            Some((rate, channels)) => Some((pcm_stream(&mut output, rate, channels)?, 0, channels)),
            None => None,
        };
        output.write_header()?;
        let video_base = output.stream(0).map_or(time_base, |s| s.time_base());
        let mut picture = frame::Video::new(Pixel::RGBA, width, height);
        // The pixels around the frame, which it never covers, are opaque black.
        let stride = picture.stride(0);
        for row in picture.data_mut(0).chunks_mut(stride) {
            for pixel in row.chunks_exact_mut(4) {
                pixel.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        Ok(Self {
            output,
            encoder,
            time_base,
            video_base,
            picture,
            size: config.size,
            sound,
            ticks: Ticks::default(),
        })
    }

    fn run(&mut self, rx: &mpsc::Receiver<Message>, counters: &Counters, queued: &AtomicUsize) {
        let mut failed = false;
        while let Ok(message) = rx.recv() {
            let result = match message {
                Message::Frame { index, rgba } => {
                    queued.fetch_sub(1, Ordering::AcqRel);
                    if failed {
                        continue;
                    }
                    self.frame(index, &rgba, counters)
                }
                Message::Audio(samples) if !failed => self.audio(&samples),
                Message::Audio(_) => Ok(()),
                Message::End { frames } => {
                    let result = if failed {
                        Ok(())
                    } else {
                        self.repeat(self.ticks.next..frames, counters)
                            .and_then(|()| self.close())
                    };
                    if let Err(err) = result {
                        record_error(counters, &err);
                    }
                    return;
                }
            };
            if let Err(err) = result {
                failed = true;
                record_error(counters, &err);
            }
        }
    }

    /// Writes the frame of tick `index`, after repeating the one before over the ticks missed.
    fn frame(&mut self, index: u64, rgba: &[u8], counters: &Counters) -> Result<(), VideoError> {
        let (width, height) = self.size;
        if rgba.len() != width as usize * height as usize * 4 {
            return Ok(());
        }
        // A frame that arrives for a tick written already is left out.
        let Some(gap) = self.ticks.gap(index) else {
            return Ok(());
        };
        // With no frame before, the ticks missed show this one, the nearest there is.
        if counters.written.load(Ordering::Relaxed) == 0 {
            self.fill(rgba);
        }
        self.repeat_range(gap, counters)?;
        self.fill(rgba);
        self.write(index)?;
        counters.written.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Repeats the picture last written over the ticks of `range`, which are counted dropped.
    fn repeat(
        &mut self,
        range: std::ops::Range<u64>,
        counters: &Counters,
    ) -> Result<(), VideoError> {
        if range.start < range.end && counters.written.load(Ordering::Relaxed) > 0 {
            self.ticks.next = range.end;
            self.repeat_range(range, counters)?;
        }
        Ok(())
    }

    fn repeat_range(
        &mut self,
        range: std::ops::Range<u64>,
        counters: &Counters,
    ) -> Result<(), VideoError> {
        for tick in range {
            self.write(tick)?;
            counters.written.fetch_add(1, Ordering::Relaxed);
            counters.dropped.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Copies a frame into the picture, row by row; the picture's rows are wider.
    fn fill(&mut self, rgba: &[u8]) {
        let row = self.size.0 as usize * 4;
        let stride = self.picture.stride(0);
        let data = self.picture.data_mut(0);
        for (y, line) in rgba.chunks_exact(row).enumerate() {
            data[y * stride..y * stride + row].copy_from_slice(line);
        }
    }

    /// Compresses the picture as tick `index` and writes what the encoder hands back.
    fn write(&mut self, index: u64) -> Result<(), VideoError> {
        self.picture.set_pts(Some(index as i64));
        self.encoder.send_frame(&self.picture)?;
        write_packets(
            &mut self.encoder,
            &mut self.output,
            self.time_base,
            self.video_base,
        )
    }

    fn audio(&mut self, samples: &[i16]) -> Result<(), VideoError> {
        let Some((base, written, channels)) = &mut self.sound else {
            return Ok(());
        };
        if samples.is_empty() {
            return Ok(());
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let frames = (samples.len() / usize::from(*channels)) as i64;
        let mut packet = Packet::copy(&bytes);
        packet.set_stream(1);
        packet.set_pts(Some(*written));
        packet.set_dts(Some(*written));
        packet.set_duration(frames);
        let to = self.output.stream(1).map_or(*base, |s| s.time_base());
        packet.rescale_ts(*base, to);
        packet.write_interleaved(&mut self.output)?;
        *written += frames;
        Ok(())
    }

    fn close(&mut self) -> Result<(), VideoError> {
        self.encoder.send_eof()?;
        write_packets(
            &mut self.encoder,
            &mut self.output,
            self.time_base,
            self.video_base,
        )?;
        self.output.write_trailer()?;
        Ok(())
    }
}

fn record_error(counters: &Counters, err: &VideoError) {
    counters
        .error
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(|| err.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_encoded_size_should_round_each_side_up_to_a_multiple_of_4() {
        assert_eq!(encoded_size((1920, 1080)), (1920, 1080));
        assert_eq!(encoded_size((1366, 768)), (1368, 768));
        assert_eq!(encoded_size((1, 5)), (4, 8));
    }

    #[test]
    fn a_missed_tick_should_be_a_gap_and_a_late_frame_left_out() {
        let mut ticks = Ticks::default();
        assert_eq!(ticks.gap(0), Some(0..0));
        assert_eq!(ticks.gap(1), Some(1..1));
        assert_eq!(ticks.gap(4), Some(2..4));
        assert_eq!(ticks.gap(3), None);
        assert_eq!(ticks.gap(5), Some(5..5));
    }
}
