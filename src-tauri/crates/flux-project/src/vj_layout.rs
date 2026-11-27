//! The dockable layout of the VJ screen, kept per machine (see `src-tauri/src/vj_layout_commands.rs`),
//! not in the show. The screen is a tree: splits (a row or a column of children with their shares),
//! tab groups of panels, and one fixed leaf, the deck (clip grid with its crossfader), which can be
//! docked around but never moved, closed or tabbed. The BPM bar is not in the tree: the frontend
//! draws it fixed under the deck. Every operation here validates, applies and normalises; the
//! frontend only draws the tree and forwards intent.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::vj_presets::{invalid, name_of, write_atomic};
use crate::{ProjectError, Result};

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

fn fail<T>(text: impl Into<String>) -> Result<T> {
    Err(ProjectError::Invalid(text.into()))
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn is_monitor(panel: &str) -> bool {
    panel.starts_with(MONITOR_PREFIX)
}

/// The least size, in px, a panel is usable at.
pub fn panel_min_size(panel: &str) -> (u32, u32) {
    match panel {
        _ if is_monitor(panel) => (200, 150),
        "mappings" => (300, 200),
        "dashboard" => (260, 140),
        "files" | "generators" | "effects" => (240, 180),
        _ => (260, 180),
    }
}

/// The least size of the deck, in px.
pub const DECK_MIN_SIZE: (u32, u32) = (480, 260);

/// The least size of a subtree: a row sums widths and gaps and takes the tallest, a column the
/// converse, tabs the largest of their panels.
pub fn min_size(node: &Node) -> (u32, u32) {
    match node {
        Node::Deck => DECK_MIN_SIZE,
        Node::Tabs { panels, .. } => panels
            .iter()
            .map(|p| panel_min_size(p))
            .fold((0, 0), |(w, h), (pw, ph)| (w.max(pw), h.max(ph))),
        Node::Split { dir, children, .. } => {
            let gaps = SPLITTER_GAP * children.len().saturating_sub(1) as u32;
            let sizes = children.iter().map(min_size);
            match dir {
                Dir::Row => sizes.fold((gaps, 0), |(w, h), (cw, ch)| (w + cw, h.max(ch))),
                Dir::Column => sizes.fold((0, gaps), |(w, h), (cw, ch)| (w.max(cw), h + ch)),
            }
        }
    }
}

fn good_sizes(sizes: &[f32]) -> bool {
    sizes.iter().all(|s| s.is_finite() && *s > 0.0)
}

/// Drops empty tab groups, collapses single-child splits, merges a split into a parent of its own
/// direction, renormalises the shares and clamps `active`.
fn normalise(node: Node) -> Option<Node> {
    match node {
        Node::Deck => Some(Node::Deck),
        Node::Tabs { id, panels, active } => (!panels.is_empty()).then(|| Node::Tabs {
            id,
            active: active.min(panels.len() - 1),
            panels,
        }),
        Node::Split {
            id,
            dir,
            children,
            sizes,
        } => {
            let sizes = if sizes.len() == children.len() && good_sizes(&sizes) {
                sizes
            } else {
                vec![1.0; children.len()]
            };
            let mut out: Vec<(Node, f32)> = Vec::new();
            for (child, share) in children.into_iter().zip(sizes) {
                match normalise(child) {
                    Some(Node::Split {
                        dir: inner,
                        children,
                        sizes,
                        ..
                    }) if inner == dir => {
                        out.extend(
                            children
                                .into_iter()
                                .zip(sizes.into_iter().map(|s| s * share)),
                        );
                    }
                    Some(other) => out.push((other, share)),
                    None => {}
                }
            }
            if out.len() <= 1 {
                return out.pop().map(|(node, _)| node);
            }
            let total: f32 = out.iter().map(|(_, s)| s).sum();
            let (children, sizes) = out.into_iter().map(|(n, s)| (n, s / total)).unzip();
            Some(Node::Split {
                id,
                dir,
                children,
                sizes,
            })
        }
    }
}

fn tabs_mut<'a>(node: &'a mut Node, group: &str) -> Option<(&'a mut Vec<PanelId>, &'a mut usize)> {
    match node {
        Node::Tabs { id, panels, active } if id == group => Some((panels, active)),
        Node::Split { children, .. } => children.iter_mut().find_map(|c| tabs_mut(c, group)),
        _ => None,
    }
}

