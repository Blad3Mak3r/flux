mod asset;
mod clip;
mod output;
mod project;
#[cfg(test)]
pub(crate) mod tests;
mod timeline;
mod track;
mod validate;

pub use asset::*;
pub use clip::*;
pub use output::*;
pub use project::*;
pub use timeline::*;
pub use track::*;
use validate::*;
pub(super) use validate::{check_asset_path, check_video_path};

pub use crate::meta::Meta;

/// Version written to `project.json`. Readers reject anything newer and migrate older ones
/// (see `migrate.rs`).
///
/// Version 2 turned event tracks into notes and `output.ltc` / `output.mtc` into lists; version 3
/// added `meta`; version 4 moved `timecode` and `tracks` into `timelines`; version 5 added the `midi`
/// and `osc` track kinds and `output.osc` (nothing was reshaped, but an older build cannot read them). Files of version 2
/// written while the format was changing can hold either shape, so the migration looks at the
/// shape, not at the number.
/// Version 6 separates portable signals from local hardware routing.
/// Version 9 adds recording origin, pronunciation text and non-destructive speech trims.
/// Version 13 adds `videos` and the `video` track kind.
/// Version 14 adds `mode` and the VJ grid `vj`.
/// Version 15 gives a VJ show a file of its own shape (see `vj_file`); a cue show's is unchanged.
/// Version 16 adds `link` to audio and video assets: media kept outside the archive, by path.
/// Version 17 adds the `dmx` track kind and `output.dmx` (nothing was reshaped, but an older build cannot read them).
pub const FORMAT_VERSION: u32 = 17;

/// Folder inside the archive that holds the audio files (flat, no subfolders).
pub const AUDIO_DIR: &str = "audio/";

/// Folder inside the archive that holds the video files (flat, no subfolders).
pub const VIDEO_DIR: &str = "video/";
