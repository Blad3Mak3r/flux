//! The `.flx` project file (legacy `.fxtc`): a zip with `project.json` and the project's audio files.

mod archive;
mod clips;
mod cue_ops;
mod dropped;
mod error;
mod events;
pub mod generator;
mod item_ops;
pub mod media;
pub(crate) mod meta;
mod migrate;
mod model;
pub mod operator;
pub use operator::{Alignment, GeneratedVoice, OperatorOp, VoiceOrder, VoiceStatus, VoiceView};
pub mod mapping;
mod outputs;
mod preview;
pub mod recovery;
pub mod routing;
pub use outputs::{ClickSettings, LtcSignal, MtcSignal, ShowOutputs, SignalOp, UNASSIGNED_DEVICE};
pub mod playhead;
mod serde_util;
mod session;
pub mod show;
#[cfg(test)]
mod test_util;
mod video_clips;
pub mod vj;
pub mod vj_effects;
pub mod vj_feedback;
mod vj_file;
pub mod vj_groups;
pub mod vj_layout;
pub mod vj_mapfile;
pub mod vj_mapping;
pub mod vj_modulation;
pub mod vj_presets;
pub use show::{ControlKind, Setlist, SetlistEntry, ShowEdit, TimelineControl, Transition};

pub use archive::{AssetSource, ProjectArchive, ProjectMeta, open, read_meta, save};
pub use clips::{ClipOp, apply as apply_clip_op};
pub use cue_ops::{CueEdited, CueOp, CueTrackKind};
pub use dropped::{Dropped, MediaKind, Staged, media_kind};
pub use error::{ProjectError, Result};
pub use events::{
    MidiEvent, MidiMessage, MidiPortRef, MscCommand, OscArg, OscDestination, OscEvent,
};
pub use flux_core::FrameRate;
pub use item_ops::{Fade, ItemOp, MAX_TRACK_NAME};
pub use model::{
    AUDIO_DIR, AudioAsset, AudioClip, AudioOutput, Autosave, DEFAULT_LTC_LEVEL, ENVELOPE_MAX_DB,
    ENVELOPE_MIN_DB, ENVELOPE_TICKS_PER_FRAME, EnvCurve, EnvPoint, FORMAT_VERSION, FadeCurve,
    LtcOutput, MAX_BPM, MAX_CLIP_NAME, MAX_ENVELOPE_POINTS, MAX_OFFSET_MS, MIN_BPM, MediaLink,
    Meta, MetronomeOutput, MtcOutput, Note, OutputConfig, Project, Tempo, TimecodeSettings,
    Timeline, Track, TrackKind, VIDEO_DIR, VideoAsset, VideoClip, VideoFit,
};
pub use preview::{AudioStamp, Preview};
pub use session::{Document, ProjectState, Session};
pub use video_clips::VideoClipOp;
pub use vj::{ProjectMode, VjOp};

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
