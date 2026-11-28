//! The VJ grid of a project, played like a media server's clip deck: layers stacked one over the
//! other, columns across them, and in each cell at most one clip, a video the user launches by hand.
//! A project is of one kind for good, chosen when it is created: a timeline (cue) project has no
//! grid, and a VJ project plays its grid. The two are apps of their own in one: a VJ project holds
//! none of a cue project's content (no tracks, setlists or timecode outputs), only one timeline that
//! carries the tempo and timecode of its clock.
//!
//! The columns come in decks: sets of columns of their own, one of which the grid shows. The layers
//! are the same in every deck, and a deck holds the clips its columns have on each layer. A global
//! layer keeps its clips in slots of its own instead, the same in every deck, so what it plays (a
//! logo, a mask) does not change with the deck shown, and launching a column leaves it alone.
//!
//! The layers are listed in drawing order: the first is at the bottom, the last on top. The levels,
//! blend, transition and crossfader side of a layer are stored with the project; which clip is
//! playing is not, it belongs to the show running.

pub use crate::generator::Generator;
use crate::model::{MAX_BPM, MAX_CLIP_NAME, MIN_BPM, Project, Timeline, VideoFit};
pub use crate::vj_dashboard::Scope;
pub use crate::vj_effects::{
    Effect, EffectKind, EffectSpec, EffectTarget, MAX_EFFECTS, MirrorAxis, ParamSpec, Sync,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::serde_util::{full, is_false, is_full, is_zero};
pub use crate::vj_groups::{Group, MAX_GROUPS};
pub use crate::vj_mapping::{ButtonMode, Knob, Mapping, Source, Target};
pub use crate::vj_modulation::{ModTarget, Modulation};
use crate::{ProjectError, Result};

mod apply;
mod clip;
mod grid;
mod layer;
mod op;
mod validate;

pub use self::apply::apply;
pub(crate) use self::clip::new_clip;
use self::clip::*;
pub use self::clip::{
    CUES, Clip, DEFAULT_DURATION, Direction, MAX_DURATION, MAX_OFFSET, MAX_SCALE, MAX_SPEED,
    MIN_DURATION, MIN_REGION, MIN_SPEED, Playback, Region, TEMPO_BEATS, Tempo,
};
use self::grid::*;
pub use self::grid::{ClockSource, Column, Deck, Vj, tapped_bpm};
pub use self::layer::{Autopilot, Blend, Layer, Side, Trigger};
pub use self::op::{Level, VjOp};
pub(crate) use self::validate::{check_name, validate};

/// Most layers a grid has.
pub const MAX_LAYERS: usize = 16;

/// Most columns a deck has, and most slots a global layer has.
pub const MAX_COLUMNS: usize = 64;

/// Most decks a grid has.
pub const MAX_DECKS: usize = 32;

/// The longest transition between two clips of a layer, in seconds.
pub const MAX_TRANSITION: f32 = 10.0;

/// The grid a new VJ project starts with.
pub const DEFAULT_LAYERS: usize = 3;
pub const DEFAULT_COLUMNS: usize = 8;

/// The kind of a project, chosen when it is created and never changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectMode {
    /// The timelines, their tracks and cues, and the timecode.
    #[default]
    Timeline,
    /// The VJ grid.
    Vj,
}

impl ProjectMode {
    pub(crate) fn is_timeline(&self) -> bool {
        *self == Self::Timeline
    }
}

#[cfg(test)]
mod tests;