fn collect<'a>(node: &'a Node, tabs: &mut Vec<(&'a str, &'a [PanelId])>, decks: &mut usize) {
    match node {
        Node::Deck => *decks += 1,
        Node::Tabs { id, panels, .. } => tabs.push((id, panels)),
        Node::Split { children, .. } => children.iter().for_each(|c| collect(c, tabs, decks)),
    }
}

/// Replaces the node addressed by `target` (a tab group id or the deck) with `f` of it.
fn replace(node: &mut Node, target: &str, f: &mut Option<impl FnOnce(Node) -> Node>) {
    let here = match node {
        Node::Deck => target == DECK,
        Node::Tabs { id, .. } => id == target,
        Node::Split { children, .. } => {
            children.iter_mut().for_each(|c| replace(c, target, f));
            false
        }
    };
    if here && let Some(f) = f.take() {
        *node = f(std::mem::replace(node, Node::Deck));
    }
}

/// The area, as a fraction of the screen, of each tab group.
fn areas<'a>(node: &'a Node, share: f32, out: &mut Vec<(&'a str, f32)>) {
    match node {
        Node::Tabs { id, .. } => out.push((id, share)),
        Node::Split {
            children, sizes, ..
        } => children
            .iter()
            .zip(sizes)
            .for_each(|(c, s)| areas(c, share * s, out)),
        Node::Deck => {}
    }
}

impl Layout {
    fn tabs(&self) -> (Vec<(&str, &[PanelId])>, usize) {
        let (mut tabs, mut decks) = (Vec::new(), 0);
        collect(&self.root, &mut tabs, &mut decks);
        (tabs, decks)
    }

    /// The tab group holding `panel`.
    fn group_of(&self, panel: &str) -> Option<String> {
        let (tabs, _) = self.tabs();
        tabs.iter()
            .find(|(_, panels)| panels.iter().any(|p| p == panel))
            .map(|(id, _)| (*id).to_owned())
    }

    fn settle(&mut self) {
        let root = std::mem::replace(&mut self.root, Node::Deck);
        self.root = normalise(root).unwrap_or(Node::Deck);
    }

