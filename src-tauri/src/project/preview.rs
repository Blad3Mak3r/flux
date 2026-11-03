//! The waveform preview of an asset, kept in the `.fxtc` so opening a project does not decode its audio.
//!
//! It is a cache: an entry is trusted only while its [`AudioStamp`] matches the audio entry it was made
//! from, and a missing, unreadable or stale one is simply regenerated.

use serde::Serialize;

/// Folder inside the archive that holds the previews, one `<audio file name>.bin` per asset.
pub const PEAKS_DIR: &str = "peaks/";

const VERSION: u32 = 1;
const HEADER_LEN: usize = 32;

/// What `audio_analyze` returns for an asset.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub duration_seconds: f64,
    /// Sample rate of the file itself, before conversion to the engine's.
    pub source_rate: u32,
    pub peaks_per_second: u32,
    /// `[min0, max0, min1, max1, ...]` as signed bytes, full scale at -128 and 127.
    pub peaks: Vec<i8>,
}

/// Identifies the exact audio a preview was made from: the size and CRC-32 of its bytes, which is
/// also what the zip directory records for the audio entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStamp {
    pub size: u64,
    pub crc32: u32,
}

impl AudioStamp {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            size: bytes.len() as u64,
            crc32: crc32fast::hash(bytes),
        }
    }
}

/// A preview and the audio it describes.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredPreview {
    pub stamp: AudioStamp,
    pub preview: Preview,
}

/// The archive path of the preview of the asset whose audio is at `file` (`audio/<name>`).
pub fn preview_path(file: &str) -> String {
    let name = file.strip_prefix(super::model::AUDIO_DIR).unwrap_or(file);
    format!("{PEAKS_DIR}{name}.bin")
}

impl StoredPreview {
    /// Little-endian header (`version`, `peaksPerSecond`, `sourceRate`, audio `crc32`, audio `size`,
    /// `durationSeconds`) followed by the peaks as signed bytes.
    pub fn encode(&self) -> Vec<u8> {
        let Self { stamp, preview } = self;
        let mut bytes = Vec::with_capacity(HEADER_LEN + preview.peaks.len());
        bytes.extend(VERSION.to_le_bytes());
        bytes.extend(preview.peaks_per_second.to_le_bytes());
        bytes.extend(preview.source_rate.to_le_bytes());
        bytes.extend(stamp.crc32.to_le_bytes());
        bytes.extend(stamp.size.to_le_bytes());
        bytes.extend(preview.duration_seconds.to_le_bytes());
        bytes.extend(preview.peaks.iter().map(|&peak| peak as u8));
        bytes
    }

    /// `None` for anything this build cannot use: another version, a short header or odd peak count.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let (header, peaks) = bytes.split_first_chunk::<HEADER_LEN>()?;
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        if u32_at(0) != VERSION || peaks.len() % 2 != 0 {
            return None;
        }
        Some(Self {
            stamp: AudioStamp {
                crc32: u32_at(12),
                size: u64::from_le_bytes(header[16..24].try_into().unwrap()),
            },
            preview: Preview {
                duration_seconds: f64::from_le_bytes(header[24..32].try_into().unwrap()),
                source_rate: u32_at(8),
                peaks_per_second: u32_at(4),
                peaks: peaks.iter().map(|&peak| peak as i8).collect(),
            },
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn stored(audio: &[u8]) -> StoredPreview {
        StoredPreview {
            stamp: AudioStamp::of(audio),
            preview: Preview {
                duration_seconds: 1.5,
                source_rate: 44_100,
                peaks_per_second: 100,
                peaks: vec![-128, 127, -3, 4],
            },
        }
    }

    #[test]
    fn decode_should_return_what_encode_wrote() {
        let original = stored(b"audio");
        assert_eq!(StoredPreview::decode(&original.encode()), Some(original));
    }

    #[test]
    fn decode_should_reject_a_short_header_an_unknown_version_and_an_odd_peak_count() {
        let bytes = stored(b"audio").encode();

        assert_eq!(StoredPreview::decode(&bytes[..HEADER_LEN - 1]), None);
        assert_eq!(StoredPreview::decode(&bytes[..bytes.len() - 1]), None);
        let mut newer = bytes;
        newer[0] = 2;
        assert_eq!(StoredPreview::decode(&newer), None);
    }

    #[test]
    fn preview_path_should_mirror_the_audio_file_name() {
        assert_eq!(preview_path("audio/a1_click.wav"), "peaks/a1_click.wav.bin");
    }
}
