//! The `.flxmap` file: the mappings and dashboard links of a VJ grid, to carry to another show.
//!
//! A mapping names its target by **id** (the id of a layer, a clip), which means nothing in
//! another project, so the file names it by **position** instead, 0-based, and import looks the
//! position up in the project it is read into:
//!
//! | Id of | In the file |
//! | --- | --- |
//! | layer | the layer's place from the bottom: `"2"` |
//! | deck | the deck's place: `"0"` |
//! | column | `"deck.column"`: `"0.3"` |
//! | clip | `"layer/deck.column"`, or `"layer/sN"` for a slot of a global layer: `"2/0.3"`, `"1/s5"` |
//! | effect | `"owner:slot"`, the owner being `c` (the composition), `lN` (layer N) or `kN/…` (a clip, as above) |
//!
//! Everything else (a source, a range, a global target, a knob's number, an effect's setting by
//! name) is stored as it is. What does not resolve, or no longer fits (a place that is not there,
//! a setting the effect in that slot does not have), is skipped and counted.

mod export;
mod import;
mod keys;
#[cfg(test)]
mod support;

pub use export::*;
pub use import::*;
pub(crate) use keys::*;

use serde::{Deserialize, Serialize};

use crate::vj_mapping::{ButtonMode, Follow, Link, Source, Target};

/// The `format` of the file.
pub const FORMAT: &str = "flux-vj-mappings";
/// The version this build writes and the newest it reads.
pub const VERSION: u32 = 1;

/// What the user decided about the mappings an import would overwrite. The default (nothing
/// wiped, nothing kept) merges: an incoming mapping replaces the one with the same source or the
/// same target, and the rest of the show's are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportChoice {
    /// Throws away the show's mappings and dashboard links first: the file takes their place.
    pub wipe: bool,
    /// The conflicts to leave as they are, by their [`Conflict::index`]; the incoming mapping
    /// is not imported.
    pub keep: Vec<usize>,
}

/// What an import did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    /// Left out because their place is not in this show, or they no longer fit.
    pub skipped: usize,
    /// Conflicts the user chose to keep as they were.
    pub kept: usize,
}

/// An incoming mapping that would overwrite one the show has.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    /// The place of the mapping in the file, which [`ImportChoice::keep`] names.
    pub index: usize,
    pub source: Source,
    /// What the show's mapping drives now.
    pub existing: Target,
    /// What the file's would drive, in this show.
    pub incoming: Target,
}

/// What reading a file into a show would do, before it is done.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportPlan {
    /// Mappings and dashboard links that resolve in this show.
    pub taken: usize,
    /// Those that do not.
    pub skipped: usize,
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMapping {
    source: Source,
    /// A [`Target`] whose ids are positions.
    target: Target,
    /// The same target by id, for a file read back into the show it came from; where the ids are
    /// not there, the positions are used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ids: Option<Target>,
    #[serde(default)]
    follow: Follow,
    min: f32,
    max: f32,
    #[serde(default)]
    invert: bool,
    #[serde(default)]
    mode: ButtonMode,
    /// The pad colours of this mapping; files written before them have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lights: Option<crate::vj_feedback::Lights>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileKnob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
}

/// The content of a `.flxmap` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapFile {
    format: String,
    version: u32,
    #[serde(default)]
    mappings: Vec<FileMapping>,
    #[serde(default)]
    dashboard: Vec<FileKnob>,
}