    /// Checks the tree and the monitors; the error says what is wrong.
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return fail(format!("layout version {}", self.version));
        }
        let (tabs, decks) = self.tabs();
        if decks != 1 {
            return fail(format!("a layout has one deck, this has {decks}"));
        }
        let mut seen: Vec<&str> = Vec::new();
        for (id, panels) in &tabs {
            if panels.is_empty() {
                return fail(format!("tab group {id} is empty"));
            }
            for p in *panels {
                let known =
                    FIXED_PANELS.contains(&p.as_str()) || self.monitors.iter().any(|m| &m.id == p);
                if !known || seen.contains(&p.as_str()) {
                    return fail(format!("panel {p} is unknown or twice"));
                }
                seen.push(p);
            }
        }
        if self.monitors.len() > MAX_MONITORS {
            return fail(format!("at most {MAX_MONITORS} monitors"));
        }
        if let Some(m) = self
            .monitors
            .iter()
            .find(|m| !is_monitor(&m.id) || !seen.contains(&m.id.as_str()))
        {
            return fail(format!("monitor {} is not in the layout", m.id));
        }
        if let Some(m) = self.monitors.iter().find(|m| m.kind.bad_screen()) {
            return fail(format!("monitor {} shows a screen with no id", m.id));
        }
        let mut ids: Vec<&str> = tabs.iter().map(|(id, _)| *id).collect();
        self.check(&self.root, &mut ids)
    }

    /// Splits have as many sizes as children, positive, and their ids are unique.
    fn check<'a>(&'a self, node: &'a Node, ids: &mut Vec<&'a str>) -> Result<()> {
        if let Node::Split {
            id,
            children,
            sizes,
            ..
        } = node
        {
            if sizes.len() != children.len() || children.len() < 2 || !good_sizes(sizes) {
                return fail(format!("split {id} has bad sizes"));
            }
            if ids.contains(&id.as_str()) {
                return fail(format!("id {id} is twice"));
            }
            ids.push(id);
            children.iter().try_for_each(|c| self.check(c, ids))?;
        }
        Ok(())
    }

    /// Reads a layout; a file that is not one, is invalid or is of another version gives the default.
    pub fn parse(text: &str) -> Self {
        serde_json::from_str::<Self>(text)
            .ok()
            .filter(|layout| layout.validate().is_ok())
            .unwrap_or_default()
    }

    /// Moves a panel to a tab group (centre) or beside a tab group or the deck (an edge).
    pub fn move_panel(&mut self, panel: &str, target: &Target) -> Result<()> {
        let Some(from) = self.group_of(panel) else {
            return fail(format!("panel {panel} is not open"));
        };
        let (tabs, _) = self.tabs();
        let group = tabs.iter().find(|(id, _)| *id == target.node);
        if target.node != DECK && group.is_none() {
            return fail(format!("no dock {}", target.node));
        }
        if target.zone == Zone::Center && target.node == DECK {
            return fail("the deck takes no tabs");
        }
        if target.node == from
            && (target.zone == Zone::Center || group.is_some_and(|g| g.1.len() == 1))
        {
            return Ok(());
        }
        self.take(panel);
        if target.zone == Zone::Center {
            let (panels, active) = tabs_mut(&mut self.root, &target.node).expect("checked above");
            panels.push(panel.to_owned());
            *active = panels.len() - 1;
        } else {
            self.dock(panel, target);
        }
        self.settle();
        Ok(())
    }

    fn take(&mut self, panel: &str) {
        if let Some(group) = self.group_of(panel) {
            let (panels, active) = tabs_mut(&mut self.root, &group).expect("it was found");
            panels.retain(|p| p != panel);
            *active = (*active).min(panels.len().saturating_sub(1));
        }
    }

    fn dock(&mut self, panel: &str, target: &Target) {
        let (dir, before) = match target.zone {
            Zone::Left => (Dir::Row, true),
            Zone::Right => (Dir::Row, false),
            Zone::Top => (Dir::Column, true),
            _ => (Dir::Column, false),
        };
        let new = Node::Tabs {
            id: new_id(),
            panels: vec![panel.to_owned()],
            active: 0,
        };
        replace(
            &mut self.root,
            &target.node,
            &mut Some(|old: Node| {
                let children = if before {
                    vec![new, old]
                } else {
                    vec![old, new]
                };
                Node::Split {
                    id: new_id(),
                    dir,
                    children,
                    sizes: vec![0.5, 0.5],
                }
            }),
        );
    }

    pub fn reorder_tab(&mut self, group: &str, from: usize, to: usize) -> Result<()> {
        let Some((panels, active)) = tabs_mut(&mut self.root, group) else {
            return fail(format!("no dock {group}"));
        };
        if from >= panels.len() || to >= panels.len() {
            return fail("tab out of range");
        }
        let panel = panels.remove(from);
        panels.insert(to, panel);
        *active = to;
        Ok(())
    }

    pub fn activate(&mut self, group: &str, index: usize) -> Result<()> {
        match tabs_mut(&mut self.root, group) {
            Some((panels, active)) if index < panels.len() => {
                *active = index;
                Ok(())
            }
            Some(_) => fail("tab out of range"),
            None => fail(format!("no dock {group}")),
        }
    }

    /// Closes a panel; a monitor loses its settings. The deck cannot be closed.
    pub fn close_panel(&mut self, panel: &str) -> Result<()> {
        let Some(group) = self.group_of(panel) else {
            return fail(if panel == DECK {
                "the deck cannot be closed".to_owned()
            } else {
                format!("panel {panel} is not open")
            });
        };
        self.take(panel);
        if is_monitor(panel) {
            self.monitors.retain(|m| m.id != panel);
        } else {
            self.last.insert(panel.to_owned(), group);
        }
        self.settle();
        Ok(())
    }

    /// Opens a closed fixed panel, or a new monitor of `monitor` when `panel` is `"monitor"`;
    /// answers with the panel's id. It goes back to the group it was last in, else into the
    /// largest group, else right of the deck. A panel already open stays where it is.
    pub fn open_panel(&mut self, panel: &str, monitor: Option<MonitorKind>) -> Result<PanelId> {
        let id = if panel == "monitor" {
            if self.monitors.len() >= MAX_MONITORS {
                return fail(format!("at most {MAX_MONITORS} monitors"));
            }
            if monitor.as_ref().is_some_and(MonitorKind::bad_screen) {
                return fail("a screen monitor needs a screen");
            }
            let id = format!("{MONITOR_PREFIX}{}", new_id());
            self.monitors.push(Monitor {
                id: id.clone(),
                kind: monitor.unwrap_or(MonitorKind::Preview),
            });
            id
        } else if FIXED_PANELS.contains(&panel) {
            if self.group_of(panel).is_some() {
                return Ok(panel.to_owned());
            }
            panel.to_owned()
        } else {
            return fail(format!("no panel {panel}"));
        };
        let remembered = self.last.remove(&id);
        let group = {
            let mut best = Vec::new();
            areas(&self.root, 1.0, &mut best);
            let known = self
                .tabs()
                .0
                .iter()
                .any(|(g, _)| Some(*g) == remembered.as_deref());
            if known {
                remembered
            } else {
                best.iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(g, _)| (*g).to_owned())
            }
        };
        match group {
            Some(group) => {
                let (panels, active) = tabs_mut(&mut self.root, &group).expect("it exists");
                panels.push(id.clone());
                *active = panels.len() - 1;
            }
            None => self.dock(
                &id,
                &Target {
                    node: DECK.into(),
                    zone: Zone::Right,
                },
            ),
        }
        self.settle();
        Ok(id)
    }

    /// Sets the shares of a split, then renormalises them.
    pub fn resize(&mut self, split: &str, new: Vec<f32>) -> Result<()> {
        fn find<'a>(node: &'a mut Node, split: &str) -> Option<(usize, &'a mut Vec<f32>)> {
            let Node::Split {
                id,
                children,
                sizes,
                ..
            } = node
            else {
                return None;
            };
            if id == split {
                return Some((children.len(), sizes));
            }
            children.iter_mut().find_map(|c| find(c, split))
        }
        let Some((count, sizes)) = find(&mut self.root, split) else {
            return fail(format!("no split {split}"));
        };
        if new.len() != count || !good_sizes(&new) {
            return fail("sizes must be positive, one per child");
        }
        let total: f32 = new.iter().sum();
        *sizes = new.into_iter().map(|s| s / total).collect();
        Ok(())
    }

    pub fn set_monitor(&mut self, id: &str, kind: MonitorKind) -> Result<()> {
        if kind.bad_screen() {
            return fail("a screen monitor needs a screen");
        }
        match self.monitors.iter_mut().find(|m| m.id == id) {
            Some(monitor) => {
                monitor.kind = kind;
                Ok(())
            }
            None => fail(format!("no monitor {id}")),
        }
    }
}

