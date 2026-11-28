//! The effects of a VJ grid: an ordered stack on each clip and on each layer, applied to the
//! clip's picture, and one more on the composition, applied to the final mix before the output
//! mapping. A clip's stack goes first and then its layer's, each from the top down; a bypassed
//! effect is skipped. The screens and the editor's monitor draw them (`flux-render`).
//!
//! Three kinds (feedback, trails and bloom) need the picture of the frames before or around the
//! pixel, so they only exist on the composition's stack: [`EffectSpec::composition_only`].
//!
//! Every kind is described once, in [`catalog`]: its category, and each setting with its range and
//! its default. The ops take their defaults and ranges from it, and the inspector draws its
//! controls from it, so no range lives in the interface.

mod catalog;
mod kind;
mod param;
mod validate;

pub use catalog::*;
pub use kind::*;
pub use param::*;
pub use validate::*;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::serde_util::is_false;
use crate::vj_modulation::Modulation;

/// Most effects one stack holds.
pub const MAX_EFFECTS: usize = 16;
/// The most and the least a zoom draws the picture larger.
pub const MAX_ZOOM: f32 = 8.0;
pub const MIN_ZOOM: f32 = 0.25;
/// The lengths, in beats, an effect synced to the tempo can take for one cycle.
pub const BEAT_CYCLES: [f32; 7] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0];

/// One effect of a stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    /// Unique among every effect of the project; it also seeds the effect's randomness.
    pub id: String,
    /// Skipped while drawing; the effect keeps its place and settings.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// What drives a numeric setting by itself (see [`crate::vj_modulation`]), by the setting's key
    /// in the [`catalog`]. The setting itself keeps the value the user set.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    #[serde(flatten)]
    pub kind: EffectKind,
}
