//! The programme: the project's audio clips mixed into a stereo pair, sample by sample on the
//! transport's clock. Pure and allocation-free once built, so the audio callback can call it.
//!
//! Clips are mixed per track: each track's clips are summed, scaled by the track's fader and added
//! to the programme, and the peak of what the track contributed is kept for the level meters.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use flux_audio::{AudioError, Decoded, ENGINE_RATE};
use flux_core::Clock;
use flux_project::{
    AudioAsset, AudioClip, ENVELOPE_MIN_DB, ENVELOPE_TICKS_PER_FRAME, EnvCurve, FadeCurve, Project,
    Timeline, TrackKind, VideoAsset,
};

mod build;
mod clip_set;
mod envelope;
mod levels;

pub use build::*;
pub use clip_set::*;
pub use envelope::*;
pub use levels::*;

#[cfg(test)]
mod tests;

/// Frames mixed at a time, on the stack: the callback must not allocate.
pub const MIX_CHUNK: usize = 256;

/// A track fader at or below this is silence (the fader's "-inf" end).
pub const SILENT_DB: f32 = -60.0;

/// Where the engine's current [`TrackLevels`] is, for threads outside the audio callback. The set
/// is swapped when the clips change, so readers ask this each time instead of keeping the old one.
#[derive(Debug, Clone, Default)]
pub struct SharedLevels(Arc<Mutex<Arc<TrackLevels>>>);

impl SharedLevels {
    pub fn new(levels: Arc<TrackLevels>) -> Self {
        Self(Arc::new(Mutex::new(levels)))
    }

    pub fn replace(&self, levels: Arc<TrackLevels>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = levels;
    }

    pub fn current(&self) -> Arc<TrackLevels> {
        Arc::clone(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// 16-bit samples to the -1.0 to 1.0 range.
const SCALE: f32 = 1.0 / 32_768.0;

fn gain_from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A track fader in dB to a linear gain; [`SILENT_DB`] and below is silence.
fn track_gain_from_db(db: f32) -> f32 {
    if db <= SILENT_DB {
        0.0
    } else {
        gain_from_db(db)
    }
}