// ---- The built-in layouts ----

struct Build {
    monitors: Vec<Monitor>,
}

impl Build {
    fn monitor(&mut self, kind: MonitorKind) -> Node {
        let id = format!("{MONITOR_PREFIX}{}", new_id());
        self.monitors.push(Monitor {
            id: id.clone(),
            kind,
        });
        tabs(&[&id])
    }
}

fn tabs(panels: &[&str]) -> Node {
    Node::Tabs {
        id: new_id(),
        panels: panels.iter().map(|p| (*p).to_owned()).collect(),
        active: 0,
    }
}

fn split(dir: Dir, parts: Vec<(Node, f32)>) -> Node {
    let (children, sizes) = parts.into_iter().unzip();
    Node::Split {
        id: new_id(),
        dir,
        children,
        sizes,
    }
}

/// A built-in layout by id (`default`, `performance`, `monitors`), with fresh ids.
pub fn factory(id: &str) -> Option<Layout> {
    use Dir::{Column, Row};
    let mut b = Build { monitors: vec![] };
    let browsers = || tabs(&["files", "generators", "effects"]);
    let root = match id {
        "default" => {
            let monitors = split(
                Column,
                vec![
                    (b.monitor(MonitorKind::Composition), 0.5),
                    (b.monitor(MonitorKind::Preview), 0.5),
                ],
            );
            split(
                Column,
                vec![
                    (split(Row, vec![(Node::Deck, 0.75), (monitors, 0.25)]), 0.6),
                    (
                        split(
                            Row,
                            vec![
                                (
                                    tabs(&[
                                        "clip",
                                        "layer",
                                        "group",
                                        "composition",
                                        "dashboard",
                                        "mappings",
                                    ]),
                                    0.65,
                                ),
                                (browsers(), 0.35),
                            ],
                        ),
                        0.4,
                    ),
                ],
            )
        }
        "performance" => {
            let top = split(
                Row,
                vec![
                    (b.monitor(MonitorKind::Composition), 1.0),
                    (b.monitor(MonitorKind::Layer { layer: None }), 1.0),
                    (b.monitor(MonitorKind::Preview), 1.0),
                ],
            );
            let bottom = split(
                Row,
                vec![
                    (tabs(&["composition", "group"]), 0.25),
                    (tabs(&["layer"]), 0.2),
                    (tabs(&["clip"]), 0.25),
                    (browsers(), 0.3),
                ],
            );
            split(Column, vec![(top, 0.28), (Node::Deck, 0.4), (bottom, 0.32)])
        }
        "monitors" => {
            let top = split(
                Row,
                vec![
                    (b.monitor(MonitorKind::Composition), 1.0),
                    (b.monitor(MonitorKind::Layer { layer: None }), 1.0),
                    (b.monitor(MonitorKind::Group { group: None }), 1.0),
                    (b.monitor(MonitorKind::Preview), 1.0),
                ],
            );
            let bottom = split(
                Row,
                vec![(tabs(&["clip", "layer"]), 0.5), (browsers(), 0.5)],
            );
            split(Column, vec![(top, 0.25), (Node::Deck, 0.4), (bottom, 0.35)])
        }
        _ => return None,
    };
    let mut layout = Layout {
        version: VERSION,
        root,
        monitors: b.monitors,
        last: BTreeMap::new(),
    };
    layout.settle();
    Some(layout)
}

