//! The sound of a video, for the mixer to play like an audio clip: FFmpeg decodes the file's main
//! audio stream, which is handed over as a 16-bit WAV at the stream's own rate. The audio crate
//! converts it to the engine's rate, as it does any other file.

use std::path::Path;

use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::sample::{Sample, Type as Layout};
use ffmpeg_next::media::Type;
use ffmpeg_next::util::error::EAGAIN;
use ffmpeg_next::{decoder, frame};

use crate::decode::VideoError;

/// Decodes the main audio stream of the video at `path` into a WAV of one or two channels (a
/// third channel and beyond is left out). `None` when the video has no sound. It reads the whole
/// file, so call it off the UI thread.
pub fn audio_wav(path: &Path) -> Result<Option<Vec<u8>>, VideoError> {
    crate::init()?;
    let mut input = ffmpeg_next::format::input(path)?;
    let Some(stream) = input.streams().best(Type::Audio) else {
        return Ok(None);
    };
    let index = stream.index();
    let mut decoder = CodecContext::from_parameters(stream.parameters())?
        .decoder()
        .audio()?;
    let mut pcm = Pcm::default();
    for (stream, packet) in input.packets() {
        if stream.index() == index {
            decoder.send_packet(&packet)?;
            pcm.drain(&mut decoder)?;
        }
    }
    decoder.send_eof()?;
    pcm.drain(&mut decoder)?;
    Ok((pcm.rate > 0).then(|| pcm.wav()))
}

#[derive(Default)]
struct Pcm {
    rate: u32,
    channels: u16,
    samples: Vec<i16>,
}

impl Pcm {
    fn drain(&mut self, decoder: &mut decoder::Audio) -> Result<(), VideoError> {
        let mut decoded = frame::Audio::empty();
        loop {
            match decoder.receive_frame(&mut decoded) {
                Ok(()) => self.push(&decoded),
                Err(ffmpeg_next::Error::Other { errno }) if errno == EAGAIN => return Ok(()),
                Err(ffmpeg_next::Error::Eof) => return Ok(()),
                Err(err) => return Err(err.into()),
            }
        }
    }

    fn push(&mut self, decoded: &frame::Audio) {
        let channels = usize::from(decoded.channels().max(1));
        if self.rate == 0 {
            self.rate = decoded.rate();
            self.channels = decoded.channels().clamp(1, 2);
        }
        let kept = usize::from(self.channels);
        let format = decoded.format();
        for i in 0..decoded.samples() {
            for ch in 0..kept.min(channels) {
                let sample = match format {
                    Sample::U8(layout) => {
                        let byte = read::<1>(decoded, layout, channels, ch, i)[0];
                        (i16::from(byte) - 128) << 8
                    }
                    Sample::I16(layout) => {
                        i16::from_ne_bytes(read::<2>(decoded, layout, channels, ch, i))
                    }
                    Sample::I32(layout) => {
                        (i32::from_ne_bytes(read::<4>(decoded, layout, channels, ch, i)) >> 16)
                            as i16
                    }
                    Sample::F32(layout) => to_i16(f64::from(f32::from_ne_bytes(read::<4>(
                        decoded, layout, channels, ch, i,
                    )))),
                    Sample::F64(layout) => to_i16(f64::from_ne_bytes(read::<8>(
                        decoded, layout, channels, ch, i,
                    ))),
                    // Formats no video codec gives: silence rather than noise.
                    _ => 0,
                };
                self.samples.push(sample);
            }
        }
    }

    fn wav(&self) -> Vec<u8> {
        let data = (self.samples.len() * 2) as u32;
        let align = self.channels * 2;
        let mut wav = Vec::with_capacity(44 + data as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&self.channels.to_le_bytes());
        wav.extend_from_slice(&self.rate.to_le_bytes());
        wav.extend_from_slice(&(self.rate * u32::from(align)).to_le_bytes());
        wav.extend_from_slice(&align.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        for sample in &self.samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        wav
    }
}

/// The `N` bytes of sample `i` of channel `ch`, wherever the layout keeps them.
fn read<const N: usize>(
    decoded: &frame::Audio,
    layout: Layout,
    channels: usize,
    ch: usize,
    i: usize,
) -> [u8; N] {
    let (plane, at) = match layout {
        Layout::Packed => (0, (i * channels + ch) * N),
        Layout::Planar => (ch, i * N),
    };
    decoded.data(plane)[at..at + N].try_into().unwrap_or([0; N])
}

fn to_i16(x: f64) -> i16 {
    (x.clamp(-1.0, 1.0) * 32_767.0) as i16
}
