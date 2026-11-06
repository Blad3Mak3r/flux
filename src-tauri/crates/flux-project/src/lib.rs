//! The `.fxtc` project file: a zip archive with `project.json` and the project's audio files.

mod archive;
mod clips;
mod cue_ops;
mod error;
mod events;
pub(crate) mod meta;
mod migrate;
mod model;
mod preview;
mod session;

pub use archive::{AssetSource, ProjectArchive, ProjectMeta, open, read_meta, save};
pub use clips::{ClipOp, apply as apply_clip_op};
pub use cue_ops::{CueEdited, CueOp, CueTrackKind};
pub use error::{ProjectError, Result};
pub use events::{
    MidiEvent, MidiMessage, MidiPortRef, MscCommand, OscArg, OscDestination, OscEvent,
};
pub use flux_core::FrameRate;
pub use model::{
    AUDIO_DIR, AudioAsset, AudioClip, AudioOutput, DEFAULT_LTC_LEVEL, ENVELOPE_MAX_DB,
    ENVELOPE_MIN_DB, ENVELOPE_TICKS_PER_FRAME, EnvCurve, EnvPoint, FORMAT_VERSION, FadeCurve,
    LtcOutput, MAX_BPM, MAX_CLIP_NAME, MAX_ENVELOPE_POINTS, MAX_OFFSET_MS, MIN_BPM, Meta,
    MetronomeOutput, MtcOutput, Note, OutputConfig, Project, Tempo, TimecodeSettings, Timeline,
    Track, TrackKind,
};
pub use preview::{AudioStamp, Preview};
pub use session::{Document, OffsetKind, ProjectState, Session};
