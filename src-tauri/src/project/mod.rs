//! The `.fxtc` project file: a zip archive with `project.json` and the project's audio files.

mod archive;
mod clips;
mod error;
pub(crate) mod meta;
mod migrate;
mod model;
mod preview;
mod session;

pub use archive::{AssetSource, ProjectArchive, ProjectMeta, open, read_meta, save};
pub use clips::ClipOp;
pub use error::{ProjectError, Result};
pub use model::{
    AUDIO_DIR, AudioAsset, AudioClip, AudioOutput, DEFAULT_LTC_LEVEL, FORMAT_VERSION, FrameRate,
    LtcOutput, MAX_BPM, MAX_OFFSET_MS, MIN_BPM, Meta, MetronomeOutput, MtcOutput, Note,
    OutputConfig, Project, Tempo, TimecodeSettings, Timeline, Track, TrackKind,
};
pub use preview::{AudioStamp, Preview};
pub use session::{Document, OffsetKind, ProjectState, Session};
