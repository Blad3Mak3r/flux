//! A timeline's LTC rendered offline into a mono WAV file.
//!
//! The samples come from the same [`LtcStream`] the output plays, started at the first frame of the
//! file, so the file is the signal Flux would send, frame for frame. It is rendered as fast as the
//! disk takes it and in pieces, so a long show does not sit in memory. Nothing here touches a device
//! or an open output.

use std::io::{self, Write};

use serde::Deserialize;

use crate::ltc::{LtcConfig, LtcStream};
use flux_core::{Clock, ENGINE_RATE, FrameRate};

/// The sample rates a file can have.
pub const SAMPLE_RATES: [u32; 3] = [44_100, 48_000, 96_000];

/// The longest pre-roll, in seconds.
pub const MAX_PRE_ROLL_SECONDS: u32 = 60;

/// Samples written between two reports of progress.
const CHUNK: u64 = 1 << 16;

/// How each sample is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BitDepth {
    Pcm16,
    Pcm24,
    /// 32-bit IEEE float, the signal as the generator makes it.
    Float32,
}

impl BitDepth {
    fn bytes(self) -> u64 {
        match self {
            Self::Pcm16 => 2,
            Self::Pcm24 => 3,
            Self::Float32 => 4,
        }
    }
}

/// What the file is made of. The default, 48 kHz and 24-bit, is what every interface takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub sample_rate: u32,
    pub depth: BitDepth,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            depth: BitDepth::Pcm24,
        }
    }
}

/// The whole frames that cover `end_samples` (at the engine rate) of a timeline at `rate`: the last
/// one is complete, so the signal never stops in the middle of a word.
pub fn frames_to_cover(end_samples: u64, rate: FrameRate) -> u64 {
    let clock = Clock::new(ENGINE_RATE, rate);
    let frames = clock.samples_to_frames(end_samples);
    if clock.frames_to_samples(frames) < end_samples {
        frames + 1
    } else {
        frames
    }
}

/// The frames of `seconds` of pre-roll at `rate`, rounded up to a whole frame.
pub fn pre_roll_frames(rate: FrameRate, seconds: u32) -> u64 {
    let (num, den) = rate.fraction();
    (u64::from(seconds) * num).div_ceil(den)
}

/// The timecode frame a file starts at when it begins `pre_roll` frames before `start`. It wraps
/// across midnight like timecode does, so a timeline at 00:00:00:00 gets its pre-roll from 23:59.
pub fn first_frame(rate: FrameRate, start: u64, pre_roll: u64) -> u64 {
    let day = rate.frames_per_day();
    (start % day + day - pre_roll % day) % day
}

/// Writes `frames` frames of LTC, the first being timecode `start` (a frame count), at `level` (0 to
/// 1, linear), in `format`. `progress` is told how far the render is, from 0 to 1. Returns the
/// samples written.
pub fn write(
    out: &mut impl Write,
    format: Format,
    rate: FrameRate,
    start: u64,
    frames: u64,
    level: f32,
    mut progress: impl FnMut(f32),
) -> io::Result<u64> {
    if !SAMPLE_RATES.contains(&format.sample_rate) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported sample rate",
        ));
    }
    let clock = Clock::new(format.sample_rate, rate);
    let samples = clock.frames_to_samples(frames);
    let width = format.depth.bytes();
    let head = header(format, samples * width);
    // A WAV file counts its bytes in 32 bits.
    if (samples * width + head.len() as u64) > u64::from(u32::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the timeline is too long for a WAV file",
        ));
    }
    out.write_all(&head)?;

    let config = LtcConfig {
        sample_rate: format.sample_rate,
        level,
        ..LtcConfig::default()
    };
    let mut stream = LtcStream::new(config, rate, start, 0);
    stream.resync(0);
    let mut buffer = Vec::with_capacity(CHUNK as usize * width as usize);
    let mut written = 0;
    while written < samples {
        buffer.clear();
        for _ in 0..CHUNK.min(samples - written) {
            let sample = stream.next_sample();
            match format.depth {
                BitDepth::Pcm16 => {
                    let value = (sample * 32_767.0).round().clamp(-32_767.0, 32_767.0) as i16;
                    buffer.extend_from_slice(&value.to_le_bytes());
                }
                BitDepth::Pcm24 => {
                    let value = (sample * 8_388_607.0)
                        .round()
                        .clamp(-8_388_607.0, 8_388_607.0) as i32;
                    buffer.extend_from_slice(&value.to_le_bytes()[..3]);
                }
                BitDepth::Float32 => buffer.extend_from_slice(&sample.to_le_bytes()),
            }
        }
        out.write_all(&buffer)?;
        written += buffer.len() as u64 / width;
        progress(written as f32 / samples as f32);
    }
    Ok(samples)
}