// ---- What is kept on the machine ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub id: String,
    pub name: String,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactoryInfo {
    pub id: &'static str,
}

/// What the UI shows: the current layout and the names it can switch to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct State {
    pub current: Layout,
    pub saved: Vec<SavedInfo>,
    pub factory: Vec<FactoryInfo>,
    /// The least `(width, height)` in px of every split, tab group and `"deck"`, so the UI clamps
    /// a splitter drag without knowing the rules.
    pub mins: BTreeMap<String, (u32, u32)>,
    /// The gap a splitter takes between two children, in px.
    pub gap: u32,
}

fn collect_mins(node: &Node, out: &mut BTreeMap<String, (u32, u32)>) {
    let id = match node {
        Node::Deck => DECK,
        Node::Tabs { id, .. } => id,
        Node::Split { id, children, .. } => {
            children.iter().for_each(|c| collect_mins(c, out));
            id
        }
    };
    out.insert(id.to_owned(), min_size(node));
}

/// The file `vj-layout.json`: the current layout and the user's named ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub version: u32,
    pub current: Layout,
    #[serde(default)]
    pub saved: Vec<Saved>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: VERSION,
            current: Layout::default(),
            saved: Vec::new(),
        }
    }
}

impl Store {
    /// Reads the store; a file that is invalid or newer gives the default one, never an error.
    pub fn parse(text: &str) -> Self {
        let Ok(mut store) = serde_json::from_str::<Self>(text) else {
            return Self::default();
        };
        if store.version != VERSION || store.current.validate().is_err() {
            return Self::default();
        }
        store.saved.retain(|s| s.layout.validate().is_ok());
        store
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(invalid)?)
    }

    pub fn state(&self) -> State {
        let mut mins = BTreeMap::new();
        collect_mins(&self.current.root, &mut mins);
        State {
            mins,
            gap: SPLITTER_GAP,
            current: self.current.clone(),
            saved: self
                .saved
                .iter()
                .map(|s| SavedInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                })
                .collect(),
            factory: FACTORY.iter().map(|id| FactoryInfo { id }).collect(),
        }
    }

    /// Makes a built-in or saved layout the current one.
    pub fn apply(&mut self, id: &str) -> Result<()> {
        let layout = factory(id).or_else(|| {
            self.saved
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.layout.clone())
        });
        match layout {
            Some(layout) => {
                self.current = layout;
                Ok(())
            }
            None => fail(format!("no layout {id}")),
        }
    }

    /// Keeps the current layout under `name`; the same name is overwritten.
    pub fn save_as(&mut self, name: &str) -> Result<()> {
        let name = name_of(name)?;
        let layout = self.current.clone();
        let full = self.saved.len() >= MAX_SAVED;
        match self.saved.iter_mut().find(|s| s.name == name) {
            Some(saved) => saved.layout = layout,
            None if full => return fail(format!("at most {MAX_SAVED} layouts")),
            None => self.saved.push(Saved {
                id: new_id(),
                name,
                layout,
            }),
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = name_of(name)?;
        if self.saved.iter().any(|s| s.id != id && s.name == name) {
            return fail("a layout has that name");
        }
        match self.saved.iter_mut().find(|s| s.id == id) {
            Some(saved) => {
                saved.name = name;
                Ok(())
            }
            None => fail(format!("no saved layout {id}")),
        }
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let before = self.saved.len();
        self.saved.retain(|s| s.id != id);
        if self.saved.len() == before {
            return fail(format!("no saved layout {id}"));
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.current = Layout::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(node: &str, zone: Zone) -> Target {
        Target {
            node: node.into(),
            zone,
        }
    }

    fn group(l: &Layout, panel: &str) -> String {
        l.group_of(panel).expect("open")
    }

    fn monitor_ids(l: &Layout) -> Vec<String> {
        l.monitors.iter().map(|m| m.id.clone()).collect()
    }

    #[test]
    fn factories_validate() {
        for id in FACTORY {
            let l = factory(id).unwrap();
            l.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        assert!(factory("nope").is_none());
        assert_eq!(factory("monitors").unwrap().monitors.len(), 4);
        // The default has every fixed panel once.
        let l = Layout::default();
        for p in FIXED_PANELS {
            assert!(l.group_of(p).is_some(), "{p}");
        }
    }

    #[test]
    fn move_as_tab_and_noop() {
        let mut l = Layout::default();
        let files = group(&l, "files");
        l.move_panel("clip", &target(&files, Zone::Center)).unwrap();
        assert_eq!(group(&l, "clip"), files);
        l.validate().unwrap();
        // Alone on its own group's centre: nothing changes.
        let mut l = Layout::default();
        let mon = monitor_ids(&l)[0].clone();
        let g = group(&l, &mon);
        let before = l.clone();
        l.move_panel(&mon, &target(&g, Zone::Center)).unwrap();
        l.move_panel(&mon, &target(&g, Zone::Left)).unwrap();
        assert_eq!(l, before);
        assert!(l.move_panel("clip", &target(DECK, Zone::Center)).is_err());
        assert!(l.move_panel("clip", &target("zzz", Zone::Left)).is_err());
        assert!(l.move_panel("deck", &target(&g, Zone::Left)).is_err());
    }

    #[test]
    fn move_to_edges_of_group_and_deck() {
        let mut l = Layout::default();
        l.move_panel("mappings", &target(DECK, Zone::Top)).unwrap();
        l.validate().unwrap();
        let Node::Split { children, .. } = &l.root else {
            panic!()
        };
        let Node::Split { children, .. } = &children[0] else {
            panic!()
        };
        // The deck now sits under mappings, in a column of its own.
        let Node::Split { dir, children, .. } = &children[0] else {
            panic!()
        };
        assert_eq!(*dir, Dir::Column);
        assert_eq!(children[1], Node::Deck);
        let mut l = Layout::default();
        let files = group(&l, "files");
        l.move_panel("clip", &target(&files, Zone::Left)).unwrap();
        l.validate().unwrap();
        // Left of the browsers: same row, merged, sizes sum to 1.
        let Node::Split { children, .. } = &l.root else {
            panic!()
        };
        let Node::Split {
            children, sizes, ..
        } = &children[1]
        else {
            panic!()
        };
        assert_eq!(children.len(), 3);
        assert!((sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn emptied_group_is_dropped_and_split_collapses() {
        let mut l = factory("monitors").unwrap();
        let ids = monitor_ids(&l);
        for id in &ids[1..] {
            l.close_panel(id).unwrap();
        }
        l.validate().unwrap();
        assert_eq!(l.monitors.len(), 1);
        // Top row collapsed to a single group.
        let Node::Split { children, .. } = &l.root else {
            panic!()
        };
        assert!(matches!(&children[0], Node::Tabs { .. }));
    }

    #[test]
    fn reorder_activate() {
        let mut l = Layout::default();
        let g = group(&l, "clip");
        l.reorder_tab(&g, 0, 2).unwrap();
        let (panels, active) = tabs_mut(&mut l.root, &g).unwrap();
        assert_eq!(panels[2], "clip");
        assert_eq!(*active, 2);
        l.activate(&g, 1).unwrap();
        assert!(l.activate(&g, 99).is_err());
        assert!(l.reorder_tab(&g, 0, 99).is_err());
        assert!(l.activate("zzz", 0).is_err());
    }

    #[test]
    fn close_and_open_back() {
        let mut l = Layout::default();
        let g = group(&l, "layer");
        l.close_panel("layer").unwrap();
        assert!(l.group_of("layer").is_none());
        assert!(l.close_panel("layer").is_err());
        assert!(l.close_panel("deck").is_err());
        l.open_panel("layer", None).unwrap();
        assert_eq!(group(&l, "layer"), g);
        // Opening what is open changes nothing.
        let before = l.clone();
        l.open_panel("layer", None).unwrap();
        assert_eq!(l, before);
        assert!(l.open_panel("bogus", None).is_err());
        l.validate().unwrap();
    }

    #[test]
    fn open_goes_to_largest_group_when_unknown() {
        let mut l = Layout::default();
        l.close_panel("files").unwrap();
        l.last.clear();
        l.open_panel("files", None).unwrap();
        // The biggest group holds clip, layer... and now files.
        assert_eq!(group(&l, "files"), group(&l, "clip"));
    }

    #[test]
    fn open_with_no_group_docks_right_of_deck() {
        let mut l = Layout {
            version: VERSION,
            root: Node::Deck,
            monitors: vec![],
            last: BTreeMap::new(),
        };
        l.open_panel("clip", None).unwrap();
        l.validate().unwrap();
        assert!(matches!(&l.root, Node::Split { dir: Dir::Row, .. }));
    }

    #[test]
    fn monitors_open_close_limit() {
        let mut l = Layout::default();
        let id = l
            .open_panel("monitor", Some(MonitorKind::Composition))
            .unwrap();
        assert!(is_monitor(&id));
        l.validate().unwrap();
        while l.monitors.len() < MAX_MONITORS {
            l.open_panel("monitor", None).unwrap();
        }
        assert!(l.open_panel("monitor", None).is_err());
        l.validate().unwrap();
        l.close_panel(&id).unwrap();
        assert_eq!(l.monitors.len(), MAX_MONITORS - 1);
        assert!(l.monitors.iter().all(|m| m.id != id));
        l.validate().unwrap();
    }

    #[test]
    fn set_monitor_kind() {
        let mut l = Layout::default();
        let id = monitor_ids(&l)[0].clone();
        let kind = MonitorKind::Layer {
            layer: Some("a".into()),
        };
        l.set_monitor(&id, kind.clone()).unwrap();
        assert_eq!(l.monitors[0].kind, kind);
        assert!(l.set_monitor("monitor:none", kind).is_err());
    }

    #[test]
    fn screen_monitor_needs_a_screen_id() {
        let mut l = Layout::default();
        let id = monitor_ids(&l)[0].clone();
        let screen = |s: &str| MonitorKind::Screen { screen: s.into() };
        assert!(l.set_monitor(&id, screen("")).is_err());
        l.set_monitor(&id, screen("main")).unwrap();
        l.validate().unwrap();
        assert!(l.open_panel("monitor", Some(screen(""))).is_err());
        assert!(l.open_panel("monitor", Some(screen("s2"))).is_ok());
        let json = serde_json::to_string(&l.monitors[0].kind).unwrap();
        assert_eq!(json, r#"{"type":"screen","screen":"main"}"#);
        l.monitors[0].kind = screen("");
        assert!(l.validate().is_err());
    }

    #[test]
    fn resize_validates_and_renormalises() {
        let mut l = Layout::default();
        let Node::Split { id, .. } = l.root.clone() else {
            panic!()
        };
        l.resize(&id, vec![1.0, 3.0]).unwrap();
        let Node::Split { sizes, .. } = &l.root else {
            panic!()
        };
        assert_eq!(sizes, &vec![0.25, 0.75]);
        assert!(l.resize(&id, vec![1.0]).is_err());
        assert!(l.resize(&id, vec![1.0, 0.0]).is_err());
        assert!(l.resize(&id, vec![1.0, f32::NAN]).is_err());
        assert!(l.resize("zzz", vec![1.0, 1.0]).is_err());
    }

    #[test]
    fn min_sizes() {
        assert_eq!(panel_min_size("mappings"), (300, 200));
        assert_eq!(panel_min_size("monitor:x"), (200, 150));
        assert_eq!(min_size(&Node::Deck), (480, 260));
        assert_eq!(min_size(&tabs(&["mappings", "dashboard"])), (300, 200));
        let row = split(Dir::Row, vec![(Node::Deck, 0.5), (tabs(&["files"]), 0.5)]);
        assert_eq!(min_size(&row), (480 + 4 + 240, 260));
        let col = split(Dir::Column, vec![(row, 0.5), (tabs(&["clip"]), 0.5)]);
        assert_eq!(min_size(&col), (724, 260 + 4 + 180));
    }

    #[test]
    fn state_lists_the_min_of_every_node() {
        let state = Store::default().state();
        assert_eq!(state.gap, SPLITTER_GAP);
        assert_eq!(state.mins["deck"], DECK_MIN_SIZE);
        let Node::Split { id, .. } = &state.current.root else {
            panic!("the default layout starts with a split");
        };
        assert_eq!(state.mins[id], min_size(&state.current.root));
    }

    #[test]
    fn invalid_or_newer_falls_back() {
        let default = Layout::default().root;
        let same = |l: Layout| {
            matches!(
                (&l.root, &default),
                (Node::Split { .. }, Node::Split { .. })
            )
        };
        assert!(same(Layout::parse("not json")));
        let mut newer = factory("monitors").unwrap();
        newer.version = VERSION + 1;
        assert_eq!(
            Layout::parse(&serde_json::to_string(&newer).unwrap())
                .monitors
                .len(),
            2
        );
        let mut twice = factory("monitors").unwrap();
        twice.monitors.pop();
        assert_eq!(
            Layout::parse(&serde_json::to_string(&twice).unwrap())
                .monitors
                .len(),
            2
        );
        let ok = factory("performance").unwrap();
        assert_eq!(Layout::parse(&serde_json::to_string(&ok).unwrap()), ok);
        // Two decks are invalid.
        let mut two = Layout::default();
        two.root = split(Dir::Row, vec![(Node::Deck, 1.0), (two.root.clone(), 1.0)]);
        assert!(two.validate().is_err());
    }

    #[test]
    fn store_saves_applies_renames_deletes() {
        let mut s = Store::default();
        s.current.close_panel("clip").unwrap();
        s.save_as(" mine ").unwrap();
        s.save_as("mine").unwrap();
        assert_eq!(s.saved.len(), 1);
        assert!(s.save_as("  ").is_err());
        let id = s.saved[0].id.clone();
        s.apply("default").unwrap();
        assert!(s.current.group_of("clip").is_some());
        s.apply(&id).unwrap();
        assert!(s.current.group_of("clip").is_none());
        s.save_as("other").unwrap();
        let other = s.saved[1].id.clone();
        assert!(s.rename(&other, "mine").is_err());
        s.rename(&other, "second").unwrap();
        assert!(s.apply("zzz").is_err());
        s.delete(&other).unwrap();
        assert!(s.delete(&other).is_err());
        s.reset();
        assert!(s.current.group_of("clip").is_some());
        let state = s.state();
        assert_eq!(state.factory.len(), 3);
        assert_eq!(state.saved.len(), 1);
    }

    #[test]
    fn store_file_round_trip_and_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vj-layout.json");
        assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
        let mut s = Store::default();
        s.save_as("x").unwrap();
        s.save(&path).unwrap();
        assert_eq!(Store::load(&path).unwrap(), s);
        std::fs::write(&path, "{ broken").unwrap();
        assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
        let mut newer = s.clone();
        newer.version = VERSION + 1;
        std::fs::write(&path, serde_json::to_string(&newer).unwrap()).unwrap();
        assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
    }
}
