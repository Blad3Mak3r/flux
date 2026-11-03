//! Audio files as the engine plays them: decoded once into stereo, 48 kHz, 16-bit.
//!
//! Decoding and the waveform peaks are pure functions of the file's bytes, so they are tested
//! without a device. Sixteen bits keeps a three minute song near 35 MB in memory (32-bit float
//! would double that), which matters when a show holds dozens of songs.

mod cache;
mod decode;
mod error;
mod peaks;

pub use cache::{AudioCache, SharedAudio};
pub use decode::{Decoded, ENGINE_RATE, decode};
pub use error::AudioError;
pub use peaks::{PEAKS_PER_SECOND, peaks};
