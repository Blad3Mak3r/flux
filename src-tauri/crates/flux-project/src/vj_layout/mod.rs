//! The dockable layout of the VJ screen, kept per machine (see `src-tauri/src/vj_layout_commands.rs`),
//! not in the show. The screen is a tree: splits (a row or a column of children with their shares),
//! tab groups of panels, and one fixed leaf, the deck (clip grid with its crossfader), which can be
//! docked around but never moved, closed or tabbed. The BPM bar is not in the tree: the frontend
//! draws it fixed under the deck. Every operation here validates, applies and normalises; the
//! frontend only draws the tree and forwards intent.

use crate::vj_presets::{invalid, name_of, write_atomic};
use crate::{ProjectError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

mod factory;
mod layout;
mod store;
mod tree;

pub use self::factory::factory;
pub use self::store::{FactoryInfo, Saved, SavedInfo, State, Store};
use self::tree::*;
pub use self::tree::{DECK_MIN_SIZE, min_size, panel_min_size};

/// The version of the layout and of the store this build writes; a file with another one falls back
/// to the default layout.
pub const VERSION: u32 = 1;

/// The most monitors that can be open at once.
pub const MAX_MONITORS: usize = 8;

/// The most named layouts the user keeps.
pub const MAX_SAVED: usize = 50;

/// The gap a splitter takes between two children, in px.
pub const SPLITTER_GAP: u32 = 4;

/// The id that addresses the deck as a drop target.
pub const DECK: &str = "deck";

/// The panels that exist once each. Monitors are `monitor:<uuid>`.
pub const FIXED_PANELS: [&str; 9] = [
    "composition",
    "group",
    "layer",
    "clip",
    "mappings",
    "dashboard",
    "files",
    "generators",
    "effects",
];

/// The ids of the built-in layouts.
pub const FACTORY: [&str; 3] = ["default", "performance", "monitors"];

const MONITOR_PREFIX: &str = "monitor:";

/// A panel: a fixed kind by its name, or `monitor:<uuid>`.
pub type PanelId = String;

/// How big the clips of the grid are drawn (Resolume's clip size). Kept per machine with the
/// layouts, not in a layout: it stays when another layout is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl ClipSize {
    fn is_medium(&self) -> bool {
        *self == Self::Medium
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dir {
    Row,
    Column,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Node {
    Split {
        id: String,
        dir: Dir,
        children: Vec<Node>,
        /// Fractions of the split, summing to 1.
        sizes: Vec<f32>,
    },
    Tabs {
        id: String,
        panels: Vec<PanelId>,
        active: usize,
    },
    Deck,
}

/// What a monitor shows; `None` follows the selection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum MonitorKind {
    Composition,
    Preview,
    Layer {
        layer: Option<String>,
    },
    Group {
        group: Option<String>,
    },
    /// What a screen of the output shows, by its id.
    Screen {
        screen: String,
    },
}

impl MonitorKind {
    /// A screen monitor needs the id of a screen (whether it exists is the project's, so the
    /// monitor then shows that it is not found).
    fn bad_screen(&self) -> bool {
        matches!(self, Self::Screen { screen } if screen.is_empty() || screen.chars().count() > 200)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    /// The panel id, `monitor:<uuid>`.
    pub id: PanelId,
    pub kind: MonitorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Zone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

/// Where a panel is dropped: a tab group id or `"deck"`, and the zone of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub node: String,
    pub zone: Zone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub version: u32,
    pub root: Node,
    #[serde(default)]
    pub monitors: Vec<Monitor>,
    /// The tab group a closed panel was last in, so opening it again puts it back.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub last: BTreeMap<PanelId, String>,
}

impl Default for Layout {
    fn default() -> Self {
        factory("default").expect("the default layout exists")
    }
}

#[cfg(test)]
mod tests;