/// The bytes that open a mono file of `format` with `data_len` bytes of samples. PCM gets the
/// classic 44-byte header; float gets the extended `fmt ` chunk and the `fact` chunk the format
/// asks for.
fn header(format: Format, data_len: u64) -> Vec<u8> {
    let float = format.depth == BitDepth::Float32;
    let bits = (format.depth.bytes() * 8) as u16;
    let block = bits / 8;
    // Clamped: `write` refuses a file whose size does not fit before it writes anything.
    let data_len = u32::try_from(data_len).unwrap_or(u32::MAX);
    let mut h = Vec::with_capacity(58);
    h.extend_from_slice(b"RIFF");
    let riff_len = data_len.saturating_add(if float { 50 } else { 36 });
    h.extend_from_slice(&riff_len.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&(if float { 18u32 } else { 16 }).to_le_bytes());
    h.extend_from_slice(&(if float { 3u16 } else { 1 }).to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&format.sample_rate.to_le_bytes());
    h.extend_from_slice(&(format.sample_rate * u32::from(block)).to_le_bytes());
    h.extend_from_slice(&block.to_le_bytes());
    h.extend_from_slice(&bits.to_le_bytes());
    if float {
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(b"fact");
        h.extend_from_slice(&4u32.to_le_bytes());
        h.extend_from_slice(&(data_len / u32::from(block)).to_le_bytes());
    }
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data_len.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ltc::BITS_PER_FRAME;
    use flux_core::Timecode;

    const RATES: [FrameRate; 4] = [
        FrameRate::Fps24,
        FrameRate::Fps25,
        FrameRate::Fps2997Drop,
        FrameRate::Fps30,
    ];
    const DEPTHS: [BitDepth; 3] = [BitDepth::Pcm16, BitDepth::Pcm24, BitDepth::Float32];

    fn format(sample_rate: u32, depth: BitDepth) -> Format {
        Format { sample_rate, depth }
    }

    fn render(format: Format, rate: FrameRate, start: u64, frames: u64, level: f32) -> Vec<u8> {
        let mut file = Vec::new();
        write(&mut file, format, rate, start, frames, level, |_| {}).unwrap();
        file
    }

    fn u32_at(file: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(file[at..at + 4].try_into().unwrap())
    }

    fn u16_at(file: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(file[at..at + 2].try_into().unwrap())
    }

    /// Reads a file back: its format tag, bits, sample rate and samples as floats.
    fn parse(file: &[u8]) -> (u16, u16, u32, Vec<f32>) {
        assert_eq!(&file[..4], b"RIFF");
        assert_eq!(&file[8..12], b"WAVE");
        let (tag, bits, rate) = (u16_at(file, 20), u16_at(file, 34), u32_at(file, 24));
        let mut at = 12;
        // Walk the chunks to `data`, as any reader does.
        while &file[at..at + 4] != b"data" {
            at += 8 + u32_at(file, at + 4) as usize;
        }
        let len = u32_at(file, at + 4) as usize;
        assert_eq!(file.len(), at + 8 + len, "the data chunk ends the file");
        assert_eq!(u32_at(file, 4) as usize, file.len() - 8);
        let body = &file[at + 8..];
        let samples = match bits {
            16 => body
                .chunks(2)
                .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_767.0)
                .collect(),
            24 => body
                .chunks(3)
                .map(|b| (i32::from_le_bytes([b[0], b[1], b[2], 0]) << 8 >> 8) as f32 / 8_388_607.0)
                .collect(),
            _ => body
                .chunks(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect(),
        };
        (tag, bits, rate, samples)
    }

    /// Reads the biphase-mark samples of one frame back into its 80 bits.
    fn decode(samples: &[f32], sample_rate: u32, rate: FrameRate) -> Vec<bool> {
        let (num, den) = rate.fraction();
        let cell = f64::from(sample_rate) * den as f64 / (num as f64 * BITS_PER_FRAME as f64);
        // The encoder carries its polarity from one frame to the next: a sample ahead of the frame
        // that is the opposite of its first one shows the transition at its very start.
        let lead = -samples[0].signum();
        let closing = -samples[samples.len() - 1].signum();
        let signal: Vec<f32> = std::iter::once(lead)
            .chain(samples.iter().copied())
            .chain(std::iter::once(closing))
            .collect();
        let edges: Vec<usize> = (1..signal.len())
            .filter(|&i| (signal[i] > 0.0) != (signal[i - 1] > 0.0))
            .collect();
        let gap = |at: usize| (edges[at + 1] - edges[at]) as f64;
        let near = |value: f64, target: f64| (value - target).abs() < cell / 4.0;
        let mut bits = Vec::new();
        let mut at = 0;
        while at + 1 < edges.len() {
            if near(gap(at), cell) {
                bits.push(false);
                at += 1;
            } else if at + 2 < edges.len()
                && near(gap(at), cell / 2.0)
                && near(gap(at + 1), cell / 2.0)
            {
                bits.push(true);
                at += 2;
            } else {
                panic!("bad edge spacing at transition {at}");
            }
        }
        bits
    }

    fn field(bits: &[bool], position: usize, width: usize) -> u8 {
        (0..width).map(|i| u8::from(bits[position + i]) << i).sum()
    }

    fn timecode_of(bits: &[bool]) -> Timecode {
        Timecode {
            hours: field(bits, 56, 2) * 10 + field(bits, 48, 4),
            minutes: field(bits, 40, 3) * 10 + field(bits, 32, 4),
            seconds: field(bits, 24, 3) * 10 + field(bits, 16, 4),
            frames: field(bits, 8, 2) * 10 + field(bits, 0, 4),
        }
    }

    /// Decodes every frame of `file` and checks it is the timecode `first + n` at `rate`.
    fn assert_frames(file: &[u8], sample_rate: u32, rate: FrameRate, first: u64, frames: u64) {
        let (_, _, _, samples) = parse(file);
        let clock = Clock::new(sample_rate, rate);
        assert_eq!(samples.len() as u64, clock.frames_to_samples(frames));
        for frame in 0..frames {
            let from = clock.frames_to_samples(frame) as usize;
            let to = clock.frames_to_samples(frame + 1) as usize;
            let bits = decode(&samples[from..to], sample_rate, rate);
            assert_eq!(bits.len(), BITS_PER_FRAME, "{rate:?} frame {frame}");
            assert_eq!(
                timecode_of(&bits),
                Timecode::from_frames(first + frame, rate),
                "{rate:?} {sample_rate} Hz frame {frame}"
            );
            assert_eq!(bits[10], rate.is_drop_frame());
        }
    }

    #[test]
    fn each_depth_should_have_the_header_of_its_format() {
        for (depth, tag, bits) in [
            (BitDepth::Pcm16, 1, 16),
            (BitDepth::Pcm24, 1, 24),
            (BitDepth::Float32, 3, 32),
        ] {
            let file = render(format(44_100, depth), FrameRate::Fps25, 0, 25, 0.5);
            let (found_tag, found_bits, rate, samples) = parse(&file);
            assert_eq!((found_tag, found_bits, rate), (tag, bits, 44_100));
            assert_eq!(samples.len(), 44_100);
            assert_eq!(u16_at(&file, 22), 1, "mono");
            assert_eq!(u32_at(&file, 28), 44_100 * u32::from(bits / 8));
            assert_eq!(u16_at(&file, 32), bits / 8);
            if depth == BitDepth::Float32 {
                assert_eq!(&file[38..42], b"fact");
                assert_eq!(u32_at(&file, 46), 44_100);
            }
        }
    }

    #[test]
    fn every_frame_should_decode_to_its_timecode_in_every_format_and_rate() {
        for rate in RATES {
            // Seconds before a minute, so the drop-frame numbers that skip show up.
            let start = Timecode {
                hours: 1,
                minutes: 0,
                seconds: 59,
                frames: 20,
            }
            .to_frames(rate)
            .unwrap();
            for sample_rate in SAMPLE_RATES {
                for depth in DEPTHS {
                    let file = render(format(sample_rate, depth), rate, start, 40, 0.5);
                    assert_frames(&file, sample_rate, rate, start, 40);
                }
            }
        }
    }

    #[test]
    fn the_signal_should_be_the_one_the_output_plays_at_the_level_asked() {
        let file = render(Format::default(), FrameRate::Fps30, 108_000, 10, 0.25);
        let (_, _, _, written) = parse(&file);
        let mut stream = LtcStream::new(
            LtcConfig {
                sample_rate: 48_000,
                level: 0.25,
                ..LtcConfig::default()
            },
            FrameRate::Fps30,
            108_000,
            0,
        );
        stream.resync(0);
        for sample in &written {
            assert!((sample - stream.next_sample()).abs() < 1e-6);
        }
        let peak = written.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!((peak - 0.25).abs() < 1e-5);
    }

    #[test]
    fn float_samples_should_be_the_generators_exactly() {
        let file = render(
            format(48_000, BitDepth::Float32),
            FrameRate::Fps25,
            0,
            5,
            0.5,
        );
        let (_, _, _, written) = parse(&file);
        assert!(written.iter().all(|s| s.abs() <= 0.5));
        assert!(written.contains(&0.5));
    }

    #[test]
    fn frames_to_cover_should_round_up_to_a_whole_frame() {
        let second = u64::from(ENGINE_RATE);
        assert_eq!(frames_to_cover(0, FrameRate::Fps25), 0);
        assert_eq!(frames_to_cover(second, FrameRate::Fps25), 25);
        assert_eq!(frames_to_cover(second + 1, FrameRate::Fps25), 26);
    }

    #[test]
    fn a_pre_roll_should_start_that_many_seconds_before_the_timeline_and_wrap_at_midnight() {
        let rate = FrameRate::Fps25;
        assert_eq!(pre_roll_frames(rate, 0), 0);
        assert_eq!(pre_roll_frames(rate, 2), 50);
        assert_eq!(pre_roll_frames(FrameRate::Fps2997Drop, 10), 300);
        // 01:00:00:00 with two seconds of pre-roll starts at 00:59:58:00.
        let hour = Timecode {
            hours: 1,
            minutes: 0,
            seconds: 0,
            frames: 0,
        }
        .to_frames(rate)
        .unwrap();
        let first = first_frame(rate, hour, 50);
        assert_eq!(
            Timecode::from_frames(first, rate),
            Timecode {
                hours: 0,
                minutes: 59,
                seconds: 58,
                frames: 0
            }
        );
        // At 00:00:00:00 it wraps to the end of the day.
        let wrapped = Timecode::from_frames(first_frame(rate, 0, 50), rate);
        assert_eq!(
            (wrapped.hours, wrapped.minutes, wrapped.seconds),
            (23, 59, 58)
        );
        // The file decodes from the pre-roll on, and the timeline's own start is where the pre-roll ends.
        let file = render(Format::default(), rate, first, 50 + 10, 0.5);
        assert_frames(&file, 48_000, rate, first, 60);
        let (_, _, _, samples) = parse(&file);
        let at_start = Clock::new(48_000, rate).frames_to_samples(50) as usize;
        let bits = decode(&samples[at_start..at_start + 1920], 48_000, rate);
        assert_eq!(timecode_of(&bits), Timecode::from_frames(hour, rate));
    }

    #[test]
    fn progress_should_reach_one_and_a_file_too_long_for_wav_should_be_refused() {
        let mut last = 0.0;
        write(
            &mut io::sink(),
            Format::default(),
            FrameRate::Fps25,
            0,
            100,
            0.5,
            |p| last = p,
        )
        .unwrap();
        assert_eq!(last, 1.0);
        let hours = 9 * 3_600 * 25;
        let err = write(
            &mut io::sink(),
            Format::default(),
            FrameRate::Fps25,
            0,
            hours,
            0.5,
            |_| {},
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        // A float file is bigger per sample, so it hits the limit sooner than 16-bit does.
        let long = 6 * 3_600 * 25;
        let float = format(96_000, BitDepth::Float32);
        assert!(
            write(
                &mut io::sink(),
                float,
                FrameRate::Fps25,
                0,
                long,
                0.5,
                |_| {}
            )
            .is_err()
        );
    }

    #[test]
    fn an_unknown_sample_rate_should_be_refused() {
        let err = write(
            &mut io::sink(),
            format(22_050, BitDepth::Pcm16),
            FrameRate::Fps25,
            0,
            1,
            0.5,
            |_| {},
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
