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

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::events::OscArg;
use crate::model::{MAX_BPM, MIN_BPM, Project};
use crate::vj::{MAX_SPEED, MAX_TRANSITION, MIN_SPEED, Vj, VjOp};
use crate::{ProjectError, Result};

/// Most mappings a grid keeps.
pub const MAX_MAPPINGS: usize = 512;
/// How many knobs the dashboard has.
pub const KNOBS: usize = 8;
/// Most numbers a knob drives.
pub const MAX_LINKS: usize = 16;
/// Longest OSC address a mapping keeps.
const MAX_ADDRESS: usize = 256;

// ---------------------------------------------------------------------------------------------
// The model

/// The kind of MIDI message a control sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MidiKind {
    /// A control change: a knob, a fader or a button.
    Cc,
    /// A pad or a key: 0 is released.
    Note,
    /// A wheel or a touch strip, 14 bits; its number is 0.
    PitchBend,
}

/// What drives a target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Source {
    Midi {
        /// A hint of the name of the input port the control was learned on, as the project keeps
        /// those of its outputs; absent matches any port.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        /// From 1 to 16.
        channel: u8,
        kind: MidiKind,
        /// The control, note or program number, 0 to 127.
        number: u8,
    },
    Osc {
        address: String,
    },
    /// A DMX channel of an Art-Net universe, as a console sends it: 0 to 255, or 0 to 65535 with
    /// a `fine` channel beside it.
    Artnet {
        /// The 15-bit Port-Address (net, sub-net and universe), as on the wire.
        universe: u16,
        /// From 1 to 512.
        channel: u16,
        /// The channel of the low byte of a 16-bit value, from 1 to 512; absent is 8 bits.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fine: Option<u16>,
    },
    /// A key of the computer's keyboard, by its position (`KeyboardEvent.code`, whatever the
    /// layout), with the modifiers held.
    Key {
        code: String,
        #[serde(default, skip_serializing_if = "is_false")]
        ctrl: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        alt: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        shift: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        meta: bool,
    },
}

/// Most channels a DMX universe has.
pub const DMX_CHANNELS: u16 = 512;
/// The largest Port-Address of Art-Net (15 bits).
pub const MAX_UNIVERSE: u16 = 0x7FFF;
/// Longest key code a mapping keeps.
const MAX_CODE: usize = 32;

/// A key the app uses itself: `cmd` is Ctrl (Cmd on macOS); the file's `action` is the window's.
#[derive(Deserialize)]
struct AppShortcut {
    code: String,
    cmd: bool,
    shift: bool,
}

/// The list the window reads too, so the menus, the keys and the refusal cannot drift apart.
fn app_shortcuts() -> &'static [AppShortcut] {
    static LIST: OnceLock<Vec<AppShortcut>> = OnceLock::new();
    LIST.get_or_init(|| serde_json::from_str(include_str!("vj_shortcuts.json")).unwrap_or_default())
}

impl Source {
    /// Whether this is a key the app uses itself, so a mapping would fight its shortcuts: one of
    /// the combinations in `vj_shortcuts.json` (the list the window reads too; Resolume's
    /// defaults: Ctrl or Cmd with N, O, S, Z, Y, K, L, T, Ctrl or Cmd and Shift with S, C, K, M,
    /// O, X, A, D, L, P, Shift+1 to 9 and Esc), or a modifier alone. Every other key is free.
    pub fn is_app_shortcut(&self) -> bool {
        let Self::Key {
            code,
            ctrl,
            alt,
            shift,
            meta,
        } = self
        else {
            return false;
        };
        if ["Control", "Meta", "Shift", "Alt", "OS"]
            .iter()
            .any(|modifier| code.starts_with(modifier))
        {
            return true;
        }
        !*alt
            && app_shortcuts()
                .iter()
                .any(|s| s.code == *code && s.cmd == (*ctrl || *meta) && s.shift == *shift)
    }

    /// Whether what was heard (`heard`, with the port it came from) is this source. A mapping
    /// without a port hint, or a message without a port, matches any port; otherwise the names
    /// agree, ignoring case.
    pub fn matches(&self, heard: &Source) -> bool {
        match (self, heard) {
            (
                Self::Midi {
                    port,
                    channel,
                    kind,
                    number,
                },
                Self::Midi {
                    port: heard_port,
                    channel: heard_channel,
                    kind: heard_kind,
                    number: heard_number,
                },
            ) => {
                channel == heard_channel
                    && kind == heard_kind
                    && number == heard_number
                    && match (port, heard_port) {
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        _ => true,
                    }
            }
            (Self::Osc { address }, Self::Osc { address: heard }) => address == heard,
            // A DMX or key source is heard as exactly itself.
            (a @ Self::Artnet { .. }, b @ Self::Artnet { .. })
            | (a @ Self::Key { .. }, b @ Self::Key { .. }) => a == b,
            _ => false,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Midi {
                channel,
                kind,
                number,
                port,
            } => {
                let number_ok = match kind {
                    MidiKind::PitchBend => *number == 0,
                    _ => *number < 128,
                };
                if !(1..=16).contains(channel)
                    || !number_ok
                    || port.as_ref().is_some_and(|p| p.trim().is_empty())
                {
                    return Err(ProjectError::Invalid(
                        "a MIDI source needs a channel of 1 to 16 and a number of 0 to 127".into(),
                    ));
                }
            }
            Self::Osc { address } => {
                if !address.starts_with('/') || address.len() > MAX_ADDRESS {
                    return Err(ProjectError::Invalid(
                        "an OSC source is an address that starts with /".into(),
                    ));
                }
            }
            Self::Artnet {
                universe,
                channel,
                fine,
            } => {
                let channel_ok = |c: u16| (1..=DMX_CHANNELS).contains(&c);
                if *universe > MAX_UNIVERSE
                    || !channel_ok(*channel)
                    || fine.is_some_and(|f| !channel_ok(f) || f == *channel)
                {
                    return Err(ProjectError::Invalid(
                        "a DMX source needs a universe of 0 to 32767 and channels of 1 to 512"
                            .into(),
                    ));
                }
            }
            Self::Key { code, .. } => {
                if code.is_empty()
                    || code.len() > MAX_CODE
                    || !code.bytes().all(|b| b.is_ascii_alphanumeric())
                {
                    return Err(ProjectError::Invalid(
                        "a key source needs the code of a key".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// What a mapped button does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonMode {
    /// A press runs the target (an action), or turns a switch on.
    #[default]
    Trigger,
    /// A press flips a switch.
    Toggle,
    /// On while the button is held: a piano clip plays and stops, a switch follows the button, a
    /// continuous target goes back to its minimum on release.
    Hold,
}

impl ButtonMode {
    fn is_trigger(&self) -> bool {
        *self == Self::Trigger
    }
}

/// What is driven. Every id names something of the grid; one that is gone makes the mapping go.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Target {
    // Actions.
    /// Launches the clip; released, a piano clip stops.
    ClipLaunch {
        clip: String,
    },
    ColumnLaunch {
        column: String,
    },
    LayerStop {
        layer: String,
    },
    /// Steps the layer's autopilot: off, next, previous, random.
    LayerAutopilot {
        layer: String,
    },
    DeckSelect {
        deck: String,
    },
    DeckNext,
    DeckPrev,
    StopAll,
    BpmTap,
    BpmResync,
    /// One beat a minute more, or less.
    BpmPlus,
    BpmMinus,
    BpmDouble,
    BpmHalf,
    /// Runs the beats a little faster or slower while held.
    BpmNudgeUp,
    BpmNudgeDown,
    // Switches.
    LayerBypass {
        layer: String,
    },
    LayerSolo {
        layer: String,
    },
    EffectBypass {
        effect: String,
    },
    /// Holds the beats.
    BpmPause,
    // Continuous.
    LayerMaster {
        layer: String,
    },
    LayerVolume {
        layer: String,
    },
    LayerOpacity {
        layer: String,
    },
    /// The transition time, 0 to 10 s.
    LayerTransition {
        layer: String,
    },
    Crossfader,
    Bpm,
    ClipSpeed {
        clip: String,
    },
    EffectParam {
        effect: String,
        param: String,
    },
    /// A number of a generator clip (`size`, `rate`...).
    GeneratorNumber {
        clip: String,
        name: String,
    },
    /// A knob of the dashboard, 0 to 1.
    Dashboard {
        knob: usize,
    },
}

/// How a target is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Class {
    Continuous,
    Switch,
    Action,
}

impl Target {
    pub fn class(&self) -> Class {
        match self {
            Self::LayerBypass { .. }
            | Self::LayerSolo { .. }
            | Self::EffectBypass { .. }
            | Self::BpmPause => Class::Switch,
            Self::LayerMaster { .. }
            | Self::LayerVolume { .. }
            | Self::LayerOpacity { .. }
            | Self::LayerTransition { .. }
            | Self::Crossfader
            | Self::Bpm
            | Self::ClipSpeed { .. }
            | Self::EffectParam { .. }
            | Self::GeneratorNumber { .. }
            | Self::Dashboard { .. } => Class::Continuous,
            _ => Class::Action,
        }
    }

    /// Whether what it names is still in the grid.
    pub fn exists(&self, vj: &Vj) -> bool {
        let layer = |id: &str| vj.layers.iter().any(|l| l.id == id);
        match self {
            Self::ClipLaunch { clip } | Self::ClipSpeed { clip } => vj.find_clip(clip).is_some(),
            Self::GeneratorNumber { clip, name } => vj
                .find_clip(clip)
                .and_then(|(_, clip)| clip.generator.as_ref())
                .is_some_and(|generator| generator.numbers().contains(&name.as_str())),
            Self::ColumnLaunch { column } => vj.column_place(column).is_some(),
            Self::LayerStop { layer: id }
            | Self::LayerAutopilot { layer: id }
            | Self::LayerBypass { layer: id }
            | Self::LayerSolo { layer: id }
            | Self::LayerMaster { layer: id }
            | Self::LayerVolume { layer: id }
            | Self::LayerOpacity { layer: id }
            | Self::LayerTransition { layer: id } => layer(id),
            Self::DeckSelect { deck } => vj.decks.iter().any(|d| &d.id == deck),
            Self::EffectBypass { effect } => vj.find_effect(effect).is_some(),
            Self::EffectParam { effect, param } => vj
                .find_effect(effect)
                .is_some_and(|e| e.kind.number_range(param).is_some()),
            Self::Dashboard { knob } => *knob < KNOBS,
            _ => true,
        }
    }

    /// The range of a continuous target, in its own units; `None` for another class or a target
    /// that is gone.
    pub fn range(&self, vj: &Vj) -> Option<(f32, f32)> {
        if !self.exists(vj) {
            return None;
        }
        match self {
            Self::LayerMaster { .. }
            | Self::LayerVolume { .. }
            | Self::LayerOpacity { .. }
            | Self::Crossfader
            | Self::Dashboard { .. } => Some((0.0, 1.0)),
            Self::LayerTransition { .. } => Some((0.0, MAX_TRANSITION)),
            Self::Bpm => Some((MIN_BPM as f32, MAX_BPM as f32)),
            Self::ClipSpeed { .. } => Some((MIN_SPEED, MAX_SPEED)),
            Self::EffectParam { effect, param } => vj.find_effect(effect)?.kind.number_range(param),
            Self::GeneratorNumber { name, .. } => crate::generator::number_range(name),
            _ => None,
        }
    }
}

/// A source driving a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub id: String,
    pub source: Source,
    pub target: Target,
    /// Where a continuous target goes when the source is at its minimum, and its maximum, in the
    /// target's own units. Either may be the larger: that inverts the movement.
    pub min: f32,
    pub max: f32,
    /// Turns the source round, on top of the range.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// What a button does; absent is [`ButtonMode::Trigger`].
    #[serde(default, skip_serializing_if = "ButtonMode::is_trigger")]
    pub mode: ButtonMode,
    /// How a clip, layer, column, deck or effect target is followed; absent is [`Follow::Id`].
    #[serde(default, skip_serializing_if = "Follow::is_id")]
    pub follow: Follow,
    /// For [`Follow::Position`]: the slot the mapping stands for (a target whose ids are places,
    /// see [`crate::vj_mapfile`]). `target` is whatever fills the slot now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Target>,
    /// A position mapping whose slot is empty: kept, and answering nothing, until something
    /// fills the slot again. `target` is then stale.
    #[serde(default, skip_serializing_if = "is_false")]
    pub parked: bool,
}

/// What a mapping of a clip, layer, column, deck or effect keeps hold of, as Resolume's shortcut
/// modes do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Follow {
    /// This clip, layer or effect, by id, wherever it moves; deleting it takes the mapping away.
    #[default]
    Id,
    /// The slot (layer N, column M, effect slot K): whatever fills it now.
    Position,
}

impl Follow {
    fn is_id(&self) -> bool {
        *self == Self::Id
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Mapping {
    /// The value of the target for a source at `level` (0 to 1).
    pub fn scale(&self, level: f32) -> f32 {
        let s = level.clamp(0.0, 1.0);
        let s = if self.invert { 1.0 - s } else { s };
        self.min + (self.max - self.min) * s
    }
}

/// One knob of the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Knob {
    /// 0 to 1.
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the knob moves, each into a range of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
}

/// A number a knob drives: at 0 the knob gives `min`, at 1 `max` (units of the target).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub target: Target,
    pub min: f32,
    pub max: f32,
}

impl Link {
    fn at(&self, knob: f32) -> f32 {
        self.min + (self.max - self.min) * knob.clamp(0.0, 1.0)
    }
}

/// Whether a knob can drive `target`: a number stored with the grid (not the runtime crossfader,
/// and not another knob).
fn linkable(target: &Target) -> bool {
    target.class() == Class::Continuous
        && !matches!(target, Target::Crossfader | Target::Dashboard { .. })
}

// ---------------------------------------------------------------------------------------------
// Editing

fn finite(value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ProjectError::Invalid("a mapping is made of numbers".into()))
    }
}

