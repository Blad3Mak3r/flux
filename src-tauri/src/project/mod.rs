//! The `.fxtc` project file: a zip archive with `project.json` and the project's audio files.

mod archive;
mod error;
mod migrate;
mod model;
mod preview;
mod session;

pub use archive::{AssetSource, ProjectArchive, ProjectMeta, open, read_meta, save};
pub use error::{ProjectError, Result};
pub use model::{
    AUDIO_DIR, AudioAsset, AudioClip, AudioOutput, FORMAT_VERSION, FrameRate, LtcOutput, MtcOutput,
    Note, OutputConfig, Project, TimecodeSettings, Track, TrackKind,
};
pub use preview::{AudioStamp, Preview};
pub use session::{Document, Session};
