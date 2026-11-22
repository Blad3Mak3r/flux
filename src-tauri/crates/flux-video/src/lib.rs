//! Video files as the engine shows them. FFmpeg demuxes and decodes them (see
//! `docs/video.md`); which frame a clip shows at a point of the timeline is a pure function of the
//! timeline position ([`source_frame`]), so the picture follows the engine clock and never a timer
//! of its own.

mod deck;
mod decode;
mod layers;
mod mapping;
mod player;
mod thumbs;

pub use deck::{PRELOAD_FRAMES, Shown, VideoDeck};
pub use decode::{Frame, VideoError, VideoFile, VideoInfo};
pub use layers::{Cue, Layer, Probe, layers, upcoming};
pub use mapping::{Rate, source_frame, timeline_frames};
pub use player::{PlayerStats, VideoPlayer};
pub use thumbs::{THUMB_HEIGHT, Thumbs, thumbnails};

/// Starts FFmpeg's global state. Safe to call more than once; [`VideoFile::open`] calls it.
pub fn init() -> Result<(), VideoError> {
    ffmpeg_next::init().map_err(VideoError::from)
}