/// The mode a new mapping of `target` starts with.
fn default_mode(target: &Target) -> ButtonMode {
    if target.class() == Class::Switch {
        ButtonMode::Toggle
    } else {
        ButtonMode::Trigger
    }
}

/// Whether a target of this class takes this button mode.
fn mode_fits(class: Class, mode: ButtonMode) -> bool {
    class == Class::Switch || mode != ButtonMode::Toggle
}

pub(crate) fn add_mapping(vj: &mut Vj, source: Source, target: Target, id: String) -> Result<()> {
    source.validate()?;
    if source.is_app_shortcut() {
        return Err(ProjectError::Invalid(
            "that key is used by the app's own shortcuts".into(),
        ));
    }
    if !target.exists(vj) {
        return Err(ProjectError::Invalid(format!(
            "there is nothing to map: {target:?}"
        )));
    }
    // The options of a target that is learned again stay.
    let kept = vj.mappings.iter().find(|m| m.target == target).cloned();
    let kept_none = kept.is_none();
    vj.mappings
        .retain(|m| m.target != target && !m.source.matches(&source));
    if vj.mappings.len() >= MAX_MAPPINGS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_MAPPINGS} mappings"
        )));
    }
    let (min, max) = target.range(vj).unwrap_or((0.0, 1.0));
    let mapping = match kept {
        Some(old) => Mapping {
            id,
            source,
            target,
            ..old
        },
        None => Mapping {
            id,
            source,
            mode: default_mode(&target),
            target,
            min,
            max,
            invert: false,
            follow: Follow::Id,
            place: None,
            parked: false,
        },
    };
    let mut mapping = mapping;
    if kept_none && target_has_place(&mapping.target) && !is_clip_or_effect(&mapping.target) {
        // New layer, column and deck mappings follow their slot; clips and effects, themselves.
        if let Some(place) = crate::vj_mapfile::to_places(vj, &mapping.target) {
            mapping.follow = Follow::Position;
            mapping.place = Some(place);
        }
    }
    vj.mappings.push(mapping);
    Ok(())
}

fn target_has_place(target: &Target) -> bool {
    !matches!(
        crate::vj_mapfile::kind_of(target),
        crate::vj_mapfile::Kind::Plain
    )
}

fn is_clip_or_effect(target: &Target) -> bool {
    matches!(
        crate::vj_mapfile::kind_of(target),
        crate::vj_mapfile::Kind::Clip | crate::vj_mapfile::Kind::Effect
    )
}

/// Moves the position mappings to what fills their slot now; one whose slot is empty is parked,
/// and one that fills again is woken. A level is held to the range of its new target.
pub(crate) fn rebind(vj: &mut Vj) {
    let found: Vec<(usize, Option<Target>)> = vj
        .mappings
        .iter()
        .enumerate()
        .filter(|(_, m)| m.follow == Follow::Position)
        .filter_map(|(at, m)| Some((at, m.place.as_ref()?)))
        .map(|(at, place)| {
            let now = crate::vj_mapfile::from_places(vj, place).filter(|t| t.exists(vj));
            (at, now)
        })
        .collect();
    for (at, now) in found {
        let range = now.as_ref().and_then(|t| t.range(vj));
        let mapping = &mut vj.mappings[at];
        match now {
            Some(target) => {
                mapping.target = target;
                mapping.parked = false;
                if let Some((lo, hi)) = range {
                    mapping.min = mapping.min.clamp(lo, hi);
                    mapping.max = mapping.max.clamp(lo, hi);
                }
            }
            None => mapping.parked = true,
        }
    }
}

