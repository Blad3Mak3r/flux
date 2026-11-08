//! The `.flx` project file (legacy `.fxtc`): a zip with `project.json` and the project's audio files.

mod archive;
mod clips;
mod cue_ops;
mod error;
mod events;
mod item_ops;
pub(crate) mod meta;
mod migrate;
mod model;
pub mod operator;
pub use operator::{Alignment, GeneratedVoice, OperatorOp, VoiceOrder, VoiceStatus, VoiceView};
mod outputs;
mod preview;
pub mod recovery;
pub mod routing;
pub use outputs::{ClickSettings, LtcSignal, MtcSignal, ShowOutputs, SignalOp, UNASSIGNED_DEVICE};
mod session;
pub mod show;
pub use show::{ControlKind, Setlist, SetlistEntry, ShowEdit, TimelineControl, Transition};

pub use archive::{AssetSource, ProjectArchive, ProjectMeta, open, read_meta, save};
pub use clips::{ClipOp, apply as apply_clip_op};
pub use cue_ops::{CueEdited, CueOp, CueTrackKind};
pub use error::{ProjectError, Result};
pub use events::{
    MidiEvent, MidiMessage, MidiPortRef, MscCommand, OscArg, OscDestination, OscEvent,
};
pub use flux_core::FrameRate;
pub use item_ops::{Fade, ItemOp, MAX_TRACK_NAME};
pub use model::{
    AUDIO_DIR, AudioAsset, AudioClip, AudioOutput, Autosave, DEFAULT_LTC_LEVEL, ENVELOPE_MAX_DB,
    ENVELOPE_MIN_DB, ENVELOPE_TICKS_PER_FRAME, EnvCurve, EnvPoint, FORMAT_VERSION, FadeCurve,
    LtcOutput, MAX_BPM, MAX_CLIP_NAME, MAX_ENVELOPE_POINTS, MAX_OFFSET_MS, MIN_BPM, Meta,
    MetronomeOutput, MtcOutput, Note, OutputConfig, Project, Tempo, TimecodeSettings, Timeline,
    Track, TrackKind,
};
pub use preview::{AudioStamp, Preview};
pub use session::{Document, ProjectState, Session};

/// Whether a path has a supported project extension, including legacy projects.
pub fn is_project_path(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("flx") || ext.eq_ignore_ascii_case("fxtc"))
}

#[cfg(test)]
mod tests {
    use super::is_project_path;
    use std::path::Path;

    #[test]
    fn project_path_should_accept_both_extensions_case_insensitively() {
        for name in ["show.flx", "show.FLX", "show.fxtc", "show.FXTC"] {
            assert!(is_project_path(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn project_path_should_reject_other_extensions_and_missing_extensions() {
        for name in ["show", "show.zip", "show.flx.tmp", "show.fxtc.bak", "flx"] {
            assert!(!is_project_path(Path::new(name)), "{name}");
        }
    }
}
