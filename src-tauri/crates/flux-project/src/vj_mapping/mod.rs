//! MIDI and OSC mapping of the VJ grid, and the composition dashboard.
//!
//! A [`Mapping`] ties a [`Source`] (a MIDI control of a port, or an OSC address, as the user
//! learned it) to a [`Target`] (a clip, a layer, an effect setting... named by id, never by place).
//! Everything here is pure, so it is tested without a controller or a network: [`react`] turns what
//! was heard into the [`Command`]s of the mappings it matches, [`plan`] turns a command into the
//! edit of the project ([`VjOp`]) or the playing action the app runs, and [`osc_command`] reads the
//! fixed Resolume-style addresses. The app (`src-tauri/src/vj_control.rs`) only carries them out.
//!
//! A target is one of three classes. A **continuous** one (a fader, a setting) is scaled from the
//! 0 to 1 of the source into the mapping's own range, inverted or not. A **switch** (a bypass)
//! is flipped by a press, or follows the button while it is held. An **action** (launch, stop,
//! tap) runs on a press, and a few also on the release (a piano clip stops, a nudge ends).
//!
//! The dashboard is eight knobs stored with the grid; turning one moves every number linked to
//! it, each into the range of its link, in a single edit.

use crate::events::OscArg;
use crate::model::{MAX_BPM, MIN_BPM};
use crate::serde_util::is_false;
use crate::vj::{MAX_SPEED, MAX_TRANSITION, MIN_SPEED, Vj, VjOp};
use crate::vj_dashboard::Scope;
use crate::{ProjectError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

mod catalog;
mod edit;
mod model;
mod osc;
mod react;

pub use self::catalog::{TargetKind, target_kinds};
pub use self::edit::prune;
pub(crate) use self::edit::{
    add_mapping, check_knob, drive, drive_knob, finite, link_knob, linkable, rebind,
    remove_mapping, rename_knob, set_mapping, set_mapping_fine, set_mapping_follow,
    set_mapping_lights, validate,
};
pub use self::model::{
    ButtonMode, Class, DMX_CHANNELS, Follow, Knob, Link, MAX_UNIVERSE, Mapping, MidiKind, Source,
    Target,
};
pub use self::osc::{osc_command, osc_level};
pub use self::react::{
    Command, Edge, Heard, Plan, PlayAction, coalesce, command, edge, plan, react, value_op,
};

/// Most mappings a grid keeps.
pub const MAX_MAPPINGS: usize = 512;

/// How many knobs the dashboard has.
pub const KNOBS: usize = 8;

/// Most numbers a knob drives.
pub const MAX_LINKS: usize = 16;

/// Longest OSC address a mapping keeps.
const MAX_ADDRESS: usize = 256;

#[cfg(test)]
mod tests;