pub(crate) fn set_mapping_follow(vj: &mut Vj, id: &str, follow: Follow) -> Result<()> {
    let at = vj
        .mappings
        .iter()
        .position(|m| m.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no mapping {id}")))?;
    let target = vj.mappings[at].target.clone();
    if !target_has_place(&target) {
        return Err(ProjectError::Invalid(
            "only a clip, layer, column, deck or effect has a slot to follow".into(),
        ));
    }
    let mapping = &mut vj.mappings[at];
    match follow {
        Follow::Id if mapping.parked => {
            return Err(ProjectError::Invalid(
                "the slot is empty: there is no clip, layer or effect to follow".into(),
            ));
        }
        Follow::Id => mapping.place = None,
        Follow::Position => {
            let place = crate::vj_mapfile::to_places(vj, &target)
                .ok_or_else(|| ProjectError::Invalid("that has no slot".into()))?;
            vj.mappings[at].place = Some(place);
        }
    }
    vj.mappings[at].follow = follow;
    Ok(())
}

pub(crate) fn remove_mapping(vj: &mut Vj, id: &str) -> Result<()> {
    let before = vj.mappings.len();
    vj.mappings.retain(|m| m.id != id);
    if vj.mappings.len() == before {
        return Err(ProjectError::Invalid(format!("no mapping {id}")));
    }
    Ok(())
}

pub(crate) fn set_mapping(
    vj: &mut Vj,
    id: &str,
    min: f32,
    max: f32,
    invert: bool,
    mode: ButtonMode,
) -> Result<()> {
    let at = vj
        .mappings
        .iter()
        .position(|m| m.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no mapping {id}")))?;
    let target = vj.mappings[at].target.clone();
    if !mode_fits(target.class(), mode) {
        return Err(ProjectError::Invalid(
            "only a switch can be toggled by a button".into(),
        ));
    }
    let (lo, hi) = target.range(vj).unwrap_or((0.0, 1.0));
    let mapping = &mut vj.mappings[at];
    if target.class() == Class::Continuous {
        mapping.min = finite(min)?.clamp(lo, hi);
        mapping.max = finite(max)?.clamp(lo, hi);
    }
    mapping.invert = invert;
    mapping.mode = mode;
    Ok(())
}

pub(crate) fn set_mapping_fine(vj: &mut Vj, id: &str, fine: Option<u16>) -> Result<()> {
    let mapping = vj
        .mappings
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no mapping {id}")))?;
    let Source::Artnet {
        universe, channel, ..
    } = mapping.source
    else {
        return Err(ProjectError::Invalid(
            "only a DMX mapping has a fine channel".into(),
        ));
    };
    let source = Source::Artnet {
        universe,
        channel,
        fine,
    };
    source.validate()?;
    mapping.source = source;
    // Two mappings of one source would both answer it; the one just set stays.
    let id = mapping.id.clone();
    let source = mapping.source.clone();
    vj.mappings.retain(|m| m.id == id || m.source != source);
    Ok(())
}

/// The knob `knob`, made if the dashboard is not that long yet.
fn knob_mut(vj: &mut Vj, knob: usize) -> Result<&mut Knob> {
    if knob >= KNOBS {
        return Err(ProjectError::Invalid(format!(
            "the dashboard has {KNOBS} knobs"
        )));
    }
    if vj.dashboard.len() <= knob {
        vj.dashboard.resize_with(knob + 1, Knob::default);
    }
    Ok(&mut vj.dashboard[knob])
}

pub(crate) fn link_knob(
    vj: &mut Vj,
    knob: usize,
    target: Target,
    min: Option<f32>,
    max: Option<f32>,
) -> Result<()> {
    if !linkable(&target) {
        return Err(ProjectError::Invalid(
            "a knob drives a number of the grid".into(),
        ));
    }
    let (lo, hi) = target
        .range(vj)
        .ok_or_else(|| ProjectError::Invalid(format!("there is nothing to link: {target:?}")))?;
    let link = Link {
        min: finite(min.unwrap_or(lo))?.clamp(lo, hi),
        max: finite(max.unwrap_or(hi))?.clamp(lo, hi),
        target,
    };
    let knob = knob_mut(vj, knob)?;
    if let Some(old) = knob.links.iter_mut().find(|l| l.target == link.target) {
        *old = link;
    } else if knob.links.len() >= MAX_LINKS {
        return Err(ProjectError::Invalid(format!(
            "a knob drives at most {MAX_LINKS} numbers"
        )));
    } else {
        knob.links.push(link);
    }
    Ok(())
}

pub(crate) fn unlink_knob(vj: &mut Vj, knob: usize, target: &Target) -> Result<()> {
    knob_mut(vj, knob)?.links.retain(|l| &l.target != target);
    Ok(())
}

pub(crate) fn rename_knob(vj: &mut Vj, knob: usize, name: Option<String>) -> Result<()> {
    knob_mut(vj, knob)?.name = name;
    Ok(())
}

/// Turns the knob and moves what it is linked to, as one edit. A link that cannot move (its
/// target is gone) is skipped; pruning takes it away at the next removal.
pub(crate) fn turn_knob(project: &mut Project, knob: usize, value: f32) -> Result<()> {
    let value = finite(value)?.clamp(0.0, 1.0);
    let links = {
        let knob = knob_mut(&mut project.vj, knob)?;
        knob.value = value;
        knob.links.clone()
    };
    for link in links {
        if let Some(op) = value_op(&project.vj, &link.target, link.at(value)) {
            // The ops of a link make no ids, and `String::new` keeps the recursion one type deep.
            crate::vj::apply(project, op, String::new)?;
        }
    }
    Ok(())
}

/// Takes away the mappings and links whose target is gone.
pub fn prune(vj: &mut Vj) {
    let mappings = std::mem::take(&mut vj.mappings);
    vj.mappings = mappings
        .into_iter()
        .filter(|m| m.parked || m.target.exists(vj))
        .collect();
    let mut dashboard = std::mem::take(&mut vj.dashboard);
    for knob in &mut dashboard {
        knob.links.retain(|link| link.target.exists(vj));
    }
    vj.dashboard = dashboard;
}

/// Checks the mappings and the dashboard: known targets, ranges, ids, modes.
pub(crate) fn validate(vj: &Vj) -> Result<()> {
    if vj.mappings.len() > MAX_MAPPINGS || vj.dashboard.len() > KNOBS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_MAPPINGS} mappings and {KNOBS} knobs"
        )));
    }
    let mut ids = HashSet::new();
    for mapping in &vj.mappings {
        if !ids.insert(mapping.id.as_str()) {
            return Err(ProjectError::Invalid(format!(
                "duplicate mapping id: {}",
                mapping.id
            )));
        }
        mapping.source.validate()?;
        let class = mapping.target.class();
        if mapping.parked && mapping.follow != Follow::Position
            || mapping.follow == Follow::Position && mapping.place.is_none()
        {
            return Err(ProjectError::Invalid(format!(
                "mapping {} follows a slot it does not name",
                mapping.id
            )));
        }
        if !mapping.parked && !mapping.target.exists(vj) {
            return Err(ProjectError::Invalid(format!(
                "mapping {} drives something that is not in the grid",
                mapping.id
            )));
        }
        if !mode_fits(class, mapping.mode) {
            return Err(ProjectError::Invalid(format!(
                "mapping {} toggles something that is not a switch",
                mapping.id
            )));
        }
        if class == Class::Continuous && !mapping.parked {
            let (lo, hi) = mapping.target.range(vj).unwrap_or((0.0, 1.0));
            let inside = |v: f32| v.is_finite() && (lo..=hi).contains(&v);
            if !inside(mapping.min) || !inside(mapping.max) {
                return Err(ProjectError::Invalid(format!(
                    "mapping {} has a range outside its target's",
                    mapping.id
                )));
            }
        }
    }
    for knob in &vj.dashboard {
        if !(0.0..=1.0).contains(&knob.value) || knob.links.len() > MAX_LINKS {
            return Err(ProjectError::Invalid(
                "a knob is 0 to 1 and drives at most 16 numbers".into(),
            ));
        }
        crate::vj::check_name(knob.name.as_deref())?;
        for link in &knob.links {
            let (lo, hi) = link.target.range(vj).unwrap_or((1.0, 0.0));
            let inside = |v: f32| v.is_finite() && (lo..=hi).contains(&v);
            if !linkable(&link.target) || !inside(link.min) || !inside(link.max) {
                return Err(ProjectError::Invalid(
                    "a knob is linked to a number that is not in the grid, or out of its range"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Hearing

/// What the source of a mapping did: the 0 to 1 it reads, and whether it is down.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub source: Source,
    pub level: f32,
    /// A note on, a control above 0, an OSC value above 0 (a bend, above half).
    pub down: bool,
}

/// How a button changed since the last message of its mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Press,
    Release,
    /// Nothing new: a knob sends many values above 0, a held button sends nothing.
    Same,
}

/// The edge of `down` for the mapping `id`, which `held` remembers.
pub fn edge(held: &mut HashSet<String>, id: &str, down: bool) -> Edge {
    match (held.contains(id), down) {
        (false, true) => {
            held.insert(id.to_owned());
            Edge::Press
        }
        (true, false) => {
            held.remove(id);
            Edge::Release
        }
        _ => Edge::Same,
    }
}

/// What a target is asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Go to this value, in the target's units.
    Value(f32),
    Switch(bool),
    Toggle,
    Press,
    Release,
}

/// The command a mapping gives for a source at `level` that changed by `edge`; `None` when it
/// asks for nothing (a release of a trigger).
pub fn command(mapping: &Mapping, level: f32, edge: Edge) -> Option<Command> {
    // A pad or a key is a button: its press is the level, its release the minimum if held.
    let is_note = matches!(
        mapping.source,
        Source::Midi {
            kind: MidiKind::Note,
            ..
        } | Source::Key { .. }
    );
    match mapping.target.class() {
        Class::Continuous => match (is_note, edge) {
            // A pad is a button: its velocity is the level, its release the minimum if held.
            (true, Edge::Press) => Some(Command::Value(mapping.scale(level))),
            (true, Edge::Release) if mapping.mode == ButtonMode::Hold => {
                Some(Command::Value(mapping.scale(0.0)))
            }
            (true, _) => None,
            (false, _) => Some(Command::Value(mapping.scale(level))),
        },
        Class::Switch => match (edge, mapping.mode) {
            (Edge::Press, ButtonMode::Toggle) => Some(Command::Toggle),
            (Edge::Press, _) => Some(Command::Switch(true)),
            (Edge::Release, ButtonMode::Hold) => Some(Command::Switch(false)),
            _ => None,
        },
        Class::Action => match edge {
            Edge::Press => Some(Command::Press),
            Edge::Release => Some(Command::Release),
            Edge::Same => None,
        },
    }
}

/// The commands of the mappings that `heard` is the source of. `held` remembers which buttons are
/// down, between calls.
pub fn react(vj: &Vj, heard: &Heard, held: &mut HashSet<String>) -> Vec<(Target, Command)> {
    vj.mappings
        .iter()
        .filter(|m| !m.parked && m.source.matches(&heard.source))
        .filter_map(|m| {
            let edge = edge(held, &m.id, heard.down);
            Some((m.target.clone(), command(m, heard.level, edge)?))
        })
        .collect()
}

/// Of consecutive values for one target keeps the last: a knob turned fast sends hundreds a
/// second, and only where it ended matters. Presses, releases and switches all stay, in order.
pub fn coalesce(commands: Vec<(Target, Command)>) -> Vec<(Target, Command)> {
    let mut out: Vec<(Target, Command)> = Vec::with_capacity(commands.len());
    for (target, command) in commands {
        if let Command::Value(_) = command
            && let Some((last, Command::Value(_))) = out.last()
            && *last == target
        {
            out.pop();
        }
        out.push((target, command));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Doing

/// What the app does to the playing grid (which is never stored).
#[derive(Debug, Clone, PartialEq)]
pub enum PlayAction {
    Launch(String),
    Release(String),
    LaunchColumn(String),
    /// A layer, or every layer.
    Stop(Option<String>),
    /// 0 is all side A, 1 all side B.
    Crossfader(f32),
    /// Holds the beats, or lets them go on; `None` flips.
    BeatPause(Option<bool>),
    Resync,
    /// Faster (1), slower (-1) or at the tempo (0).
    Nudge(i8),
}

/// What a command comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// An edit of the project, which enters the undo history.
    Edit(VjOp),
    Play(PlayAction),
    /// A tap of the tempo; the app keeps the times of the taps.
    Tap,
}

/// The edit that sets a continuous `target` to `value`, brought into range by the op.
pub fn value_op(vj: &Vj, target: &Target, value: f32) -> Option<VjOp> {
    let value = if value.is_finite() {
        value
    } else {
        return None;
    };
    Some(match target {
        Target::LayerMaster { layer } => VjOp::LayerMaster {
            layer: layer.clone(),
            value,
        },
        Target::LayerVolume { layer } => VjOp::LayerVolume {
            layer: layer.clone(),
            value,
        },
        Target::LayerOpacity { layer } => VjOp::LayerOpacity {
            layer: layer.clone(),
            value,
        },
        Target::LayerTransition { layer } => VjOp::LayerTransition {
            layer: layer.clone(),
            value,
        },
        Target::Bpm => VjOp::SetBpm {
            bpm: f64::from(value),
        },
        Target::ClipSpeed { clip } => VjOp::ClipSpeed {
            clip: clip.clone(),
            value,
        },
        Target::EffectParam { effect, param } => {
            let found = vj.find_effect(effect)?;
            VjOp::EffectParams {
                effect: effect.clone(),
                kind: found.kind.with_numbers(&[(param.clone(), value)])?,
            }
        }
        Target::GeneratorNumber { clip, name } => {
            let (_, found) = vj.find_clip(clip)?;
            let mut generator = found.generator.clone()?;
            generator.set_modulated(name, value);
            VjOp::ClipGenerator {
                clip: clip.clone(),
                generator,
            }
        }
        Target::Dashboard { knob } => VjOp::DashboardValue { knob: *knob, value },
        _ => return None,
    })
}

/// The deck `step` places from the one shown, wrapping round.
fn deck_by(vj: &Vj, step: isize) -> Option<String> {
    let here = vj.decks.iter().position(|d| d.id == vj.deck)?;
    let n = vj.decks.len() as isize;
    let at = (here as isize + step).rem_euclid(n);
    vj.decks
        .get(usize::try_from(at).ok()?)
        .map(|d| d.id.clone())
}

/// What `command` asks of `target`, given the grid as it is now (a flip needs the current state).
pub fn plan(vj: &Vj, target: &Target, command: Command) -> Option<Plan> {
    use Command::{Press, Release, Switch, Toggle, Value};
    let layer_of = |id: &str| vj.layers.iter().find(|l| l.id == id);
    let edit = |op| Some(Plan::Edit(op));
    let play = |action| Some(Plan::Play(action));
    match (target, command) {
        (Target::Crossfader, Value(v)) => play(PlayAction::Crossfader(v.clamp(0.0, 1.0))),
        (t, Value(v)) if t.class() == Class::Continuous => value_op(vj, t, v).map(Plan::Edit),
        (Target::ClipLaunch { clip }, Press) => play(PlayAction::Launch(clip.clone())),
        (Target::ClipLaunch { clip }, Release) => play(PlayAction::Release(clip.clone())),
        (Target::ColumnLaunch { column }, Press) => play(PlayAction::LaunchColumn(column.clone())),
        (Target::LayerStop { layer }, Press) => play(PlayAction::Stop(Some(layer.clone()))),
        (Target::StopAll, Press) => play(PlayAction::Stop(None)),
        (Target::LayerAutopilot { layer }, Press) => edit(VjOp::LayerAutopilot {
            layer: layer.clone(),
            mode: layer_of(layer)?.autopilot.cycled(),
        }),
        (Target::DeckSelect { deck }, Press) => edit(VjOp::SelectDeck { deck: deck.clone() }),
        (Target::DeckNext, Press) => edit(VjOp::SelectDeck {
            deck: deck_by(vj, 1)?,
        }),
        (Target::DeckPrev, Press) => edit(VjOp::SelectDeck {
            deck: deck_by(vj, -1)?,
        }),
        (Target::BpmTap, Press) => Some(Plan::Tap),
        (Target::BpmResync, Press) => play(PlayAction::Resync),
        (Target::BpmPlus, Press) => edit(VjOp::NudgeBpm { by: 1.0 }),
        (Target::BpmMinus, Press) => edit(VjOp::NudgeBpm { by: -1.0 }),
        (Target::BpmDouble, Press) => edit(VjOp::ScaleBpm { factor: 2.0 }),
        (Target::BpmHalf, Press) => edit(VjOp::ScaleBpm { factor: 0.5 }),
        (Target::BpmNudgeUp, Press) => play(PlayAction::Nudge(1)),
        (Target::BpmNudgeDown, Press) => play(PlayAction::Nudge(-1)),
        (Target::BpmNudgeUp | Target::BpmNudgeDown, Release) => play(PlayAction::Nudge(0)),
        (Target::BpmPause, Switch(on)) => play(PlayAction::BeatPause(Some(on))),
        (Target::BpmPause, Toggle) => play(PlayAction::BeatPause(None)),
        (Target::LayerBypass { layer }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !layer_of(layer)?.bypass,
            };
            edit(VjOp::LayerBypass {
                layer: layer.clone(),
                on,
            })
        }
        (Target::LayerSolo { layer }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !layer_of(layer)?.solo,
            };
            edit(VjOp::LayerSolo {
                layer: layer.clone(),
                on,
            })
        }
        (Target::EffectBypass { effect }, Switch(_) | Toggle) => {
            let bypass = match command {
                Switch(on) => on,
                _ => !vj.find_effect(effect)?.bypass,
            };
            edit(VjOp::EffectBypass {
                effect: effect.clone(),
                bypass,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// The fixed OSC addresses

/// The number an OSC message carries: an integer, a float, a boolean or a string with a number.
pub fn osc_level(args: &[OscArg]) -> Option<f32> {
    let value = match args.first()? {
        OscArg::Int(v) => *v as f32,
        OscArg::Float(v) => *v,
        OscArg::Bool(v) => f32::from(u8::from(*v)),
        OscArg::String(text) => text.trim().parse().ok()?,
    };
    value.is_finite().then_some(value)
}

/// A place counted from 1 in an address, as an index.
fn place(segment: &str) -> Option<usize> {
    segment.parse::<usize>().ok()?.checked_sub(1)
}

/// The deck layer's or global layer's clip at the place `column` (from 0) of the deck shown.
fn clip_at(vj: &Vj, layer: usize, column: usize) -> Option<String> {
    let layer = vj.layers.get(layer)?;
    let deck = vj.shown_deck()?;
    let column_id = &deck.columns.get(column)?.id;
    layer
        .clips
        .iter()
        .find(|clip| {
            if layer.global {
                clip.slot == Some(column)
            } else {
                clip.column.as_deref() == Some(column_id)
            }
        })
        .map(|clip| clip.id.clone())
}

/// The target and command of an OSC message to one of the fixed addresses, which count from 1 by
/// position as Resolume does (layers from the bottom, columns of the deck shown from the left):
///
/// | Address | Value | Does |
/// | --- | --- | --- |
/// | `/composition/layers/{n}/clips/{m}/connect` | 1 or 0 | launches (releases) the clip |
/// | `/composition/columns/{m}/connect` | 1 | launches the column |
/// | `/composition/layers/{n}/video/opacity` | 0 to 1 | opacity |
/// | `/composition/layers/{n}/master` | 0 to 1 | master |
/// | `/composition/layers/{n}/audio/volume` | 0 to 1 | audio level |
/// | `/composition/layers/{n}/transition/duration` | 0 to 1 | transition time (0 to 10 s) |
/// | `/composition/layers/{n}/bypassed`, `/solo` | 0 or 1 | sets the switch |
/// | `/composition/layers/{n}/clear` | 1 | stops the layer |
/// | `/composition/crossfader/phase` | 0 to 1 | the crossfader |
/// | `/composition/tempocontroller/tempo` | beats a minute | the tempo |
/// | `/composition/tempocontroller/{tempotap,resync,plus,minus,double,half}` | 1 | tempo buttons |
/// | `/composition/tempocontroller/pause` | 0 or 1 | holds the beats |
/// | `/composition/decks/{d}/select` | 1 | shows the deck |
/// | `/composition/decks/next`, `/composition/decks/prev` | 1 | next or previous deck |
/// | `/composition/disconnectall` | 1 | stops every layer |
/// | `/composition/dashboard/link{k}` | 0 to 1 | turns knob `k` |
///
/// The tempo addresses also work without `/composition`. A button address with no value is a
/// press, with a value above 0 too, and with 0 a release.
pub fn osc_command(vj: &Vj, address: &str, args: &[OscArg]) -> Option<(Target, Command)> {
    let level = osc_level(args);
    let parts: Vec<&str> = address.strip_prefix('/')?.split('/').collect();
    let layer_id = |n: &str| Some(vj.layers.get(place(n)?)?.id.clone());
    // A button: pressed unless the value is 0.
    let button = |target: Target| {
        let command = if level.is_some_and(|v| v <= 0.0) {
            Command::Release
        } else {
            Command::Press
        };
        Some((target, command))
    };
    // A fader: the 0 to 1 of the address, into the whole range of the target.
    let fader = |target: Target| {
        let (lo, hi) = target.range(vj)?;
        Some((
            target,
            Command::Value(lo + (hi - lo) * level?.clamp(0.0, 1.0)),
        ))
    };
    let switch = |target: Target| Some((target, Command::Switch(level? >= 0.5)));
    let parts = match parts.as_slice() {
        ["composition", rest @ ..] => rest,
        ["tempocontroller", ..] => parts.as_slice(),
        _ => return None,
    };
    match parts {
        ["layers", n, "clips", m, "connect"] => {
            let clip = clip_at(vj, place(n)?, place(m)?)?;
            button(Target::ClipLaunch { clip })
        }
        ["columns", m, "connect"] => {
            let column = vj.shown_deck()?.columns.get(place(m)?)?.id.clone();
            button(Target::ColumnLaunch { column })
        }
        ["layers", n, "video", "opacity"] => fader(Target::LayerOpacity {
            layer: layer_id(n)?,
        }),
        ["layers", n, "master"] => fader(Target::LayerMaster {
            layer: layer_id(n)?,
        }),
        ["layers", n, "audio", "volume"] => fader(Target::LayerVolume {
            layer: layer_id(n)?,
        }),
        ["layers", n, "transition", "duration"] => fader(Target::LayerTransition {
            layer: layer_id(n)?,
        }),
        ["layers", n, "bypassed"] => switch(Target::LayerBypass {
            layer: layer_id(n)?,
        }),
        ["layers", n, "solo"] => switch(Target::LayerSolo {
            layer: layer_id(n)?,
        }),
        ["layers", n, "clear"] => button(Target::LayerStop {
            layer: layer_id(n)?,
        }),
        ["crossfader", "phase"] => fader(Target::Crossfader),
        ["tempocontroller", "tempo"] => Some((
            Target::Bpm,
            Command::Value(level?.clamp(MIN_BPM as f32, MAX_BPM as f32)),
        )),
        ["tempocontroller", "tempotap"] => button(Target::BpmTap),
        ["tempocontroller", "resync"] => button(Target::BpmResync),
        ["tempocontroller", "plus"] => button(Target::BpmPlus),
        ["tempocontroller", "minus"] => button(Target::BpmMinus),
        ["tempocontroller", "double"] => button(Target::BpmDouble),
        ["tempocontroller", "half"] => button(Target::BpmHalf),
        ["tempocontroller", "pause"] => switch(Target::BpmPause),
        ["decks", d, "select"] => {
            let deck = vj.decks.get(place(d)?)?.id.clone();
            button(Target::DeckSelect { deck })
        }
        ["decks", "next"] => button(Target::DeckNext),
        ["decks", "prev"] => button(Target::DeckPrev),
        ["disconnectall"] => button(Target::StopAll),
        ["dashboard", link] => {
            let knob = place(link.strip_prefix("link")?)?;
            (knob < KNOBS).then_some(())?;
            fader(Target::Dashboard { knob })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj::{Autopilot, Layer, VjOp};
    use crate::{Project, ProjectMode};
    use flux_core::FrameRate;

    fn show() -> Project {
        let mut ids = (0..).map(|n| format!("id{n}"));
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(|| ids.next().unwrap());
        project
    }

    fn apply(project: &mut Project, op: VjOp) -> Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(1000);
        crate::vj::apply(project, op, || {
            format!("new{}", NEXT.fetch_add(1, Ordering::Relaxed))
        })
    }

    fn cc(number: u8) -> Source {
        Source::Midi {
            port: Some("APC40".into()),
            channel: 1,
            kind: MidiKind::Cc,
            number,
        }
    }

    fn note(number: u8) -> Source {
        Source::Midi {
            port: None,
            channel: 1,
            kind: MidiKind::Note,
            number,
        }
    }

    fn heard(source: Source, level: f32, down: bool) -> Heard {
        Heard {
            source,
            level,
            down,
        }
    }

    fn layer(project: &Project, at: usize) -> &Layer {
        &project.vj.layers[at]
    }

    fn with_generator(project: &mut Project) -> String {
        let layer = layer(project, 0).id.clone();
        let column = project.vj.decks[0].columns[0].id.clone();
        apply(
            project,
            VjOp::PlaceGenerator {
                generator: crate::generator::Generator::Strobe {
                    colour: "#ffffff".into(),
                    rate: 8.0,
                    width: 0.5,
                },
                layer,
                column,
            },
        )
        .unwrap();
        project.vj.layers[0].clips[0].id.clone()
    }

    #[test]
    fn a_cc_should_scale_into_the_range_of_its_mapping() {
        let mut project = show();
        let layer = layer(&project, 0).id.clone();
        let target = Target::LayerOpacity {
            layer: layer.clone(),
        };
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(7),
                target,
            },
        )
        .unwrap();
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.2,
                max: 0.8,
                invert: false,
                mode: ButtonMode::Trigger,
            },
        )
        .unwrap();
        let mut held = HashSet::new();

        let low = react(&project.vj, &heard(cc(7), 0.0, false), &mut held);
        let high = react(&project.vj, &heard(cc(7), 1.0, true), &mut held);

        assert_eq!(low[0].1, Command::Value(0.2));
        assert_eq!(high[0].1, Command::Value(0.8));
    }

    #[test]
    fn invert_should_turn_the_source_round_inside_the_range() {
        let mapping = Mapping {
            id: "m".into(),
            source: cc(1),
            target: Target::Crossfader,
            min: 0.0,
            max: 1.0,
            invert: true,
            mode: ButtonMode::Trigger,
            follow: Follow::Id,
            place: None,
            parked: false,
        };
        assert_eq!(mapping.scale(0.25), 0.75);
        let reversed = Mapping {
            min: 1.0,
            max: 0.0,
            invert: false,
            ..mapping
        };
        assert_eq!(
            reversed.scale(0.25),
            0.75,
            "a larger min is an inversion too"
        );
    }

    #[test]
    fn a_pitch_bend_should_use_all_its_fourteen_bits() {
        let bend = Source::Midi {
            port: None,
            channel: 1,
            kind: MidiKind::PitchBend,
            number: 0,
        };
        let mapping = Mapping {
            id: "m".into(),
            source: bend,
            target: Target::Bpm,
            min: 100.0,
            max: 200.0,
            invert: false,
            mode: ButtonMode::Trigger,
            follow: Follow::Id,
            place: None,
            parked: false,
        };
        // The decoder gives value / 16383 as the level.
        let centre = command(&mapping, 8192.0 / 16383.0, Edge::Same).unwrap();
        let top = command(&mapping, 16383.0 / 16383.0, Edge::Same).unwrap();
        let Command::Value(centre) = centre else {
            panic!("a value")
        };
        assert!((centre - 150.0).abs() < 0.01);
        assert_eq!(top, Command::Value(200.0));
    }

    #[test]
    fn a_note_should_toggle_a_switch_and_a_held_one_should_follow_the_pad() {
        let mut project = show();
        let id = layer(&project, 1).id.clone();
        let target = Target::LayerBypass { layer: id };
        apply(
            &mut project,
            VjOp::AddMapping {
                source: note(36),
                target: target.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            project.vj.mappings[0].mode,
            ButtonMode::Toggle,
            "a switch starts as a toggle"
        );
        let mut held = HashSet::new();
        let on = react(&project.vj, &heard(note(36), 1.0, true), &mut held);
        let off = react(&project.vj, &heard(note(36), 0.0, false), &mut held);
        assert_eq!(on, vec![(target.clone(), Command::Toggle)]);
        assert!(off.is_empty(), "a toggle ignores the release");

        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.0,
                max: 1.0,
                invert: false,
                mode: ButtonMode::Hold,
            },
        )
        .unwrap();
        let down = react(&project.vj, &heard(note(36), 1.0, true), &mut held);
        let up = react(&project.vj, &heard(note(36), 0.0, false), &mut held);
        assert_eq!(down[0].1, Command::Switch(true));
        assert_eq!(up[0].1, Command::Switch(false));
    }

    #[test]
    fn a_knob_sending_many_values_should_press_a_button_once() {
        let mut project = show();
        let target = Target::StopAll;
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(20),
                target,
            },
        )
        .unwrap();
        let mut held = HashSet::new();
        let first = react(&project.vj, &heard(cc(20), 0.5, true), &mut held);
        let second = react(&project.vj, &heard(cc(20), 0.6, true), &mut held);
        let release = react(&project.vj, &heard(cc(20), 0.0, false), &mut held);
        assert_eq!(first[0].1, Command::Press);
        assert!(second.is_empty());
        assert_eq!(release[0].1, Command::Release);
    }

    #[test]
    fn a_pad_mapped_to_a_fader_should_use_its_velocity_and_hold_should_drop_it() {
        let mut project = show();
        let target = Target::LayerMaster {
            layer: layer(&project, 0).id.clone(),
        };
        apply(
            &mut project,
            VjOp::AddMapping {
                source: note(40),
                target,
            },
        )
        .unwrap();
        let mut held = HashSet::new();
        let press = react(&project.vj, &heard(note(40), 0.5, true), &mut held);
        let release = react(&project.vj, &heard(note(40), 0.0, false), &mut held);
        assert_eq!(press[0].1, Command::Value(0.5));
        assert!(release.is_empty());
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.0,
                max: 1.0,
                invert: false,
                mode: ButtonMode::Hold,
            },
        )
        .unwrap();
        react(&project.vj, &heard(note(40), 1.0, true), &mut held);
        let release = react(&project.vj, &heard(note(40), 0.0, false), &mut held);
        assert_eq!(release[0].1, Command::Value(0.0));
    }

    #[test]
    fn a_clip_launch_should_press_and_release_and_the_plan_should_launch_and_release() {
        let mut project = show();
        let clip = with_generator(&mut project);
        let target = Target::ClipLaunch { clip: clip.clone() };
        apply(
            &mut project,
            VjOp::AddMapping {
                source: note(48),
                target,
            },
        )
        .unwrap();
        let mut held = HashSet::new();
        let press = react(&project.vj, &heard(note(48), 1.0, true), &mut held);
        let release = react(&project.vj, &heard(note(48), 0.0, false), &mut held);
        let plan_of = |(target, command): &(Target, Command)| plan(&project.vj, target, *command);
        assert_eq!(
            plan_of(&press[0]),
            Some(Plan::Play(PlayAction::Launch(clip.clone())))
        );
        assert_eq!(
            plan_of(&release[0]),
            Some(Plan::Play(PlayAction::Release(clip)))
        );
    }

    #[test]
    fn learning_should_replace_by_source_and_by_target_and_keep_the_options() {
        let mut project = show();
        let layer_id = layer(&project, 0).id.clone();
        let opacity = Target::LayerOpacity {
            layer: layer_id.clone(),
        };
        let master = Target::LayerMaster { layer: layer_id };
        let add = |project: &mut Project, source, target| {
            apply(project, VjOp::AddMapping { source, target }).unwrap();
        };
        add(&mut project, cc(1), opacity.clone());
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.1,
                max: 0.9,
                invert: true,
                mode: ButtonMode::Trigger,
            },
        )
        .unwrap();

        add(&mut project, cc(2), opacity.clone());
        assert_eq!(project.vj.mappings.len(), 1, "a target has one source");
        assert_eq!(project.vj.mappings[0].source, cc(2));
        assert_eq!(
            (project.vj.mappings[0].min, project.vj.mappings[0].invert),
            (0.1, true),
            "learning again keeps what was set"
        );

        add(&mut project, cc(2), master.clone());
        assert_eq!(project.vj.mappings.len(), 1, "a source has one target");
        assert_eq!(project.vj.mappings[0].target, master);
    }

    #[test]
    fn a_target_that_is_gone_should_be_refused_and_removing_it_should_prune_its_mappings() {
        let mut project = show();
        let gone = Target::LayerOpacity {
            layer: "nope".into(),
        };
        assert!(
            apply(
                &mut project,
                VjOp::AddMapping {
                    source: cc(1),
                    target: gone
                }
            )
            .is_err()
        );

        let clip = with_generator(&mut project);
        let layer_id = layer(&project, 1).id.clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::ClipLaunch { clip: clip.clone() },
            },
        )
        .unwrap();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(2),
                target: Target::LayerMaster {
                    layer: layer_id.clone(),
                },
            },
        )
        .unwrap();
        // A layer mapping follows its slot unless told to follow the layer itself.
        let mapping = project.vj.mappings[1].id.clone();
        apply(
            &mut project,
            VjOp::SetMappingFollow {
                mapping,
                follow: Follow::Id,
            },
        )
        .unwrap();
        apply(&mut project, VjOp::RemoveClip { clip }).unwrap();
        assert_eq!(project.vj.mappings.len(), 1);
        apply(&mut project, VjOp::RemoveLayer { layer: layer_id }).unwrap();
        assert!(project.vj.mappings.is_empty());
        project.validate().unwrap();
    }

    #[test]
    fn validation_should_refuse_a_dangling_mapping_and_a_range_out_of_the_target() {
        let mut project = show();
        let layer_id = layer(&project, 0).id.clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerOpacity {
                    layer: layer_id.clone(),
                },
            },
        )
        .unwrap();
        project.validate().unwrap();
        let mut dangling = project.clone();
        dangling.vj.layers[0].id = "renamed".into();
        assert!(dangling.validate().is_err());
        let mut wide = project.clone();
        wide.vj.mappings[0].max = 3.0;
        assert!(wide.validate().is_err());
        let mut toggled = project;
        toggled.vj.mappings[0].mode = ButtonMode::Toggle;
        assert!(toggled.validate().is_err(), "only a switch toggles");
    }

    #[test]
    fn a_mapping_should_survive_a_round_trip_and_an_old_file_should_have_none() {
        let mut project = show();
        let layer_id = layer(&project, 0).id.clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(7),
                target: Target::LayerOpacity { layer: layer_id },
            },
        )
        .unwrap();
        let json = serde_json::to_string(&project.vj).unwrap();
        let back: Vj = serde_json::from_str(&json).unwrap();
        assert_eq!(back, project.vj);
        assert!(json.contains(r#""type":"midi""#));
        assert!(json.contains(r#""kind":"layerOpacity""#));

        let mut old = serde_json::to_value(&project.vj).unwrap();
        let object = old.as_object_mut().unwrap();
        object.remove("mappings");
        object.remove("dashboard");
        let old: Vj = serde_json::from_value(old).unwrap();
        assert!(old.mappings.is_empty() && old.dashboard.is_empty());
        let plain = serde_json::to_value(Vj::starting(String::new)).unwrap();
        assert!(
            plain.get("mappings").is_none(),
            "an empty list is not written"
        );
    }

    #[test]
    fn effect_and_generator_numbers_should_become_edits_in_their_own_range() {
        let mut project = show();
        let clip = with_generator(&mut project);
        let layer_id = layer(&project, 0).id.clone();
        apply(
            &mut project,
            VjOp::AddEffect {
                target: crate::vj::EffectTarget::Layer(layer_id),
                kind: "blur".into(),
            },
        )
        .unwrap();
        let effect = project.vj.layers[0].effects[0].id.clone();
        let param = Target::EffectParam {
            effect: effect.clone(),
            param: "radius".into(),
        };
        let (lo, hi) = param.range(&project.vj).unwrap();
        let Some(Plan::Edit(op)) = plan(&project.vj, &param, Command::Value(hi)) else {
            panic!("an edit")
        };
        apply(&mut project, op).unwrap();
        assert!(lo < hi);
        let number = Target::GeneratorNumber {
            clip,
            name: "rate".into(),
        };
        let Some(Plan::Edit(op)) = plan(&project.vj, &number, Command::Value(20.0)) else {
            panic!("an edit")
        };
        apply(&mut project, op).unwrap();
        match &project.vj.layers[0].clips[0].generator {
            Some(crate::generator::Generator::Strobe { rate, .. }) => assert_eq!(*rate, 20.0),
            other => panic!("{other:?}"),
        }
        project.validate().unwrap();
    }

    #[test]
    fn a_knob_should_move_every_link_into_its_own_range_in_one_edit() {
        let mut project = show();
        let a = layer(&project, 0).id.clone();
        let b = layer(&project, 1).id.clone();
        let link = |project: &mut Project, layer: &str, min, max| {
            apply(
                project,
                VjOp::DashboardLink {
                    knob: 2,
                    target: Target::LayerOpacity {
                        layer: layer.into(),
                    },
                    min: Some(min),
                    max: Some(max),
                },
            )
            .unwrap();
        };
        link(&mut project, &a, 0.0, 1.0);
        link(&mut project, &b, 1.0, 0.5);
        assert_eq!(
            project.vj.dashboard.len(),
            3,
            "the dashboard grows to the knob"
        );

        apply(
            &mut project,
            VjOp::DashboardValue {
                knob: 2,
                value: 0.5,
            },
        )
        .unwrap();

        assert_eq!(layer(&project, 0).opacity, 0.5);
        assert_eq!(
            layer(&project, 1).opacity,
            0.75,
            "a larger min inverts the link"
        );
        assert_eq!(project.vj.dashboard[2].value, 0.5);
        project.validate().unwrap();
    }

    #[test]
    fn a_knob_should_refuse_what_it_cannot_drive_and_a_ninth_knob() {
        let mut project = show();
        let id = layer(&project, 0).id.clone();
        let link = |target| VjOp::DashboardLink {
            knob: 0,
            target,
            min: None,
            max: None,
        };
        assert!(apply(&mut project, link(Target::Crossfader)).is_err());
        assert!(apply(&mut project, link(Target::Dashboard { knob: 1 })).is_err());
        assert!(apply(&mut project, link(Target::StopAll)).is_err());
        assert!(
            apply(
                &mut project,
                VjOp::DashboardValue {
                    knob: KNOBS,
                    value: 0.5
                }
            )
            .is_err()
        );
        apply(
            &mut project,
            link(Target::LayerMaster { layer: id.clone() }),
        )
        .unwrap();
        apply(&mut project, VjOp::RemoveLayer { layer: id }).unwrap();
        assert!(
            project.vj.dashboard[0].links.is_empty(),
            "removing the layer unlinks it"
        );
    }

    #[test]
    fn the_values_of_a_burst_should_coalesce_to_the_last_but_presses_stay() {
        let a = Target::Crossfader;
        let b = Target::Bpm;
        let out = coalesce(vec![
            (a.clone(), Command::Value(0.1)),
            (a.clone(), Command::Value(0.2)),
            (b.clone(), Command::Value(100.0)),
            (a.clone(), Command::Value(0.3)),
            (Target::StopAll, Command::Press),
            (Target::StopAll, Command::Press),
        ]);
        assert_eq!(
            out,
            vec![
                (a.clone(), Command::Value(0.2)),
                (b, Command::Value(100.0)),
                (a, Command::Value(0.3)),
                (Target::StopAll, Command::Press),
                (Target::StopAll, Command::Press),
            ]
        );
    }

    #[test]
    fn a_source_should_match_the_port_by_name_only_when_both_have_one() {
        let heard = cc(7);
        let other_port = Source::Midi {
            port: Some("Launchpad".into()),
            channel: 1,
            kind: MidiKind::Cc,
            number: 7,
        };
        let any_port = Source::Midi {
            port: None,
            channel: 1,
            kind: MidiKind::Cc,
            number: 7,
        };
        assert!(heard.matches(&cc(7)));
        assert!(!other_port.matches(&heard));
        assert!(any_port.matches(&heard));
        assert!(!cc(8).matches(&heard));
        assert!(
            !Source::Osc {
                address: "/a".into()
            }
            .matches(&heard)
        );
    }

    fn osc(project: &Project, address: &str, args: &[OscArg]) -> Option<(Target, Command)> {
        osc_command(&project.vj, address, args)
    }

    #[test]
    fn the_fixed_addresses_should_count_from_one_by_position() {
        let mut project = show();
        let clip = with_generator(&mut project);
        let layers: Vec<String> = project.vj.layers.iter().map(|l| l.id.clone()).collect();
        let columns: Vec<String> = project.vj.decks[0]
            .columns
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let one = [OscArg::Int(1)];
        let half = [OscArg::Float(0.5)];

        assert_eq!(
            osc(&project, "/composition/layers/1/clips/1/connect", &one),
            Some((Target::ClipLaunch { clip: clip.clone() }, Command::Press))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/layers/1/clips/1/connect",
                &[OscArg::Int(0)]
            ),
            Some((Target::ClipLaunch { clip }, Command::Release))
        );
        assert_eq!(
            osc(&project, "/composition/layers/1/clips/2/connect", &one),
            None,
            "an empty cell"
        );
        assert_eq!(
            osc(&project, "/composition/columns/3/connect", &[]),
            Some((
                Target::ColumnLaunch {
                    column: columns[2].clone()
                },
                Command::Press
            )),
            "no value is a press"
        );
        assert_eq!(
            osc(&project, "/composition/layers/2/video/opacity", &half),
            Some((
                Target::LayerOpacity {
                    layer: layers[1].clone()
                },
                Command::Value(0.5)
            ))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/layers/3/master",
                &[OscArg::Float(2.0)]
            ),
            Some((
                Target::LayerMaster {
                    layer: layers[2].clone()
                },
                Command::Value(1.0)
            )),
            "held inside 0 to 1"
        );
        assert_eq!(
            osc(&project, "/composition/layers/1/audio/volume", &half),
            Some((
                Target::LayerVolume {
                    layer: layers[0].clone()
                },
                Command::Value(0.5)
            ))
        );
        assert_eq!(
            osc(&project, "/composition/layers/1/transition/duration", &half),
            Some((
                Target::LayerTransition {
                    layer: layers[0].clone()
                },
                Command::Value(5.0)
            ))
        );
        assert_eq!(
            osc(&project, "/composition/layers/1/bypassed", &one),
            Some((
                Target::LayerBypass {
                    layer: layers[0].clone()
                },
                Command::Switch(true)
            ))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/layers/1/solo",
                &[OscArg::Bool(false)]
            ),
            Some((
                Target::LayerSolo {
                    layer: layers[0].clone()
                },
                Command::Switch(false)
            ))
        );
        assert_eq!(
            osc(&project, "/composition/layers/2/clear", &one),
            Some((
                Target::LayerStop {
                    layer: layers[1].clone()
                },
                Command::Press
            ))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/crossfader/phase",
                &[OscArg::Float(0.25)]
            ),
            Some((Target::Crossfader, Command::Value(0.25)))
        );
        assert_eq!(
            osc(&project, "/composition/layers/9/master", &half),
            None,
            "no ninth layer"
        );
    }

    #[test]
    fn the_tempo_deck_and_dashboard_addresses_should_resolve() {
        let mut project = show();
        apply(&mut project, VjOp::AddDeck).unwrap();
        let decks: Vec<String> = project.vj.decks.iter().map(|d| d.id.clone()).collect();
        let one = [OscArg::Int(1)];

        assert_eq!(
            osc(
                &project,
                "/composition/tempocontroller/tempo",
                &[OscArg::Float(128.0)]
            ),
            Some((Target::Bpm, Command::Value(128.0)))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/tempocontroller/tempo",
                &[OscArg::String("500".into())]
            ),
            Some((Target::Bpm, Command::Value(400.0))),
            "a string with a number, held inside the tempo range"
        );
        for (address, target) in [
            ("tempotap", Target::BpmTap),
            ("resync", Target::BpmResync),
            ("plus", Target::BpmPlus),
            ("minus", Target::BpmMinus),
            ("double", Target::BpmDouble),
            ("half", Target::BpmHalf),
        ] {
            assert_eq!(
                osc(
                    &project,
                    &format!("/composition/tempocontroller/{address}"),
                    &one
                ),
                Some((target.clone(), Command::Press))
            );
            assert_eq!(
                osc(&project, &format!("/tempocontroller/{address}"), &one),
                Some((target, Command::Press)),
                "also without /composition"
            );
        }
        assert_eq!(
            osc(&project, "/composition/tempocontroller/pause", &one),
            Some((Target::BpmPause, Command::Switch(true)))
        );
        assert_eq!(
            osc(&project, "/composition/decks/2/select", &one),
            Some((
                Target::DeckSelect {
                    deck: decks[1].clone()
                },
                Command::Press
            ))
        );
        assert_eq!(
            osc(&project, "/composition/decks/3/select", &one),
            None,
            "no third deck"
        );
        assert_eq!(
            osc(&project, "/composition/decks/next", &one),
            Some((Target::DeckNext, Command::Press))
        );
        assert_eq!(
            osc(&project, "/composition/decks/prev", &one),
            Some((Target::DeckPrev, Command::Press))
        );
        assert_eq!(
            osc(&project, "/composition/disconnectall", &one),
            Some((Target::StopAll, Command::Press))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/dashboard/link8",
                &[OscArg::Float(0.5)]
            ),
            Some((Target::Dashboard { knob: 7 }, Command::Value(0.5)))
        );
        assert_eq!(
            osc(
                &project,
                "/composition/dashboard/link9",
                &[OscArg::Float(0.5)]
            ),
            None
        );
        assert_eq!(
            osc(&project, "/flux/play", &[]),
            None,
            "not a composition address"
        );
    }

    #[test]
    fn the_layers_of_a_global_layer_should_be_found_by_slot() {
        let mut project = show();
        apply(&mut project, VjOp::AddLayer { global: true }).unwrap();
        let layer_id = project.vj.layers[3].id.clone();
        let column = project.vj.decks[0].columns[1].id.clone();
        apply(
            &mut project,
            VjOp::PlaceGenerator {
                generator: crate::generator::Generator::Bars,
                layer: layer_id,
                column,
            },
        )
        .unwrap();
        let clip = project.vj.layers[3].clips[0].id.clone();
        assert_eq!(
            osc(&project, "/composition/layers/4/clips/2/connect", &[]),
            Some((Target::ClipLaunch { clip }, Command::Press))
        );
    }

    #[test]
    fn a_plan_should_flip_a_switch_from_its_state_and_step_decks_round() {
        let mut project = show();
        let id = layer(&project, 0).id.clone();
        let bypass = Target::LayerBypass { layer: id.clone() };
        let Some(Plan::Edit(op)) = plan(&project.vj, &bypass, Command::Toggle) else {
            panic!("an edit")
        };
        assert_eq!(
            op,
            VjOp::LayerBypass {
                layer: id.clone(),
                on: true
            }
        );
        apply(&mut project, op).unwrap();
        let Some(Plan::Edit(op)) = plan(&project.vj, &bypass, Command::Toggle) else {
            panic!("an edit")
        };
        assert_eq!(
            op,
            VjOp::LayerBypass {
                layer: id,
                on: false
            }
        );

        apply(&mut project, VjOp::AddDeck).unwrap();
        let first = project.vj.decks[0].id.clone();
        let Some(Plan::Edit(VjOp::SelectDeck { deck })) =
            plan(&project.vj, &Target::DeckNext, Command::Press)
        else {
            panic!("a deck")
        };
        assert_eq!(
            deck, first,
            "the second deck is shown, next wraps to the first"
        );
        assert_eq!(
            plan(&project.vj, &Target::Crossfader, Command::Value(2.0)),
            Some(Plan::Play(PlayAction::Crossfader(1.0)))
        );
        assert_eq!(plan(&project.vj, &Target::StopAll, Command::Release), None);
    }

    #[test]
    fn the_autopilot_button_should_cycle_through_every_mode() {
        let mut mode = Autopilot::Off;
        let seen: Vec<Autopilot> = (0..4)
            .map(|_| {
                mode = mode.cycled();
                mode
            })
            .collect();
        assert_eq!(
            seen,
            [
                Autopilot::Next,
                Autopilot::Previous,
                Autopilot::Random,
                Autopilot::Off
            ]
        );
    }

    fn key(code: &str, shift: bool) -> Source {
        Source::Key {
            code: code.into(),
            ctrl: false,
            alt: false,
            shift,
            meta: false,
        }
    }

    fn artnet(universe: u16, channel: u16, fine: Option<u16>) -> Source {
        Source::Artnet {
            universe,
            channel,
            fine,
        }
    }

    #[test]
    fn a_dmx_and_a_key_source_should_survive_a_round_trip_and_stay_short_in_the_file() {
        let sources = [
            artnet(1, 37, None),
            artnet(0x0231, 5, Some(6)),
            key("KeyQ", false),
            key("KeyQ", true),
        ];
        for source in sources {
            let json = serde_json::to_string(&source).unwrap();
            assert_eq!(serde_json::from_str::<Source>(&json).unwrap(), source);
        }
        assert_eq!(
            serde_json::to_string(&artnet(1, 37, None)).unwrap(),
            r#"{"type":"artnet","universe":1,"channel":37}"#
        );
        assert_eq!(
            serde_json::to_string(&key("KeyQ", true)).unwrap(),
            r#"{"type":"key","code":"KeyQ","shift":true}"#
        );
    }

    #[test]
    fn a_file_from_before_dmx_keys_and_follow_modes_should_still_load() {
        let old = r#"{"id":"m","source":{"type":"osc","address":"/a"},"target":{"kind":"stopAll"},"min":0.0,"max":1.0}"#;
        let mapping: Mapping = serde_json::from_str(old).unwrap();

        assert_eq!(mapping.follow, Follow::Id);
        assert!(mapping.place.is_none() && !mapping.parked);
        assert!(!serde_json::to_string(&mapping).unwrap().contains("follow"));
    }

    #[test]
    fn dmx_and_key_sources_should_be_checked() {
        assert!(artnet(0, 1, None).validate().is_ok());
        assert!(artnet(MAX_UNIVERSE, 512, Some(511)).validate().is_ok());
        assert!(artnet(MAX_UNIVERSE + 1, 1, None).validate().is_err());
        assert!(artnet(0, 0, None).validate().is_err());
        assert!(artnet(0, 513, None).validate().is_err());
        assert!(
            artnet(0, 5, Some(5)).validate().is_err(),
            "fine is another channel"
        );
        assert!(key("KeyQ", false).validate().is_ok());
        assert!(key("", false).validate().is_err());
        assert!(key("Key Q!", false).validate().is_err());
    }

    #[test]
    fn a_dmx_or_key_source_should_only_match_exactly_itself() {
        assert!(artnet(1, 37, None).matches(&artnet(1, 37, None)));
        assert!(!artnet(1, 37, None).matches(&artnet(2, 37, None)));
        assert!(!artnet(1, 37, Some(38)).matches(&artnet(1, 37, None)));
        assert!(key("KeyQ", false).matches(&key("KeyQ", false)));
        assert!(
            !key("KeyQ", false).matches(&key("KeyQ", true)),
            "Shift+Q is another key"
        );
        assert!(!key("KeyQ", false).matches(&cc(1)));
    }

    #[test]
    fn the_keys_the_app_uses_should_be_refused_and_the_others_taken() {
        let combo = |code: &str, ctrl: bool, shift: bool| Source::Key {
            code: code.into(),
            ctrl,
            alt: false,
            shift,
            meta: false,
        };
        for used in [
            combo("KeyS", true, false),
            combo("KeyS", true, true),
            combo("KeyM", true, true),
            combo("KeyK", true, true),
            combo("KeyY", true, false),
            combo("KeyL", true, false),
            combo("KeyD", true, true),
            combo("KeyL", true, true),
            combo("KeyP", true, true),
            combo("Digit1", true, true),
            combo("Digit9", true, true),
            key("Escape", false),
            key("ShiftLeft", false),
        ] {
            assert!(used.is_app_shortcut(), "{used:?}");
        }
        // Resolume leaves these free by default, so a user can map them.
        for free in [
            combo("KeyM", true, false),
            combo("KeyZ", true, true),
            combo("Digit1", true, false),
            combo("KeyQ", true, false),
            combo("KeyK", false, true),
            key("Space", false),
            key("Enter", false),
            key("Delete", false),
            key("Tab", false),
            key("ArrowLeft", false),
            key("PageUp", false),
            key("KeyB", false),
            key("KeyS", true),
            key("KeyX", false),
            key("Digit1", false),
            key("Digit9", false),
            key("Digit1", true),
            key("Numpad3", false),
            key("KeyQ", false),
            key("F5", false),
            key("Backquote", false),
        ] {
            assert!(!free.is_app_shortcut(), "{free:?}");
        }
        assert!(!cc(1).is_app_shortcut());
    }

    #[test]
    fn learning_a_key_the_app_uses_should_be_refused_by_the_backend() {
        let mut project = show();
        let target = Target::LayerBypass {
            layer: layer(&project, 0).id.clone(),
        };

        let refused = apply(
            &mut project,
            VjOp::AddMapping {
                source: key("Escape", false),
                target: target.clone(),
            },
        );
        let taken = apply(
            &mut project,
            VjOp::AddMapping {
                source: key("Space", false),
                target,
            },
        );

        assert!(refused.is_err());
        assert!(taken.is_ok());
    }

    #[test]
    fn a_key_should_press_and_release_like_a_pad() {
        let mut project = show();
        let clip = with_generator(&mut project);
        apply(
            &mut project,
            VjOp::AddMapping {
                source: key("KeyQ", false),
                target: Target::ClipLaunch { clip: clip.clone() },
            },
        )
        .unwrap();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: key("KeyW", false),
                target: Target::Crossfader,
            },
        )
        .unwrap();
        let mut held = HashSet::new();

        let down = react(
            &project.vj,
            &heard(key("KeyQ", false), 1.0, true),
            &mut held,
        );
        let up = react(
            &project.vj,
            &heard(key("KeyQ", false), 0.0, false),
            &mut held,
        );
        let level_down = react(
            &project.vj,
            &heard(key("KeyW", false), 1.0, true),
            &mut held,
        );
        let level_up = react(
            &project.vj,
            &heard(key("KeyW", false), 0.0, false),
            &mut held,
        );

        assert_eq!(
            down,
            vec![(Target::ClipLaunch { clip: clip.clone() }, Command::Press)]
        );
        assert_eq!(up, vec![(Target::ClipLaunch { clip }, Command::Release)]);
        assert_eq!(
            level_down[0].1,
            Command::Value(1.0),
            "a key at a level goes to the maximum"
        );
        assert!(
            level_up.is_empty(),
            "and stays there unless the mode is hold"
        );
    }

    #[test]
    fn a_held_key_should_press_once_whatever_the_repeats() {
        let mut project = show();
        let clip = with_generator(&mut project);
        apply(
            &mut project,
            VjOp::AddMapping {
                source: key("KeyQ", false),
                target: Target::ClipLaunch { clip },
            },
        )
        .unwrap();
        let mut held = HashSet::new();
        let down = heard(key("KeyQ", false), 1.0, true);

        assert_eq!(react(&project.vj, &down, &mut held).len(), 1);
        assert!(react(&project.vj, &down, &mut held).is_empty());
    }

    #[test]
    fn a_dmx_button_should_toggle_a_switch_on_each_press() {
        let mut project = show();
        let target = Target::LayerBypass {
            layer: layer(&project, 0).id.clone(),
        };
        apply(
            &mut project,
            VjOp::AddMapping {
                source: artnet(1, 10, None),
                target: target.clone(),
            },
        )
        .unwrap();
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.0,
                max: 1.0,
                invert: false,
                mode: ButtonMode::Toggle,
            },
        )
        .unwrap();
        let mut held = HashSet::new();
        let at = |level: f32| heard(artnet(1, 10, None), level, level >= 128.0 / 255.0);

        assert_eq!(
            react(&project.vj, &at(1.0), &mut held),
            vec![(target, Command::Toggle)]
        );
        assert!(
            react(&project.vj, &at(0.9), &mut held).is_empty(),
            "still down"
        );
        assert!(
            react(&project.vj, &at(0.0), &mut held).is_empty(),
            "a release flips nothing"
        );
    }

    #[test]
    fn a_fine_channel_should_be_set_for_a_dmx_mapping_only() {
        let mut project = show();
        let layer_id = layer(&project, 0).id.clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: artnet(1, 37, None),
                target: Target::LayerOpacity {
                    layer: layer_id.clone(),
                },
            },
        )
        .unwrap();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerMaster { layer: layer_id },
            },
        )
        .unwrap();
        let (dmx, midi) = (
            project.vj.mappings[0].id.clone(),
            project.vj.mappings[1].id.clone(),
        );

        apply(
            &mut project,
            VjOp::SetMappingFine {
                mapping: dmx.clone(),
                fine: Some(38),
            },
        )
        .unwrap();
        assert_eq!(project.vj.mappings[0].source, artnet(1, 37, Some(38)));
        assert!(
            apply(
                &mut project,
                VjOp::SetMappingFine {
                    mapping: dmx.clone(),
                    fine: Some(37)
                }
            )
            .is_err()
        );
        assert!(
            apply(
                &mut project,
                VjOp::SetMappingFine {
                    mapping: midi,
                    fine: Some(2)
                }
            )
            .is_err()
        );
        apply(
            &mut project,
            VjOp::SetMappingFine {
                mapping: dmx,
                fine: None,
            },
        )
        .unwrap();
        assert_eq!(project.vj.mappings[0].source, artnet(1, 37, None));
    }

    fn move_clip(project: &mut Project, clip: &str, layer: usize, column: usize) {
        let (layer, column) = (
            project.vj.layers[layer].id.clone(),
            project.vj.decks[0].columns[column].id.clone(),
        );
        apply(
            project,
            VjOp::MoveClip {
                clip: clip.into(),
                layer,
                column,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_new_layer_mapping_should_follow_its_slot_and_a_clip_mapping_itself() {
        let mut project = show();
        let clip = with_generator(&mut project);
        let layer_id = layer(&project, 1).id.clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerOpacity { layer: layer_id },
            },
        )
        .unwrap();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(2),
                target: Target::ClipLaunch { clip },
            },
        )
        .unwrap();

        assert_eq!(project.vj.mappings[0].follow, Follow::Position);
        assert_eq!(project.vj.mappings[1].follow, Follow::Id);
    }

    #[test]
    fn a_slot_mapping_should_follow_a_layer_reorder_and_an_id_mapping_the_layer() {
        let mut project = show();
        let (bottom, middle) = (layer(&project, 0).id.clone(), layer(&project, 1).id.clone());
        for (cc_number, id) in [(1, &bottom), (2, &middle)] {
            apply(
                &mut project,
                VjOp::AddMapping {
                    source: cc(cc_number),
                    target: Target::LayerOpacity { layer: id.clone() },
                },
            )
            .unwrap();
        }
        // The second mapping follows the layer, the first the slot.
        let by_layer = project.vj.mappings[1].id.clone();
        apply(
            &mut project,
            VjOp::SetMappingFollow {
                mapping: by_layer,
                follow: Follow::Id,
            },
        )
        .unwrap();

        apply(
            &mut project,
            VjOp::MoveLayer {
                layer: bottom.clone(),
                to: 2,
            },
        )
        .unwrap();

        let now = |n: usize| project.vj.layers[n].id.clone();
        assert_ne!(now(0), bottom, "the layers did move");
        assert_eq!(
            project.vj.mappings[0].target,
            Target::LayerOpacity { layer: now(0) },
            "slot 0 is whatever is there now"
        );
        assert_eq!(
            project.vj.mappings[1].target,
            Target::LayerOpacity { layer: middle },
            "this layer, wherever it went"
        );
        project.validate().unwrap();
    }

    #[test]
    fn a_clip_mapping_by_slot_should_hit_what_a_swap_leaves_there_and_by_id_the_clip_itself() {
        let mut project = show();
        let first = with_generator(&mut project);
        // A second clip in the next column of the same layer.
        let (layer_id, column) = (
            layer(&project, 0).id.clone(),
            project.vj.decks[0].columns[1].id.clone(),
        );
        apply(
            &mut project,
            VjOp::PlaceGenerator {
                generator: crate::generator::Generator::Strobe {
                    colour: "#ffffff".into(),
                    rate: 8.0,
                    width: 0.5,
                },
                layer: layer_id,
                column,
            },
        )
        .unwrap();
        let second = project.vj.layers[0]
            .clips
            .iter()
            .find(|c| c.id != first)
            .unwrap()
            .id
            .clone();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::ClipLaunch {
                    clip: first.clone(),
                },
            },
        )
        .unwrap();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(2),
                target: Target::ClipSpeed {
                    clip: first.clone(),
                },
            },
        )
        .unwrap();
        let slot = project.vj.mappings[1].id.clone();
        apply(
            &mut project,
            VjOp::SetMappingFollow {
                mapping: slot,
                follow: Follow::Position,
            },
        )
        .unwrap();

        // Move the first clip onto the second's cell: the two swap.
        move_clip(&mut project, &first, 0, 1);

        assert_eq!(
            project.vj.mappings[0].target,
            Target::ClipLaunch {
                clip: first.clone()
            },
            "this clip"
        );
        assert_eq!(
            project.vj.mappings[1].target,
            Target::ClipSpeed { clip: second },
            "the clip that is now in that cell"
        );
        project.validate().unwrap();
    }

    #[test]
    fn a_slot_that_empties_should_park_its_mapping_and_a_clip_that_fills_it_should_wake_it() {
        let mut project = show();
        let clip = with_generator(&mut project);
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(2),
                target: Target::ClipSpeed { clip: clip.clone() },
            },
        )
        .unwrap();
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMappingFollow {
                mapping: id,
                follow: Follow::Position,
            },
        )
        .unwrap();

        move_clip(&mut project, &clip, 1, 3);
        assert!(project.vj.mappings[0].parked, "the cell is empty now");
        assert!(react(&project.vj, &heard(cc(2), 0.5, true), &mut HashSet::new()).is_empty());
        project.validate().unwrap();

        move_clip(&mut project, &clip, 0, 0);
        assert!(!project.vj.mappings[0].parked);
        assert_eq!(project.vj.mappings[0].target, Target::ClipSpeed { clip });
    }

    #[test]
    fn a_follow_mode_should_be_refused_for_a_target_with_no_slot() {
        let mut project = show();
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::Crossfader,
            },
        )
        .unwrap();
        let id = project.vj.mappings[0].id.clone();

        assert!(
            apply(
                &mut project,
                VjOp::SetMappingFollow {
                    mapping: id,
                    follow: Follow::Position
                }
            )
            .is_err()
        );
    }
}
