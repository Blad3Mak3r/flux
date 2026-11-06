use std::io::Cursor;

use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use super::error::AudioError;

pub use flux_core::ENGINE_RATE;

const RESAMPLE_CHUNK: usize = 1024;

/// An audio file ready to be mixed: stereo, [`ENGINE_RATE`], 16-bit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Interleaved left and right.
    pub samples: Vec<i16>,
    /// The sample rate of the file before it was converted.
    pub source_rate: u32,
}

impl Decoded {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration_seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(ENGINE_RATE)
    }
}

/// Decodes `bytes`, using `extension` (`"mp3"`, `"wav"`, ...) as a hint for the container.
/// Mono is played on both sides and anything beyond two channels is dropped.
pub fn decode(bytes: Vec<u8>, extension: Option<&str>) -> Result<Decoded, AudioError> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = extension {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(decode_error)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or(AudioError::NoAudioTrack)?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err(AudioError::NoAudioTrack);
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(decode_error)?;

    let mut stereo: Vec<f32> = Vec::new();
    let mut scratch: Vec<f32> = Vec::new();
    let mut source_rate = params.sample_rate.unwrap_or(ENGINE_RATE);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // The track list changed halfway: what was decoded so far is what we can use.
            Err(SymphoniaError::ResetRequired) => break,
            Err(err) => return Err(decode_error(err)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buffer) => {
                let channels = buffer.spec().channels().count().max(1);
                source_rate = buffer.spec().rate();
                buffer.copy_to_vec_interleaved::<f32>(&mut scratch);
                for frame in scratch.chunks_exact(channels) {
                    stereo.push(frame[0]);
                    stereo.push(frame[channels.min(2) - 1]);
                }
            }
            // A damaged packet is skipped; the rest of the file is still good.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(decode_error(err)),
        }
    }
    if stereo.is_empty() {
        return Err(AudioError::NoAudioTrack);
    }

    let stereo = if source_rate == ENGINE_RATE {
        stereo
    } else {
        resample(stereo, source_rate)?
    };
    let samples = stereo
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)
        .collect();
    Ok(Decoded {
        samples,
        source_rate,
    })
}

fn decode_error(err: SymphoniaError) -> AudioError {
    AudioError::Decode(err.to_string())
}

fn resample(stereo: Vec<f32>, from: u32) -> Result<Vec<f32>, AudioError> {
    let frames = stereo.len() / 2;
    let input = InterleavedOwned::new_from(stereo, 2, frames)
        .map_err(|err| AudioError::Resample(err.to_string()))?;
    let mut resampler = Fft::<f32>::new(
        from as usize,
        ENGINE_RATE as usize,
        RESAMPLE_CHUNK,
        2,
        FixedSync::Both,
    )
    .map_err(|err| AudioError::Resample(err.to_string()))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|err| AudioError::Resample(err.to_string()))?;
    Ok(output.take_data())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 16-bit PCM WAV file holding `samples` (interleaved).
    pub fn wav(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        bytes.extend_from_slice(&(channels * 2).to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    /// `seconds` of a 440 Hz sine at `amplitude` (0 to 1), one value per frame.
    pub fn sine(sample_rate: u32, seconds: f64, amplitude: f64) -> Vec<i16> {
        (0..(f64::from(sample_rate) * seconds) as usize)
            .map(|n| {
                let phase = std::f64::consts::TAU * 440.0 * n as f64 / f64::from(sample_rate);
                (phase.sin() * amplitude * f64::from(i16::MAX)) as i16
            })
            .collect()
    }

    #[test]
    fn decode_should_keep_the_length_of_a_48k_stereo_file() {
        let mono = sine(48_000, 1.0, 0.5);
        let stereo: Vec<i16> = mono.iter().flat_map(|&s| [s, s]).collect();

        let decoded = decode(wav(48_000, 2, &stereo), Some("wav")).unwrap();

        assert_eq!(decoded.frames(), 48_000);
        assert_eq!(decoded.source_rate, 48_000);
        assert!((decoded.duration_seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decode_should_play_a_mono_file_on_both_sides() {
        let mono = sine(48_000, 0.25, 0.5);

        let decoded = decode(wav(48_000, 1, &mono), Some("wav")).unwrap();

        assert_eq!(decoded.frames(), mono.len());
        for frame in decoded.samples.chunks_exact(2) {
            assert_eq!(frame[0], frame[1]);
        }
    }

    #[test]
    fn decode_should_convert_44_1k_to_the_engine_rate_without_changing_the_duration() {
        let mono = sine(44_100, 2.0, 0.5);
        let stereo: Vec<i16> = mono.iter().flat_map(|&s| [s, s]).collect();

        let decoded = decode(wav(44_100, 2, &stereo), Some("wav")).unwrap();

        assert_eq!(decoded.source_rate, 44_100);
        let frames = decoded.frames() as i64;
        assert!((frames - 96_000).abs() <= 2, "{frames} frames");
        let peak = decoded
            .samples
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        assert!((15_000..=17_500).contains(&peak), "peak {peak}");
    }

    #[test]
    fn decode_should_work_without_an_extension_hint() {
        let decoded = decode(wav(48_000, 1, &sine(48_000, 0.1, 0.5)), None).unwrap();
        assert_eq!(decoded.frames(), 4_800);
    }

    #[test]
    fn decode_should_reject_bytes_that_are_not_audio() {
        let result = decode(b"definitely not audio".to_vec(), Some("wav"));
        assert!(matches!(result, Err(AudioError::Decode(_))));
    }
}
