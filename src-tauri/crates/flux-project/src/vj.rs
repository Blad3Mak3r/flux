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

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

pub use crate::generator::Generator;
use crate::model::{MAX_BPM, MAX_CLIP_NAME, MIN_BPM, Project, Timeline, VideoFit};
pub use crate::vj_effects::{
    Effect, EffectKind, EffectSpec, EffectTarget, MAX_EFFECTS, MirrorAxis, ParamSpec, Sync,
};
pub use crate::vj_groups::{Group, MAX_GROUPS};
pub use crate::vj_mapping::{ButtonMode, Knob, Mapping, Source, Target};
pub use crate::vj_modulation::{ModTarget, Modulation};
use crate::{ProjectError, Result};

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vj {
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// The groups of consecutive layers, bottom first (see [`crate::vj_groups`]); absent is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// In the order of their tabs.
    pub decks: Vec<Deck>,
    /// Id of the deck the grid shows.
    #[serde(default)]
    pub deck: String,
    /// Where the show's timecode clock comes from; absent is [`ClockSource::Internal`].
    #[serde(default, skip_serializing_if = "ClockSource::is_internal")]
    pub clock: ClockSource,
    /// The show's tempo and beat phase are shared with the other applications of an Ableton Link
    /// session on the network; absent is off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub link: bool,
    /// The composition's effects, applied to the final mix of the layers before the output mapping,
    /// the first applied first. Only here the kinds that need the frames before them (feedback,
    /// trails, bloom) are allowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composition_effects: Vec<Effect>,
    /// Draws the composition without its effects; they keep their place and settings.
    #[serde(default, skip_serializing_if = "is_false")]
    pub composition_bypass: bool,
    /// What a MIDI controller or an OSC address drives (see [`crate::vj_mapping`]); absent is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<Mapping>,
    /// The composition dashboard: up to [`crate::vj_mapping::KNOBS`] knobs, each driving the numbers
    /// it is linked to; a knob not listed is at 0 with no links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboard: Vec<Knob>,
    /// The colours a controller's pads get for each state when a mapping has none of its own;
    /// absent is the controller's preset (see [`crate::vj_feedback`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lights: Option<crate::vj_feedback::Lights>,
    /// The OSC destinations (`host:port`) the grid's state is echoed to, on the addresses that
    /// control it (see [`crate::vj_osc_feedback`]); absent is none, and nothing is sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub osc_feedback: Vec<String>,
}

/// Where the timecode clock of a VJ show comes from: Flux's own transport, or timecode coming in
/// by LTC or MTC, which the clock follows and the operator cannot move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockSource {
    #[default]
    Internal,
    Ltc,
    Mtc,
}

impl ClockSource {
    fn is_internal(&self) -> bool {
        *self == Self::Internal
    }
}

/// Taps further apart than this start a new count.
const TAP_GAP_MS: f64 = 2000.0;
/// How many of the last taps make the tempo.
const TAP_COUNT: usize = 8;

/// The tempo of `taps` (milliseconds, oldest first): the mean time between the last run of taps
/// that are closer than [`TAP_GAP_MS`], at most [`TAP_COUNT`] of them. `None` before two taps.
pub fn tapped_bpm(taps: &[f64]) -> Option<f64> {
    let mut run = taps.len().checked_sub(1)?;
    while run > 0 && taps.len() - run < TAP_COUNT {
        let gap = taps[run] - taps[run - 1];
        if !(gap > 0.0 && gap <= TAP_GAP_MS) {
            break;
        }
        run -= 1;
    }
    let taps = &taps[run..];
    if taps.len() < 2 {
        return None;
    }
    let mean = (taps[taps.len() - 1] - taps[0]) / (taps.len() - 1) as f64;
    (mean > 0.0).then(|| 60_000.0 / mean)
}

/// Sets the tempo of the show to `change` of the one it has. It is kept in the show's timeline, so
/// the metronome and the engine follow it, brought into range and to two decimals.
fn set_bpm(timelines: &mut [Timeline], change: impl FnOnce(f64) -> f64) -> Result<()> {
    let timeline = timelines
        .first_mut()
        .ok_or_else(|| ProjectError::Invalid("a VJ project keeps one timeline".into()))?;
    let bpm = change(timeline.tempo.bpm);
    if !bpm.is_finite() {
        return Err(ProjectError::Invalid("a tempo is a number".into()));
    }
    timeline.tempo.bpm = (bpm.clamp(MIN_BPM, MAX_BPM) * 100.0).round() / 100.0;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    pub id: String,
    /// Absent is the deck's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Left first.
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub id: String,
    /// Absent is the column's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    /// Absent is the layer's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A global layer keeps its clips in slots, the same in every deck; chosen when the layer is
    /// added.
    #[serde(default, skip_serializing_if = "is_false")]
    pub global: bool,
    /// The layer's master, 0 to 1: scales both its picture and its sound. Absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub master: f32,
    /// How much the picture covers the layers below, 0 to 1; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub opacity: f32,
    /// The level of the sound of the layer's clips, 0 to 1 as the fader stands; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub volume: f32,
    /// How the picture mixes with the layers below; absent is [`Blend::Alpha`].
    #[serde(default, skip_serializing_if = "Blend::is_alpha")]
    pub blend: Blend,
    /// How long a clip launched on the layer takes to replace the one playing, in seconds, 0 to
    /// [`MAX_TRANSITION`]; absent is 0, a cut.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transition: f32,
    /// How the clip arriving mixes with the one leaving during a transition, any blend (Resolume's
    /// transition mode); absent is [`Blend::Alpha`], a crossfade.
    #[serde(default, skip_serializing_if = "Blend::is_alpha")]
    pub transition_effect: Blend,
    /// Whether each transition on the layer picks a blend at random, from all of them, instead of
    /// using [`Self::transition_effect`] (Resolume's Random transition); absent is off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub transition_random: bool,
    /// What the layer launches when the clip playing on it ends a pass; absent is [`Autopilot::Off`].
    #[serde(default, skip_serializing_if = "Autopilot::is_off")]
    pub autopilot: Autopilot,
    /// The side of the crossfader the layer is on; absent is neither, so the crossfader leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossfade: Option<Side>,
    /// Bypassed: the layer shows nothing and sounds nothing, though its clip keeps playing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Soloed: while any layer is, only the soloed ones show.
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// Folded to a thin row on the grid, as in Resolume; it plays as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// The id of the group the layer is in; the layers of a group are next to each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The layer's effects, drawn on each of its clips' pictures after the clip's own; the first
    /// is applied first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// What drives `master`, `opacity` or `volume` by itself (see [`crate::vj_modulation`]), by
    /// field. The stored value is the one the fader stands at.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    /// At most one per column of each deck (per slot on a global layer), in no particular order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
}

pub(crate) fn full() -> f32 {
    1.0
}

pub(crate) fn is_full(value: &f32) -> bool {
    *value == 1.0
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

impl Layer {
    pub(crate) fn new(id: String, global: bool) -> Self {
        Self {
            id,
            name: None,
            global,
            master: 1.0,
            opacity: 1.0,
            volume: 1.0,
            blend: Blend::Alpha,
            transition: 0.0,
            transition_effect: Blend::Alpha,
            transition_random: false,
            autopilot: Autopilot::Off,
            crossfade: None,
            bypass: false,
            solo: false,
            collapsed: false,
            group: None,
            effects: Vec::new(),
            modulation: BTreeMap::new(),
            clips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    A,
    B,
}

/// How a layer's picture mixes with what the layers below show, at the layer's opacity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    /// Covers what is below.
    #[default]
    Alpha,
    /// Adds its light to what is below.
    Add,
    /// Takes its light from what is below.
    Subtract,
    /// Darkens what is below by its colour.
    Multiply,
    /// Lightens what is below, softer than adding.
    Screen,
    /// Keeps the lighter of the two.
    Lighten,
    /// Keeps the darker of the two.
    Darken,
    /// The distance between the two lights.
    Difference,
    /// Like [`Self::Difference`], with less contrast.
    Exclusion,
    /// Multiplies where what is below is dark, screens where it is light.
    Overlay,
    /// A gentle [`Self::Overlay`]-like contrast, driven by the layer.
    SoftLight,
    /// [`Self::Overlay`] driven by the layer's light.
    HardLight,
    /// Brightens what is below by the layer, to overexposure (Resolume's Dodge).
    ColorDodge,
    /// Darkens what is below by the layer, with more contrast than multiply (Resolume's Burn).
    ColorBurn,
    /// Burns or dodges linearly by the layer's light.
    LinearLight,
    /// Burns or dodges by the layer's light, with more contrast than linear light.
    VividLight,
    /// Keeps the darker or the lighter of the two by the layer's light.
    PinLight,
    /// The layer's hue on the saturation and luminosity below.
    Hue,
    /// The layer's saturation on the hue and luminosity below.
    Saturation,
    /// The layer's hue and saturation on the luminosity below.
    Color,
    /// The layer's luminosity on the hue and saturation below.
    Luminosity,
    /// A hard cut by the layer: where its cover is at least half it replaces what is below, elsewhere it leaves it (Resolume's AlphaCut).
    AlphaCut,
    /// The layer's dark parts turn transparent: its alpha is multiplied by its luma (Resolume's Luma Key).
    LumaKey,
    /// The layer's bright parts turn transparent: its alpha is multiplied by one minus its luma (Resolume's Luma Key I).
    LumaKeyInverted,
    /// The layer's luma replaces its own alpha: bright is opaque, dark is transparent, wherever it has a picture.
    LumaIsAlpha,
    /// The inverse of the distance between the two lights, a lighter picture (Resolume's Difference I).
    DifferenceInverted,
    /// The layer acts as a mask: what is below shows where it is bright and goes dark where it is dark; it is not drawn (Resolume's 50Mask).
    Mask50,
    /// The layer takes over what is below one channel at a time as its cover rises: blue, then green, then red.
    Rgb,
    /// The layer shifts what is below sideways by its luma, none at mid grey; it is not drawn.
    Displace,
}

impl Blend {
    /// Every blend, for the random transition to pick from.
    pub const ALL: [Blend; 29] = [
        Blend::Alpha,
        Blend::Add,
        Blend::Subtract,
        Blend::Multiply,
        Blend::Screen,
        Blend::Lighten,
        Blend::Darken,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Overlay,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::ColorDodge,
        Blend::ColorBurn,
        Blend::LinearLight,
        Blend::VividLight,
        Blend::PinLight,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
        Blend::AlphaCut,
        Blend::LumaKey,
        Blend::LumaKeyInverted,
        Blend::LumaIsAlpha,
        Blend::DifferenceInverted,
        Blend::Mask50,
        Blend::Rgb,
        Blend::Displace,
    ];

    pub(crate) fn is_alpha(&self) -> bool {
        *self == Self::Alpha
    }
}

/// Which clip a layer launches by itself when the one playing ends a pass (Resolume's Layer
/// Autopilot): the next or previous one of the layer in the deck shown, or a random one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Autopilot {
    #[default]
    Off,
    Next,
    Previous,
    /// Never the clip that played, when there is another.
    Random,
}

impl Autopilot {
    fn is_off(&self) -> bool {
        *self == Self::Off
    }

    /// The mode after this one, as the cycle button steps: off, next, previous, random, off.
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::Next,
            Self::Next => Self::Previous,
            Self::Previous => Self::Random,
            Self::Random => Self::Off,
        }
    }

    /// The cell to launch after `current` among `filled` (one entry per column, `true` for a clip),
    /// skipping empty cells and wrapping around; `roll` is a random number, used by
    /// [`Self::Random`]. `None` when the layer has no clip in the deck, or autopilot is off. A layer
    /// whose only clip is `current` launches it again.
    pub fn pick(self, filled: &[bool], current: Option<usize>, roll: u64) -> Option<usize> {
        let n = filled.len();
        match self {
            Self::Off => None,
            Self::Next | Self::Previous => {
                let forward = self == Self::Next;
                (1..=n)
                    .map(|k| match (current, forward) {
                        (Some(c), true) => (c + k) % n,
                        (Some(c), false) => (c + n - k % n) % n,
                        (None, true) => k - 1,
                        (None, false) => n - k,
                    })
                    .find(|&at| filled[at])
            }
            Self::Random => {
                let others: Vec<usize> = (0..n)
                    .filter(|&at| filled[at] && Some(at) != current)
                    .collect();
                match others.len() {
                    0 => current.filter(|&at| filled.get(at) == Some(&true)),
                    len => Some(others[(roll % len as u64) as usize]),
                }
            }
        }
    }
}

/// What launching a clip by hand does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// A press launches the clip, which plays on.
    #[default]
    Normal,
    /// The clip plays while it is held down and stops when it is let go.
    Piano,
}

impl Trigger {
    fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}

/// A video, or a generator, in a cell of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    /// On a deck layer, the id of the column the clip is in; the layer is the one that lists it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// On a global layer, the place of the clip from the left, the same in every deck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    /// Id of an entry in `Project::videos`; empty (absent) for a generator.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asset: String,
    /// What makes the clip's picture when it is not a video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
    /// Absent is the video's name, or the generator's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the clip does when it reaches its end; absent is [`Playback::Loop`].
    #[serde(default, skip_serializing_if = "Playback::is_loop")]
    pub playback: Playback,
    /// What a press on the clip does; absent is [`Trigger::Normal`].
    #[serde(default, skip_serializing_if = "Trigger::is_normal")]
    pub trigger: Trigger,
    /// How fast the clip plays when its [`tempo`](Self::tempo) is free: 1 is its own speed, from
    /// [`MIN_SPEED`] to [`MAX_SPEED`]. Absent is 1.
    #[serde(default = "full_speed", skip_serializing_if = "is_full_speed")]
    pub speed: f32,
    /// How long a still image or a generator lasts, in seconds, from [`MIN_DURATION`] to
    /// [`MAX_DURATION`]: the length its transport (region, cues, loop, once, tempo) runs over, as
    /// a video's file does for a video, which ignores it. Absent is [`DEFAULT_DURATION`].
    #[serde(
        default = "default_duration",
        skip_serializing_if = "is_default_duration"
    )]
    pub duration: f32,
    /// Which way the clip plays; absent is [`Direction::Forward`].
    #[serde(default, skip_serializing_if = "Direction::is_forward")]
    pub direction: Direction,
    /// Whether a pass lasts as long as the clip says or a number of beats; absent is
    /// [`Tempo::Free`].
    #[serde(default, skip_serializing_if = "Tempo::is_free")]
    pub tempo: Tempo,
    /// The part of the clip that plays and repeats, as fractions of its length; absent is all of it.
    #[serde(default, skip_serializing_if = "Region::is_whole")]
    pub region: Region,
    /// Places the clip jumps to, as fractions of its length; absent are all empty.
    #[serde(default, skip_serializing_if = "no_cues")]
    pub cues: [Option<f32>; CUES],
    /// How the video is sized into the composition; absent is [`VideoFit::Fit`].
    #[serde(default, skip_serializing_if = "is_fit")]
    pub fit: VideoFit,
    /// How much larger than its fit the clip is drawn, about its centre: 1 is 100%. Absent is 1.
    #[serde(default = "full_scale", skip_serializing_if = "is_full_scale")]
    pub scale: f32,
    /// How far its centre is moved from where its fit puts it, as fractions of the composition's
    /// width and height, right and down. Absent is not moved.
    #[serde(default, skip_serializing_if = "is_centred")]
    pub position: [f32; 2],
    /// The clip's effects, first applied first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// What drives `scale`, `positionX` or `positionY` by itself (see [`crate::vj_modulation`]), by
    /// field.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
}

fn is_fit(fit: &VideoFit) -> bool {
    *fit == VideoFit::Fit
}

fn full_scale() -> f32 {
    1.0
}

fn is_full_scale(scale: &f32) -> bool {
    *scale == 1.0
}

fn is_centred(position: &[f32; 2]) -> bool {
    *position == [0.0, 0.0]
}

fn default_duration() -> f32 {
    DEFAULT_DURATION
}

fn is_default_duration(duration: &f32) -> bool {
    *duration == DEFAULT_DURATION
}

fn full_speed() -> f32 {
    1.0
}

fn is_full_speed(speed: &f32) -> bool {
    *speed == 1.0
}

fn no_cues(cues: &[Option<f32>; CUES]) -> bool {
    cues.iter().all(Option::is_none)
}

/// Slowest and fastest a clip plays, times its own speed.
pub const MIN_SPEED: f32 = 0.05;
pub const MAX_SPEED: f32 = 8.0;
/// The length of a still image or a generator, in seconds: the shortest, the longest and the one a
/// new clip has.
pub const MIN_DURATION: f32 = 0.1;
pub const MAX_DURATION: f32 = 3600.0;
pub const DEFAULT_DURATION: f32 = 10.0;
/// The shortest region of a clip, as a fraction of its length.
pub const MIN_REGION: f32 = 0.001;
/// How many cues a clip has.
pub const CUES: usize = 8;
/// The lengths, in beats, that one pass of a clip can be made to last.
pub const TEMPO_BEATS: [u32; 8] = [1, 2, 4, 8, 16, 32, 64, 128];

/// Which way a clip plays through its region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    #[default]
    Forward,
    Reverse,
    /// Holds the picture where it is, and is silent.
    Pause,
}

impl Direction {
    fn is_forward(&self) -> bool {
        *self == Self::Forward
    }

    /// 1 forwards, -1 backwards, 0 paused.
    pub fn sign(self) -> f64 {
        match self {
            Self::Forward => 1.0,
            Self::Reverse => -1.0,
            Self::Pause => 0.0,
        }
    }
}

/// How long one pass through a clip's region lasts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Tempo {
    /// As long as the clip's speed makes it.
    #[default]
    Free,
    /// Exactly this many beats of the composition's tempo, one of [`TEMPO_BEATS`]; the speed does
    /// not count.
    Beats { beats: u32 },
}

impl Tempo {
    fn is_free(&self) -> bool {
        *self == Self::Free
    }
}

/// The part of a clip that plays, as fractions of its length: `start` below `end`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub start: f32,
    pub end: f32,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 1.0,
        }
    }
}

impl Region {
    fn is_whole(&self) -> bool {
        *self == Self::default()
    }

    /// The region with both ends brought into 0 to 1 and at least [`MIN_REGION`] apart: the start
    /// is kept unless there is no room for the end after it, and the end gives way to it.
    pub fn tidy(start: f32, end: f32) -> Result<Self> {
        if !start.is_finite() || !end.is_finite() {
            return Err(ProjectError::Invalid("a region is made of numbers".into()));
        }
        let start = start.clamp(0.0, 1.0 - MIN_REGION);
        let end = end.clamp(start + MIN_REGION, 1.0);
        Ok(Self { start, end })
    }

    /// Its length, as a fraction of the clip's.
    pub fn span(&self) -> f32 {
        self.end - self.start
    }

    fn valid(&self) -> bool {
        self.start.is_finite()
            && self.end.is_finite()
            && self.start >= 0.0
            && self.end <= 1.0
            && self.end - self.start >= MIN_REGION * 0.999
    }
}

/// Largest scale of a clip: 1000%.
pub const MAX_SCALE: f32 = 10.0;
/// Farthest a clip's centre moves from its place, in compositions.
pub const MAX_OFFSET: f32 = 2.0;

/// What a launched clip does when it reaches the end of its video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Playback {
    /// Starts again from the beginning.
    #[default]
    Loop,
    /// Stops, and the layer shows nothing.
    Once,
    /// Plays backwards to the beginning, then forwards again.
    PingPong,
}

impl Playback {
    fn is_loop(&self) -> bool {
        *self == Self::Loop
    }
}

/// Where a clip sits: a column of a deck, or a slot of a global layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Cell {
    Column(String),
    Slot(usize),
}

impl Clip {
    fn cell(&self) -> Option<Cell> {
        match (&self.column, self.slot) {
            (Some(column), None) => Some(Cell::Column(column.clone())),
            (None, Some(slot)) => Some(Cell::Slot(slot)),
            _ => None,
        }
    }

    fn put(&mut self, cell: Cell) {
        match cell {
            Cell::Column(column) => (self.column, self.slot) = (Some(column), None),
            Cell::Slot(slot) => (self.column, self.slot) = (None, Some(slot)),
        }
    }
}

/// A fader of a layer (a group has the first two only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Master,
    Opacity,
    Volume,
    Transition,
}

/// What the user can do to the grid. Every id names something of the grid; a name that is empty
/// after trimming goes back to the default one. A level out of its range is brought into it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VjOp {
    /// A new empty layer on top of the others, global or of the decks.
    AddLayer {
        #[serde(default)]
        global: bool,
    },
    /// Takes the layer and its clips away; the last layer stays.
    RemoveLayer {
        layer: String,
    },
    /// Puts the layer at `to` of the drawing order, 0 being the bottom.
    MoveLayer {
        layer: String,
        to: usize,
    },
    RenameLayer {
        layer: String,
        name: String,
    },
    LayerMaster {
        layer: String,
        value: f32,
    },
    LayerOpacity {
        layer: String,
        value: f32,
    },
    LayerVolume {
        layer: String,
        value: f32,
    },
    /// Brings one fader of the layer back to the value a new layer has.
    LayerLevelReset {
        layer: String,
        level: Level,
    },
    LayerBlend {
        layer: String,
        blend: Blend,
    },
    /// In seconds.
    LayerTransition {
        layer: String,
        value: f32,
    },
    /// With `random`, every transition of the layer picks its own blend and `effect` is kept for
    /// when it is turned off.
    LayerTransitionEffect {
        layer: String,
        effect: Blend,
        #[serde(default)]
        random: bool,
    },
    LayerCrossfade {
        layer: String,
        side: Option<Side>,
    },
    /// What the layer launches by itself at the end of a clip's pass.
    LayerAutopilot {
        layer: String,
        mode: Autopilot,
    },
    LayerBypass {
        layer: String,
        on: bool,
    },
    LayerSolo {
        layer: String,
        on: bool,
    },
    /// Folds the layer's row on the grid, or unfolds it.
    LayerCollapse {
        layer: String,
        on: bool,
    },
    /// Puts the layers in a new group (Resolume's New Group): they gather where the lowest of
    /// them is, in their order, and leave the groups they were in.
    GroupLayers {
        layers: Vec<String>,
    },
    /// Puts the layer in `group`, on its top unless `to` says where in the drawing order, or in
    /// none. A group never stays empty: the layer that was its last leaves a new empty one behind.
    LayerGroup {
        layer: String,
        group: Option<String>,
        to: Option<usize>,
    },
    /// Takes the group away; its layers stay as they are, outside any group.
    Ungroup {
        group: String,
    },
    RenameGroup {
        group: String,
        name: String,
    },
    GroupMaster {
        group: String,
        value: f32,
    },
    GroupOpacity {
        group: String,
        value: f32,
    },
    /// Brings the group's master or video fader back to the value a new group has.
    GroupLevelReset {
        group: String,
        level: Level,
    },
    GroupBlend {
        group: String,
        blend: Blend,
    },
    GroupBypass {
        group: String,
        on: bool,
    },
    GroupSolo {
        group: String,
        on: bool,
    },
    /// Folds the group's header and layers on the grid, or unfolds them.
    GroupCollapse {
        group: String,
        on: bool,
    },
    /// The tempo of the show, in beats a minute.
    SetBpm {
        bpm: f64,
    },
    /// The tempo moved by `by` beats a minute.
    NudgeBpm {
        by: f64,
    },
    /// The tempo times `factor`, as the ×2 and ÷2 buttons.
    ScaleBpm {
        factor: f64,
    },
    /// The tempo of the taps on the tap button, in milliseconds (see [`tapped_bpm`]).
    TapBpm {
        taps: Vec<f64>,
    },
    /// Where the timecode clock comes from.
    SetClock {
        source: ClockSource,
    },
    /// Joins the Ableton Link session, or leaves it.
    SetLink {
        on: bool,
    },
    /// A new deck with [`DEFAULT_COLUMNS`] empty columns, after the others, which the grid shows.
    AddDeck,
    /// Takes the deck, its columns and their clips away; the last deck stays.
    RemoveDeck {
        deck: String,
    },
    MoveDeck {
        deck: String,
        to: usize,
    },
    RenameDeck {
        deck: String,
        name: String,
    },
    /// Shows the deck; what plays keeps playing.
    SelectDeck {
        deck: String,
    },
    /// A new empty column on the right of the deck the grid shows.
    AddColumn,
    /// Takes the column and its clips away; the last column of a deck stays.
    RemoveColumn {
        column: String,
    },
    /// Moves the column to `to` within its deck.
    MoveColumn {
        column: String,
        to: usize,
    },
    RenameColumn {
        column: String,
        name: String,
    },
    /// Puts a video of the project in a cell, in place of the clip that was there. The cell is the
    /// layer's in `column`; on a global layer, the slot at the place of `column` in its deck.
    PlaceVideo {
        asset: String,
        layer: String,
        column: String,
    },
    /// Puts a new generator in a cell, named as for [`VjOp::PlaceVideo`].
    PlaceGenerator {
        generator: Generator,
        layer: String,
        column: String,
    },
    /// Moves a clip to a cell, named as for [`VjOp::PlaceVideo`]; a clip already there takes the
    /// place the moved one left.
    MoveClip {
        clip: String,
        layer: String,
        column: String,
    },
    RemoveClip {
        clip: String,
    },
    RenameClip {
        clip: String,
        name: String,
    },
    ClipPlayback {
        clip: String,
        playback: Playback,
    },
    /// How fast the clip plays, times its own speed, from [`MIN_SPEED`] to [`MAX_SPEED`].
    ClipSpeed {
        clip: String,
        value: f32,
    },
    /// How long a still image or a generator lasts, in seconds, from [`MIN_DURATION`] to
    /// [`MAX_DURATION`]; a video keeps the length of its file.
    ClipDuration {
        clip: String,
        value: f32,
    },
    ClipDirection {
        clip: String,
        direction: Direction,
    },
    /// Free, or a pass of so many beats; the beats go to the nearest of [`TEMPO_BEATS`].
    ClipTempo {
        clip: String,
        tempo: Tempo,
    },
    /// The part of the clip that plays, as fractions of its length; cues stay where they are.
    ClipRegion {
        clip: String,
        start: f32,
        end: f32,
    },
    /// Puts cue `index` (0 to 7) of the clip at `position`, a fraction of its length, or empties
    /// it with `None`.
    ClipCue {
        clip: String,
        index: usize,
        position: Option<f32>,
    },
    /// Changes what a generator clip draws; a video clip stays a video.
    ClipGenerator {
        clip: String,
        generator: Generator,
    },
    /// Moves the line a text block shows (its `line`) by `step` lines, skipping the empty ones and
    /// wrapping round. A video clip or another generator refuses it.
    TextLine {
        clip: String,
        step: i32,
    },
    ClipTrigger {
        clip: String,
        trigger: Trigger,
    },
    ClipFit {
        clip: String,
        fit: VideoFit,
    },
    /// Draws the clip `value` times its fitted size (1 is 100%), up to [`MAX_SCALE`].
    ClipScale {
        clip: String,
        value: f32,
    },
    /// Moves the clip's centre from where its fit puts it, in fractions of the composition, up to
    /// [`MAX_OFFSET`] either way.
    ClipPosition {
        clip: String,
        x: f32,
        y: f32,
    },
    /// Puts a new effect of the kind called `kind` (see [`crate::vj_effects::catalog`]), with the settings
    /// it starts with, at the end of the stack of a clip, of a layer or of the composition; a stack
    /// holds at most [`MAX_EFFECTS`]. The kinds that are for the composition only are refused on a
    /// clip or a layer.
    AddEffect {
        target: EffectTarget,
        kind: String,
    },
    /// Skips (or brings back) the whole composition stack, whatever each effect's own bypass says.
    CompositionBypass {
        bypass: bool,
    },
    RemoveEffect {
        effect: String,
    },
    /// Puts the effect at `index` of its stack, 0 being the first applied.
    MoveEffect {
        effect: String,
        index: usize,
    },
    /// Gives the effect the settings of a preset of its kind (see [`crate::vj_presets`]); what the
    /// preset lacks keeps its value.
    ApplyEffectPreset {
        effect: String,
        preset: crate::vj_presets::EffectPreset,
    },
    /// Puts the effects of a preset, with new ids, at the end of a stack.
    AddEffectPreset {
        target: EffectTarget,
        preset: crate::vj_presets::EffectPreset,
    },
    /// Sets the effect's settings; `kind` is the kind it already is.
    EffectParams {
        effect: String,
        kind: EffectKind,
    },
    EffectBypass {
        effect: String,
        bypass: bool,
    },
    /// Puts a copy of the effect, with a new id and the same settings, right after it; a stack
    /// holds at most [`MAX_EFFECTS`].
    DuplicateEffect {
        effect: String,
    },
    /// Gives the effect the settings its kind starts with, and takes its modulation away.
    ResetEffect {
        effect: String,
    },
    /// Sets what drives a numeric setting by itself (see [`crate::vj_modulation`]), or with `None`
    /// takes it away. The values are brought into the setting's range.
    SetModulation {
        target: ModTarget,
        modulation: Option<Modulation>,
    },
    /// Makes `source` (a MIDI control or an OSC address, as learned) drive `target`. Latest wins: a
    /// mapping of the same source or of the same target is replaced, and a target that is mapped
    /// again keeps the range, inversion and mode it had.
    AddMapping {
        source: Source,
        target: Target,
    },
    RemoveMapping {
        mapping: String,
    },
    /// Takes away the mapping of `target`, if it has one.
    ClearMapping {
        target: Target,
    },
    /// Sets the options of a mapping: the range it scales into (in the target's own units),
    /// whether it is inverted, and what a button does.
    SetMapping {
        mapping: String,
        min: f32,
        max: f32,
        invert: bool,
        mode: ButtonMode,
    },
    /// Pairs a DMX mapping with the `fine` channel of its 16-bit value, or back to 8 bits with
    /// `None`. Only a DMX source has one.
    SetMappingFine {
        mapping: String,
        fine: Option<u16>,
    },
    /// Sets the values a mapping's pad is sent for its states, or with `None` takes them away so
    /// the grid's colours apply.
    SetMappingLights {
        mapping: String,
        lights: Option<crate::vj_feedback::Lights>,
    },
    /// Sets the colours the pads of mappings with none of their own get, or with `None` goes
    /// back to the controller's preset.
    SetLights {
        lights: Option<crate::vj_feedback::Lights>,
    },
    /// Sets where the OSC feedback goes: `host:port` separated by commas or spaces, at most 4;
    /// an empty text turns it off.
    SetOscFeedback {
        destinations: String,
    },
    /// Sets how a mapping of a clip, layer, column, deck or effect follows it: by id, or by the
    /// slot it is in.
    SetMappingFollow {
        mapping: String,
        follow: crate::vj_mapping::Follow,
    },
    /// Turns a knob of the dashboard to `value` (0 to 1) and moves every number linked to it.
    DashboardValue {
        knob: usize,
        value: f32,
    },
    /// Links a number to a knob (or changes the range of an existing link): the knob at 0 gives
    /// `min` and at 1 gives `max`. A bound left out is the target's own, so a new link covers its
    /// whole range.
    DashboardLink {
        knob: usize,
        target: Target,
        min: Option<f32>,
        max: Option<f32>,
    },
    DashboardUnlink {
        knob: usize,
        target: Target,
    },
    RenameKnob {
        knob: usize,
        name: String,
    },
}

impl VjOp {
    /// Whether the op only moves a value, so that a fader or a controller can send it many times
    /// a second without the project being copied and checked each time (see `Session::preview_vj`).
    pub fn is_preview(&self) -> bool {
        matches!(
            self,
            Self::LayerMaster { .. }
                | Self::LayerOpacity { .. }
                | Self::GroupMaster { .. }
                | Self::GroupOpacity { .. }
                | Self::LayerVolume { .. }
                | Self::LayerTransition { .. }
                | Self::ClipScale { .. }
                | Self::ClipPosition { .. }
                | Self::ClipSpeed { .. }
                | Self::ClipRegion { .. }
                | Self::ClipGenerator { .. }
                | Self::EffectParams { .. }
                | Self::SetModulation { .. }
                | Self::DashboardValue { .. }
        )
    }

    /// Whether the op can take something a mapping or a link names out of the grid.
    fn may_remove(&self) -> bool {
        matches!(
            self,
            Self::RemoveLayer { .. }
                | Self::GroupLayers { .. }
                | Self::LayerGroup { .. }
                | Self::Ungroup { .. }
                | Self::RemoveDeck { .. }
                | Self::RemoveColumn { .. }
                | Self::RemoveClip { .. }
                | Self::RemoveEffect { .. }
                | Self::PlaceVideo { .. }
                | Self::PlaceGenerator { .. }
                | Self::MoveClip { .. }
                | Self::ClipGenerator { .. }
        )
    }
}

impl Vj {
    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty()
            && self.decks.is_empty()
            && self.composition_effects.is_empty()
            && self.mappings.is_empty()
            && self.dashboard.is_empty()
            && self.lights.is_none()
            && self.osc_feedback.is_empty()
    }

    /// The grid of a new VJ project: [`DEFAULT_LAYERS`] layers and one deck of [`DEFAULT_COLUMNS`]
    /// columns.
    pub fn starting(mut new_id: impl FnMut() -> String) -> Self {
        let layers = (0..DEFAULT_LAYERS)
            .map(|_| Layer::new(new_id(), false))
            .collect();
        let deck = new_deck(&mut new_id);
        Self {
            layers,
            groups: Vec::new(),
            deck: deck.id.clone(),
            decks: vec![deck],
            clock: ClockSource::Internal,
            link: false,
            composition_effects: Vec::new(),
            composition_bypass: false,
            mappings: Vec::new(),
            dashboard: Vec::new(),
            lights: None,
            osc_feedback: Vec::new(),
        }
    }

    /// Every column of every deck.
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.decks.iter().flat_map(|deck| &deck.columns)
    }

    /// The deck the grid shows.
    pub fn shown_deck(&self) -> Option<&Deck> {
        self.decks.iter().find(|deck| deck.id == self.deck)
    }

    /// The deck holding `column`, and the column's place in it.
    pub fn column_place(&self, column: &str) -> Option<(&Deck, usize)> {
        self.decks.iter().find_map(|deck| {
            let at = deck.columns.iter().position(|c| c.id == column)?;
            Some((deck, at))
        })
    }

    /// The clips of the column launched together, by layer: every deck layer's clip in the
    /// column, `None` for a deck layer without one. Global layers are not in it.
    pub fn column_clips<'a>(
        &'a self,
        column: &'a str,
    ) -> impl Iterator<Item = (&'a Layer, Option<&'a Clip>)> {
        self.layers
            .iter()
            .filter(|layer| !layer.global)
            .map(move |layer| {
                let clip = layer
                    .clips
                    .iter()
                    .find(|clip| clip.column.as_deref() == Some(column));
                (layer, clip)
            })
    }

    /// The first column of the deck shown where `layer` has no clip (no slot, on a global layer).
    pub fn empty_column(&self, layer: &str) -> Option<String> {
        let layer = self.layers.iter().find(|l| l.id == layer)?;
        let deck = self.shown_deck()?;
        deck.columns
            .iter()
            .enumerate()
            .find(|(at, column)| {
                !layer.clips.iter().any(|clip| match clip.cell() {
                    Some(Cell::Column(c)) => c == column.id,
                    Some(Cell::Slot(s)) => s == *at,
                    None => false,
                })
            })
            .map(|(_, column)| column.id.clone())
    }

    /// The clip `layer` launches after `current` by its autopilot, among its clips in the deck
    /// shown (a column each, the same place on a global layer's slots); `roll` is a random number.
    pub fn autopilot_next<'a>(
        &self,
        layer: &'a Layer,
        current: &str,
        roll: u64,
    ) -> Option<&'a Clip> {
        let deck = self.shown_deck()?;
        let cells: Vec<Option<&Clip>> = deck
            .columns
            .iter()
            .enumerate()
            .map(|(at, column)| {
                layer.clips.iter().find(|clip| match clip.cell() {
                    Some(Cell::Column(c)) => c == column.id,
                    Some(Cell::Slot(s)) => s == at,
                    None => false,
                })
            })
            .collect();
        let filled: Vec<bool> = cells.iter().map(Option::is_some).collect();
        let here = cells
            .iter()
            .position(|clip| clip.is_some_and(|clip| clip.id == current));
        layer
            .autopilot
            .pick(&filled, here, roll)
            .and_then(|at| cells[at])
    }

    /// The effect `id` of a clip, a layer or the composition.
    pub fn find_effect(&self, id: &str) -> Option<&Effect> {
        let layers = self.layers.iter().flat_map(|layer| {
            layer
                .effects
                .iter()
                .chain(layer.clips.iter().flat_map(|clip| &clip.effects))
        });
        let groups = self.groups.iter().flat_map(|group| &group.effects);
        self.composition_effects
            .iter()
            .chain(groups)
            .chain(layers)
            .find(|effect| effect.id == id)
    }

    /// The layer showing `clip`, and the clip.
    pub fn find_clip(&self, clip: &str) -> Option<(&Layer, &Clip)> {
        self.layers.iter().find_map(|layer| {
            let found = layer.clips.iter().find(|c| c.id == clip)?;
            Some((layer, found))
        })
    }

    pub(crate) fn layer_mut(&mut self, id: &str) -> Result<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
    }

    pub(crate) fn clip_mut(&mut self, id: &str) -> Result<&mut Clip> {
        self.layers
            .iter_mut()
            .flat_map(|layer| &mut layer.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no clip {id}")))
    }

    /// The stack holding the effect `id` (of a layer, of a clip or of the composition), and the
    /// effect's place in it.
    pub(crate) fn effect_mut(&mut self, id: &str) -> Result<(&mut Vec<Effect>, usize)> {
        let layers = self.layers.iter_mut().flat_map(|layer| {
            std::iter::once(&mut layer.effects)
                .chain(layer.clips.iter_mut().map(|clip| &mut clip.effects))
        });
        std::iter::once(&mut self.composition_effects)
            .chain(self.groups.iter_mut().map(|group| &mut group.effects))
            .chain(layers)
            .find_map(|stack| {
                let at = stack.iter().position(|effect| effect.id == id)?;
                Some((stack, at))
            })
            .ok_or_else(|| ProjectError::Invalid(format!("no effect {id}")))
    }

    fn deck_index(&self, id: &str) -> Result<usize> {
        self.decks
            .iter()
            .position(|deck| deck.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no deck {id}")))
    }

    /// The deck holding `column` and the column's place in it, by index.
    fn column_index(&self, id: &str) -> Result<(usize, usize)> {
        self.decks
            .iter()
            .enumerate()
            .find_map(|(d, deck)| Some((d, deck.columns.iter().position(|c| c.id == id)?)))
            .ok_or_else(|| ProjectError::Invalid(format!("no column {id}")))
    }

    /// The cell of `layer` that `column` names: the column itself on a deck layer, the slot at the
    /// column's place on a global layer.
    fn cell_of(&self, layer: &str, column: &str) -> Result<Cell> {
        let (_, at) = self.column_index(column)?;
        let layer = self
            .layers
            .iter()
            .find(|l| l.id == layer)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {layer}")))?;
        Ok(if layer.global {
            Cell::Slot(at)
        } else {
            Cell::Column(column.to_owned())
        })
    }
}

fn new_deck(new_id: &mut impl FnMut() -> String) -> Deck {
    Deck {
        id: new_id(),
        name: None,
        columns: (0..DEFAULT_COLUMNS)
            .map(|_| Column {
                id: new_id(),
                name: None,
            })
            .collect(),
    }
}

/// A level as a fader stands, 0 to 1.
fn level(value: f32) -> Result<f32> {
    within(value, 0.0, 1.0)
}

fn within(value: f32, min: f32, max: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value.clamp(min, max))
    } else {
        Err(ProjectError::Invalid("a level is a number".into()))
    }
}

/// The length in [`TEMPO_BEATS`] nearest to `beats`.
fn nearest_beats(beats: u32) -> u32 {
    TEMPO_BEATS
        .into_iter()
        .min_by_key(|length| length.abs_diff(beats))
        .unwrap_or(1)
}

/// Applies `op` to a VJ project; `new_id` names whatever it creates. The caller validates the result.
pub fn apply(project: &mut Project, op: VjOp, mut new_id: impl FnMut() -> String) -> Result<()> {
    if project.mode != ProjectMode::Vj {
        return Err(ProjectError::Invalid("only a VJ project has a grid".into()));
    }
    let prune = op.may_remove();
    let vj = &mut project.vj;
    match op {
        VjOp::AddLayer { global } => {
            if vj.layers.len() >= MAX_LAYERS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_LAYERS} layers"
                )));
            }
            vj.layers.push(Layer::new(new_id(), global));
        }
        VjOp::RemoveLayer { layer } => {
            let index = layer_index(vj, &layer)?;
            if vj.layers.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a layer".into()));
            }
            vj.layers.remove(index);
        }
        VjOp::MoveLayer { layer, to } => {
            let index = layer_index(vj, &layer)?;
            let moved = vj.layers.remove(index);
            vj.layers.insert(to.min(vj.layers.len()), moved);
            crate::vj_groups::settle_moved(vj, &layer);
        }
        VjOp::RenameLayer { layer, name } => vj.layer_mut(&layer)?.name = typed_name(&name)?,
        VjOp::LayerMaster { layer, value } => vj.layer_mut(&layer)?.master = level(value)?,
        VjOp::LayerOpacity { layer, value } => vj.layer_mut(&layer)?.opacity = level(value)?,
        VjOp::LayerVolume { layer, value } => vj.layer_mut(&layer)?.volume = level(value)?,
        VjOp::LayerLevelReset { layer, level } => {
            let new = Layer::new(String::new(), false);
            let layer = vj.layer_mut(&layer)?;
            match level {
                Level::Master => layer.master = new.master,
                Level::Opacity => layer.opacity = new.opacity,
                Level::Volume => layer.volume = new.volume,
                Level::Transition => layer.transition = new.transition,
            }
        }
        VjOp::LayerBlend { layer, blend } => vj.layer_mut(&layer)?.blend = blend,
        VjOp::LayerTransition { layer, value } => {
            vj.layer_mut(&layer)?.transition = level(value / MAX_TRANSITION)? * MAX_TRANSITION;
        }
        VjOp::LayerTransitionEffect {
            layer,
            effect,
            random,
        } => {
            let layer = vj.layer_mut(&layer)?;
            layer.transition_effect = effect;
            layer.transition_random = random;
        }
        VjOp::LayerCrossfade { layer, side } => vj.layer_mut(&layer)?.crossfade = side,
        VjOp::LayerAutopilot { layer, mode } => vj.layer_mut(&layer)?.autopilot = mode,
        VjOp::LayerBypass { layer, on } => vj.layer_mut(&layer)?.bypass = on,
        VjOp::LayerSolo { layer, on } => vj.layer_mut(&layer)?.solo = on,
        VjOp::LayerCollapse { layer, on } => vj.layer_mut(&layer)?.collapsed = on,
        VjOp::GroupLayers { layers } => crate::vj_groups::group_layers(vj, &layers, new_id())?,
        VjOp::LayerGroup { layer, group, to } => {
            crate::vj_groups::layer_group(vj, &layer, group, to, new_id())?;
        }
        VjOp::Ungroup { group } => crate::vj_groups::ungroup(vj, &group)?,
        VjOp::RenameGroup { group, name } => vj.group_mut(&group)?.name = typed_name(&name)?,
        VjOp::GroupMaster { group, value } => vj.group_mut(&group)?.master = level(value)?,
        VjOp::GroupOpacity { group, value } => vj.group_mut(&group)?.opacity = level(value)?,
        VjOp::GroupLevelReset { group, level } => {
            let new = crate::vj_groups::Group::new(String::new());
            let group = vj.group_mut(&group)?;
            match level {
                Level::Master => group.master = new.master,
                Level::Opacity => group.opacity = new.opacity,
                _ => return Err(ProjectError::Invalid("a group has no such fader".into())),
            }
        }
        VjOp::GroupBlend { group, blend } => vj.group_mut(&group)?.blend = blend,
        VjOp::GroupBypass { group, on } => vj.group_mut(&group)?.bypass = on,
        VjOp::GroupSolo { group, on } => vj.group_mut(&group)?.solo = on,
        VjOp::GroupCollapse { group, on } => vj.group_mut(&group)?.collapsed = on,
        VjOp::SetClock { source } => vj.clock = source,
        VjOp::SetLink { on } => vj.link = on,
        VjOp::SetBpm { bpm } => set_bpm(&mut project.timelines, |_| bpm)?,
        VjOp::NudgeBpm { by } => set_bpm(&mut project.timelines, |now| now + by)?,
        VjOp::ScaleBpm { factor } => set_bpm(&mut project.timelines, |now| now * factor)?,
        VjOp::TapBpm { taps } => {
            let tapped = tapped_bpm(&taps)
                .ok_or_else(|| ProjectError::Invalid("tap at least twice".into()))?;
            set_bpm(&mut project.timelines, |_| tapped)?;
        }
        VjOp::AddDeck => {
            if vj.decks.len() >= MAX_DECKS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_DECKS} decks"
                )));
            }
            let deck = new_deck(&mut new_id);
            vj.deck = deck.id.clone();
            vj.decks.push(deck);
        }
        VjOp::RemoveDeck { deck } => {
            let index = vj.deck_index(&deck)?;
            if vj.decks.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a deck".into()));
            }
            let removed = vj.decks.remove(index);
            let gone: HashSet<&str> = removed.columns.iter().map(|c| c.id.as_str()).collect();
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| !clip.column.as_deref().is_some_and(|c| gone.contains(c)));
            }
            if vj.deck == deck {
                vj.deck = vj.decks[index.min(vj.decks.len() - 1)].id.clone();
            }
        }
        VjOp::MoveDeck { deck, to } => {
            let index = vj.deck_index(&deck)?;
            let moved = vj.decks.remove(index);
            vj.decks.insert(to.min(vj.decks.len()), moved);
        }
        VjOp::RenameDeck { deck, name } => {
            let index = vj.deck_index(&deck)?;
            vj.decks[index].name = typed_name(&name)?;
        }
        VjOp::SelectDeck { deck } => {
            vj.deck_index(&deck)?;
            vj.deck = deck;
        }
        VjOp::AddColumn => {
            let index = vj.deck_index(&vj.deck.clone())?;
            let deck = &mut vj.decks[index];
            if deck.columns.len() >= MAX_COLUMNS {
                return Err(ProjectError::Invalid(format!(
                    "a deck has at most {MAX_COLUMNS} columns"
                )));
            }
            deck.columns.push(Column {
                id: new_id(),
                name: None,
            });
        }
        VjOp::RemoveColumn { column } => {
            let (deck, at) = vj.column_index(&column)?;
            if vj.decks[deck].columns.len() == 1 {
                return Err(ProjectError::Invalid("a deck needs a column".into()));
            }
            vj.decks[deck].columns.remove(at);
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| clip.column.as_deref() != Some(&column));
            }
        }
        VjOp::MoveColumn { column, to } => {
            let (deck, at) = vj.column_index(&column)?;
            let columns = &mut vj.decks[deck].columns;
            let moved = columns.remove(at);
            columns.insert(to.min(columns.len()), moved);
        }
        VjOp::RenameColumn { column, name } => {
            let (deck, at) = vj.column_index(&column)?;
            vj.decks[deck].columns[at].name = typed_name(&name)?;
        }
        VjOp::PlaceVideo {
            asset,
            layer,
            column,
        } => {
            if !project.videos.iter().any(|video| video.id == asset) {
                return Err(ProjectError::MissingAsset(asset));
            }
            place_clip(vj, &layer, &column, new_clip(new_id(), asset, None))?;
        }
        VjOp::PlaceGenerator {
            generator,
            layer,
            column,
        } => {
            let clip = new_clip(new_id(), String::new(), Some(generator.tidy()));
            place_clip(vj, &layer, &column, clip)?;
        }
        VjOp::MoveClip {
            clip,
            layer,
            column,
        } => {
            let to = vj.cell_of(&layer, &column)?;
            let (from_layer, from) = vj
                .layers
                .iter()
                .find_map(|l| {
                    let c = l.clips.iter().find(|c| c.id == clip)?;
                    Some((l.id.clone(), c.cell()?))
                })
                .ok_or_else(|| ProjectError::Invalid(format!("no clip {clip}")))?;
            let moved = take_clip(vj, &from_layer, |c| c.id == clip);
            let occupant = take_clip(vj, &layer, |c| c.cell().as_ref() == Some(&to));
            if let Some(mut occupant) = occupant {
                occupant.put(from);
                vj.layer_mut(&from_layer)?.clips.push(occupant);
            }
            if let Some(mut moved) = moved {
                moved.put(to);
                vj.layer_mut(&layer)?.clips.push(moved);
            }
        }
        VjOp::RemoveClip { clip } => {
            vj.clip_mut(&clip)?;
            for layer in &mut vj.layers {
                layer.clips.retain(|c| c.id != clip);
            }
        }
        VjOp::RenameClip { clip, name } => vj.clip_mut(&clip)?.name = typed_name(&name)?,
        VjOp::ClipPlayback { clip, playback } => vj.clip_mut(&clip)?.playback = playback,
        VjOp::ClipSpeed { clip, value } => {
            vj.clip_mut(&clip)?.speed = within(value, MIN_SPEED, MAX_SPEED)?;
        }
        VjOp::ClipDuration { clip, value } => {
            vj.clip_mut(&clip)?.duration = within(value, MIN_DURATION, MAX_DURATION)?;
        }
        VjOp::ClipDirection { clip, direction } => vj.clip_mut(&clip)?.direction = direction,
        VjOp::ClipTempo { clip, tempo } => {
            vj.clip_mut(&clip)?.tempo = match tempo {
                Tempo::Free => Tempo::Free,
                Tempo::Beats { beats } => Tempo::Beats {
                    beats: nearest_beats(beats),
                },
            };
        }
        VjOp::ClipRegion { clip, start, end } => {
            vj.clip_mut(&clip)?.region = Region::tidy(start, end)?;
        }
        VjOp::ClipCue {
            clip,
            index,
            position,
        } => {
            let clip = vj.clip_mut(&clip)?;
            let slot = clip
                .cues
                .get_mut(index)
                .ok_or_else(|| ProjectError::Invalid(format!("a clip has {CUES} cues")))?;
            *slot = position.map(|p| within(p, 0.0, 1.0)).transpose()?;
        }
        VjOp::ClipGenerator { clip, generator } => {
            let clip = vj.clip_mut(&clip)?;
            if clip.generator.is_none() {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is a video, not a generator",
                    clip.id
                )));
            }
            let generator = generator.tidy();
            // A modulation of a number the new kind of generator does not have goes with it.
            clip.modulation.retain(|field, _| {
                field
                    .strip_prefix(crate::vj_modulation::GENERATOR_FIELD)
                    .is_none_or(|name| generator.numbers().contains(&name))
            });
            clip.generator = Some(generator);
        }
        VjOp::TextLine { clip, step } => {
            let clip = vj.clip_mut(&clip)?;
            let Some(Generator::Text { text, line, .. }) = &mut clip.generator else {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is not a text block",
                    clip.id
                )));
            };
            *line = crate::generator::step_line(text, *line, step);
        }
        VjOp::ClipTrigger { clip, trigger } => vj.clip_mut(&clip)?.trigger = trigger,
        VjOp::ClipFit { clip, fit } => vj.clip_mut(&clip)?.fit = fit,
        VjOp::ClipScale { clip, value } => {
            vj.clip_mut(&clip)?.scale = within(value, 0.0, MAX_SCALE)?;
        }
        VjOp::ClipPosition { clip, x, y } => {
            let offset = |v| within(v, -MAX_OFFSET, MAX_OFFSET);
            vj.clip_mut(&clip)?.position = [offset(x)?, offset(y)?];
        }
        VjOp::AddEffect { target, kind } => {
            let kind = EffectKind::default_of(&kind)?;
            if kind.composition_only() && target != EffectTarget::Composition {
                return Err(ProjectError::Invalid(format!(
                    "a {} effect only works on the composition",
                    kind.name()
                )));
            }
            let stack = match target {
                EffectTarget::Clip(clip) => &mut vj.clip_mut(&clip)?.effects,
                EffectTarget::Layer(layer) => &mut vj.layer_mut(&layer)?.effects,
                EffectTarget::Group(group) => &mut vj.group_mut(&group)?.effects,
                EffectTarget::Composition => &mut vj.composition_effects,
            };
            if stack.len() >= MAX_EFFECTS {
                return Err(ProjectError::Invalid(format!(
                    "a stack has at most {MAX_EFFECTS} effects"
                )));
            }
            stack.push(Effect {
                id: new_id(),
                bypass: false,
                modulation: BTreeMap::new(),
                kind,
            });
        }
        VjOp::ApplyEffectPreset { effect, preset } => {
            crate::vj_presets::apply_to_effect(vj, &effect, &preset)?;
        }
        VjOp::AddEffectPreset { target, preset } => {
            crate::vj_presets::add_to_stack(vj, &target, &preset, new_id)?;
        }
        VjOp::CompositionBypass { bypass } => vj.composition_bypass = bypass,
        VjOp::RemoveEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack.remove(at);
        }
        VjOp::MoveEffect { effect, index } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            let moved = stack.remove(at);
            stack.insert(index.min(stack.len()), moved);
        }
        VjOp::EffectParams { effect, kind } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            if !stack[at].kind.same_kind(&kind) {
                return Err(ProjectError::Invalid(format!(
                    "effect {effect} is a {}, not a {}",
                    stack[at].kind.name(),
                    kind.name()
                )));
            }
            stack[at].kind = kind.tidy()?;
        }
        VjOp::EffectBypass { effect, bypass } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack[at].bypass = bypass;
        }
        VjOp::DuplicateEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            if stack.len() >= MAX_EFFECTS {
                return Err(ProjectError::Invalid(format!(
                    "a stack has at most {MAX_EFFECTS} effects"
                )));
            }
            let copy = Effect {
                id: new_id(),
                ..stack[at].clone()
            };
            stack.insert(at + 1, copy);
        }
        VjOp::ResetEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack[at].kind = EffectKind::default_of(&stack[at].kind.name())?;
            stack[at].modulation.clear();
        }
        VjOp::SetModulation { target, modulation } => set_modulation(vj, target, modulation)?,
        VjOp::AddMapping { source, target } => {
            crate::vj_mapping::add_mapping(vj, source, target, new_id())?;
        }
        VjOp::RemoveMapping { mapping } => crate::vj_mapping::remove_mapping(vj, &mapping)?,
        VjOp::ClearMapping { target } => vj.mappings.retain(|m| m.target != target),
        VjOp::SetMapping {
            mapping,
            min,
            max,
            invert,
            mode,
        } => crate::vj_mapping::set_mapping(vj, &mapping, min, max, invert, mode)?,
        VjOp::SetMappingFollow { mapping, follow } => {
            crate::vj_mapping::set_mapping_follow(vj, &mapping, follow)?;
        }
        VjOp::SetMappingLights { mapping, lights } => {
            crate::vj_mapping::set_mapping_lights(vj, &mapping, lights)?;
        }
        VjOp::SetLights { lights } => {
            if let Some(lights) = lights {
                lights.validate()?;
            }
            vj.lights = lights;
        }
        VjOp::SetOscFeedback { destinations } => {
            vj.osc_feedback = crate::vj_osc_feedback::parse_destinations(&destinations)?;
        }
        VjOp::SetMappingFine { mapping, fine } => {
            crate::vj_mapping::set_mapping_fine(vj, &mapping, fine)?;
        }
        VjOp::DashboardValue { knob, value } => {
            crate::vj_mapping::turn_knob(project, knob, value)?;
        }
        VjOp::DashboardLink {
            knob,
            target,
            min,
            max,
        } => crate::vj_mapping::link_knob(vj, knob, target, min, max)?,
        VjOp::DashboardUnlink { knob, target } => {
            crate::vj_mapping::unlink_knob(vj, knob, &target)?;
        }
        VjOp::RenameKnob { knob, name } => {
            let name = typed_name(&name)?;
            crate::vj_mapping::rename_knob(vj, knob, name)?;
        }
    }
    project.vj.tidy_groups();
    // Position mappings follow their slot, after whatever moved things.
    crate::vj_mapping::rebind(&mut project.vj);
    if prune {
        crate::vj_mapping::prune(&mut project.vj);
    }
    Ok(())
}

pub(crate) fn new_clip(id: String, asset: String, generator: Option<Generator>) -> Clip {
    Clip {
        id,
        column: None,
        slot: None,
        asset,
        generator,
        name: None,
        playback: Playback::Loop,
        trigger: Trigger::Normal,
        speed: 1.0,
        duration: DEFAULT_DURATION,
        direction: Direction::Forward,
        tempo: Tempo::Free,
        region: Region::default(),
        cues: [None; CUES],
        fit: VideoFit::Fit,
        scale: 1.0,
        position: [0.0, 0.0],
        effects: Vec::new(),
        modulation: BTreeMap::new(),
    }
}

/// Puts `clip` in the cell of `layer` that `column` names, in place of the clip that was there.
fn place_clip(vj: &mut Vj, layer: &str, column: &str, mut clip: Clip) -> Result<()> {
    let cell = vj.cell_of(layer, column)?;
    let layer = vj.layer_mut(layer)?;
    layer
        .clips
        .retain(|clip| clip.cell().as_ref() != Some(&cell));
    clip.put(cell);
    layer.clips.push(clip);
    Ok(())
}

fn layer_index(vj: &Vj, id: &str) -> Result<usize> {
    vj.layers
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
}

fn take_clip(vj: &mut Vj, layer: &str, which: impl Fn(&Clip) -> bool) -> Option<Clip> {
    let layer = vj.layers.iter_mut().find(|l| l.id == layer)?;
    let index = layer.clips.iter().position(which)?;
    Some(layer.clips.remove(index))
}

/// Sets or takes away the modulation `target` names, brought into the range of the setting.
fn set_modulation(vj: &mut Vj, target: ModTarget, modulation: Option<Modulation>) -> Result<()> {
    if let ModTarget::Knob { knob } = target {
        return crate::vj_mapping::modulate_knob(vj, knob, modulation);
    }
    let (map, key, (lo, hi)) = match target {
        ModTarget::Effect { effect, param } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            let range = stack[at].kind.modulatable_range(&param)?;
            (&mut stack[at].modulation, param, range)
        }
        ModTarget::Layer { layer, field } => {
            let range = crate::vj_modulation::layer_range(&field)
                .ok_or_else(|| ProjectError::Invalid(format!("{field} cannot be modulated")))?;
            (&mut vj.layer_mut(&layer)?.modulation, field, range)
        }
        ModTarget::Clip { clip, field } => {
            let clip = vj.clip_mut(&clip)?;
            let range = crate::vj_modulation::clip_field_range(clip, &field)
                .ok_or_else(|| ProjectError::Invalid(format!("{field} cannot be modulated")))?;
            (&mut clip.modulation, field, range)
        }
        ModTarget::Knob { .. } => unreachable!("a knob is handled above"),
    };
    match modulation {
        Some(modulation) => {
            map.insert(key, modulation.tidy(lo, hi)?);
        }
        None => {
            map.remove(&key);
        }
    }
    Ok(())
}

/// A typed name, or `None` for an empty one (the default name).
fn typed_name(name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.chars().count() > MAX_CLIP_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name has at most {MAX_CLIP_NAME} characters"
        )));
    }
    Ok((!name.is_empty()).then(|| name.to_owned()))
}

/// Checks the grid: unique ids (a clip's is unique among every item of the project, in `item_ids`),
/// a shown deck, each clip in a known column (a slot on a global layer), at most one clip per
/// cell, clips showing known videos, levels in range, a grid in a VJ project and none in a timeline
/// project.
pub(crate) fn validate<'a>(
    project: &'a Project,
    video_ids: &HashSet<&str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    let vj = &project.vj;
    match project.mode {
        ProjectMode::Vj
            if vj.layers.is_empty()
                || vj.decks.is_empty()
                || vj.decks.iter().any(|deck| deck.columns.is_empty()) =>
        {
            return Err(ProjectError::Invalid(
                "a VJ project needs a layer and a deck with a column".into(),
            ));
        }
        ProjectMode::Vj if vj.shown_deck().is_none() => {
            return Err(ProjectError::Invalid(format!("no deck {}", vj.deck)));
        }
        // A VJ show is an app of its own: none of a cue show's content, so its clock plays nothing
        // but the grid. Its one timeline only carries the clock's tempo and timecode.
        ProjectMode::Vj
            if project.timelines.len() != 1
                || project
                    .timelines
                    .iter()
                    .any(|t| !t.tracks.is_empty() || !t.controls.is_empty())
                || !project.setlists.is_empty()
                || !project.assets.is_empty()
                || project.voice.is_some()
                || !project.output.ltc.is_empty()
                || !project.output.mtc.is_empty()
                || !project.output.osc.is_empty()
                || !project.output.audio.is_empty()
                || project.output.metronome.is_some() =>
        {
            return Err(ProjectError::Invalid(
                "a VJ show has no timeline tracks, audio, setlists or timecode outputs".into(),
            ));
        }
        ProjectMode::Timeline if !vj.is_empty() => {
            return Err(ProjectError::Invalid(
                "a timeline project has no VJ grid".into(),
            ));
        }
        _ => {}
    }
    if vj.layers.len() > MAX_LAYERS
        || vj.decks.len() > MAX_DECKS
        || vj.decks.iter().any(|deck| deck.columns.len() > MAX_COLUMNS)
    {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_LAYERS} layers and {MAX_DECKS} decks of {MAX_COLUMNS} columns"
        )));
    }
    let mut ids = HashSet::new();
    let mut columns = HashSet::new();
    for deck in &vj.decks {
        unique(&mut ids, &deck.id)?;
        check_name(deck.name.as_deref())?;
        for column in &deck.columns {
            unique(&mut ids, &column.id)?;
            columns.insert(column.id.as_str());
            check_name(column.name.as_deref())?;
        }
    }
    crate::vj_effects::validate_stack(&vj.composition_effects, true, item_ids)?;
    crate::vj_groups::validate(vj, &mut ids, item_ids)?;
    for layer in &vj.layers {
        unique(&mut ids, &layer.id)?;
        check_name(layer.name.as_deref())?;
        let levels = [layer.master, layer.opacity, layer.volume];
        if levels.iter().any(|v| !(0.0..=1.0).contains(v))
            || !(0.0..=MAX_TRANSITION).contains(&layer.transition)
        {
            return Err(ProjectError::Invalid(format!(
                "layer {} needs levels of 0 to 1 and a transition of 0 to {MAX_TRANSITION} s",
                layer.id
            )));
        }
        crate::vj_effects::validate_stack(&layer.effects, false, item_ids)?;
        crate::vj_modulation::validate_map(&layer.modulation, crate::vj_modulation::layer_range)?;
        let mut cells = HashSet::new();
        for clip in &layer.clips {
            if !item_ids.insert(&clip.id) {
                return Err(ProjectError::Invalid(format!(
                    "duplicate clip id: {}",
                    clip.id
                )));
            }
            let placed = match (clip.cell(), layer.global) {
                (Some(Cell::Column(column)), false) => columns.contains(column.as_str()),
                (Some(Cell::Slot(slot)), true) => slot < MAX_COLUMNS,
                _ => false,
            };
            if !placed {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is in no cell of its layer",
                    clip.id
                )));
            }
            if !cells.insert(clip.cell()) {
                return Err(ProjectError::Invalid(format!(
                    "layer {} has two clips in one cell",
                    layer.id
                )));
            }
            match &clip.generator {
                Some(generator) if clip.asset.is_empty() => generator.validate()?,
                Some(_) => {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} is both a video and a generator",
                        clip.id
                    )));
                }
                None if !video_ids.contains(clip.asset.as_str()) => {
                    return Err(ProjectError::MissingAsset(clip.asset.clone()));
                }
                None => {}
            }
            check_name(clip.name.as_deref())?;
            check_transport(clip)?;
            crate::vj_effects::validate_stack(&clip.effects, false, item_ids)?;
            crate::vj_modulation::validate_map(&clip.modulation, |field| {
                crate::vj_modulation::clip_field_range(clip, field)
            })?;
            if !(0.0..=MAX_SCALE).contains(&clip.scale)
                || clip
                    .position
                    .iter()
                    .any(|v| !(-MAX_OFFSET..=MAX_OFFSET).contains(v))
            {
                return Err(ProjectError::Invalid(format!(
                    "clip {} needs a scale of 0 to {MAX_SCALE} and a position of -{MAX_OFFSET} to {MAX_OFFSET}",
                    clip.id
                )));
            }
        }
    }
    crate::vj_mapping::validate(vj)?;
    Ok(())
}

/// The speed, tempo, region and cues of a clip are within their ranges.
fn check_transport(clip: &Clip) -> Result<()> {
    let tempo = match clip.tempo {
        Tempo::Free => true,
        Tempo::Beats { beats } => TEMPO_BEATS.contains(&beats),
    };
    let cues = clip
        .cues
        .iter()
        .flatten()
        .all(|cue| (0.0..=1.0).contains(cue));
    if !(MIN_SPEED..=MAX_SPEED).contains(&clip.speed)
        || !(MIN_DURATION..=MAX_DURATION).contains(&clip.duration)
        || !tempo
        || !clip.region.valid()
        || !cues
    {
        return Err(ProjectError::Invalid(format!(
            "clip {} needs a speed of {MIN_SPEED} to {MAX_SPEED}, a duration of {MIN_DURATION} to {MAX_DURATION} seconds, a length of 1 to 128 beats, a region of at least {MIN_REGION} inside the clip and cues inside the clip",
            clip.id
        )));
    }
    Ok(())
}

fn unique<'a>(seen: &mut HashSet<&'a str>, id: &'a str) -> Result<()> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate grid id: {id}")))
    }
}

pub(crate) fn check_name(name: Option<&str>) -> Result<()> {
    match name {
        Some(name) if name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME => {
            Err(ProjectError::Invalid(format!(
                "a grid name has 1 to {MAX_CLIP_NAME} characters; leave it out instead"
            )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::video_project;

    fn ids() -> impl FnMut() -> String {
        let mut next = 0;
        move || {
            next += 1;
            format!("g{next}")
        }
    }

    /// A VJ project with the starting grid (layers g1 to g3, deck g4 with columns g5 to g12).
    fn vj_project() -> (Project, impl FnMut() -> String) {
        let mut project = Project::new("Show", crate::FrameRate::Fps25);
        project.videos = video_project().videos;
        let mut new_id = ids();
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(&mut new_id);
        (project, new_id)
    }

    fn run(project: &mut Project, op: VjOp, new_id: impl FnMut() -> String) {
        apply(project, op, new_id).unwrap();
        project.validate().unwrap();
    }

    fn place(project: &mut Project, layer: &str, column: &str, new_id: impl FnMut() -> String) {
        let op = VjOp::PlaceVideo {
            asset: "v1".into(),
            layer: layer.into(),
            column: column.into(),
        };
        run(project, op, new_id);
    }

    #[test]
    fn starting_grid_should_be_a_valid_vj_project() {
        let (project, _) = vj_project();
        assert_eq!(project.vj.layers.len(), DEFAULT_LAYERS);
        assert_eq!(project.vj.decks.len(), 1);
        assert_eq!(
            project.vj.shown_deck().unwrap().columns.len(),
            DEFAULT_COLUMNS
        );
        project.validate().unwrap();
    }

    #[test]
    fn a_timeline_project_should_have_no_grid() {
        let mut project = video_project();
        assert!(apply(&mut project, VjOp::AddLayer { global: false }, ids()).is_err());
        project.vj = Vj::starting(ids());
        assert!(project.validate().is_err());
    }

    #[test]
    fn a_vj_project_should_need_its_grid_and_a_shown_deck() {
        let (mut project, _) = vj_project();
        project.vj.deck = "nope".into();
        assert!(project.validate().is_err());
        project.vj = Vj::default();
        assert!(project.validate().is_err());
    }

    #[test]
    fn place_video_should_replace_the_clip_of_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        place(&mut project, "g1", "g5", &mut new_id);
        let clips = &project.vj.layers[0].clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "g14");
    }

    #[test]
    fn place_video_should_refuse_an_unknown_video() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::PlaceVideo {
            asset: "nope".into(),
            layer: "g1".into(),
            column: "g5".into(),
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn a_generator_should_be_placed_edited_and_saved_without_a_video() {
        let (mut project, mut new_id) = vj_project();
        let strobe = Generator::Strobe {
            colour: "#ff0000".into(),
            rate: 4.0,
            width: 0.25,
        };
        let op = VjOp::PlaceGenerator {
            generator: Generator::Solid {
                colour: "#ffffff".into(),
            },
            layer: "g1".into(),
            column: "g5".into(),
        };
        run(&mut project, op, &mut new_id);
        let clip = project.vj.layers[0].clips[0].id.clone();
        let op = VjOp::ClipGenerator {
            clip: clip.clone(),
            generator: strobe.clone(),
        };
        run(&mut project, op, &mut new_id);
        let json = serde_json::to_value(&project.vj.layers[0].clips[0]).unwrap();
        assert!(json.get("asset").is_none(), "{json}");
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(back.generator, Some(strobe));
        project.vj.layers[0].clips[0].generator = Some(Generator::Noise {
            colour: "#ffffff".into(),
            rate: 0.0,
        });
        assert!(project.validate().is_err());
        project.vj.layers[0].clips[0].generator = None;
        assert!(project.validate().is_err(), "a clip without a video");
    }

    #[test]
    fn clip_generator_should_refuse_a_video_clip() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        let op = VjOp::ClipGenerator {
            clip: project.vj.layers[0].clips[0].id.clone(),
            generator: Generator::Bars,
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn text_line_should_step_a_text_block_and_refuse_anything_else() {
        let (mut project, mut new_id) = vj_project();
        let text: Generator = serde_json::from_value(
            serde_json::json!({ "kind": "text", "text": "a\n\nb", "lines": "one" }),
        )
        .unwrap();
        let op = VjOp::PlaceGenerator {
            generator: text,
            layer: "g1".into(),
            column: "g5".into(),
        };
        run(&mut project, op, &mut new_id);
        let clip = project.vj.layers[0].clips[0].id.clone();
        let step = |project: &mut Project, new_id: &mut dyn FnMut() -> String, step: i32| {
            let op = VjOp::TextLine {
                clip: clip.clone(),
                step,
            };
            apply(project, op, new_id).unwrap();
            match &project.vj.layers[0].clips[0].generator {
                Some(Generator::Text { line, .. }) => *line,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(step(&mut project, &mut new_id, 1), 2);
        assert_eq!(step(&mut project, &mut new_id, 1), 0, "wraps");
        assert_eq!(step(&mut project, &mut new_id, -1), 2);
        project.vj.layers[0].clips[0].generator = Some(Generator::Bars);
        let op = VjOp::TextLine { clip, step: 1 };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn a_global_layer_should_keep_its_clips_in_slots_across_decks() {
        let (mut project, mut new_id) = vj_project();
        run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
        place(&mut project, "g13", "g7", &mut new_id); // slot 2
        let clip = &project.vj.layers[3].clips[0];
        assert_eq!((clip.column.as_deref(), clip.slot), (None, Some(2)));

        run(&mut project, VjOp::AddDeck, &mut new_id); // g15, columns g16 to g23
        assert_eq!(project.vj.deck, "g15");
        place(&mut project, "g13", "g18", &mut new_id); // slot 2 again, from the other deck
        assert_eq!(project.vj.layers[3].clips.len(), 1);
        assert_eq!(project.vj.column_clips("g18").count(), DEFAULT_LAYERS);
    }

    #[test]
    fn a_clip_should_scale_and_move_within_its_limits_and_keep_them_in_the_file() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let clip = |project: &Project| project.vj.layers[0].clips[0].clone();
        assert_eq!(
            (clip(&project).scale, clip(&project).position),
            (1.0, [0.0, 0.0])
        );
        let scale = |value| VjOp::ClipScale {
            clip: "g13".into(),
            value,
        };
        run(&mut project, scale(50.0), &mut new_id);
        assert_eq!(clip(&project).scale, MAX_SCALE);
        run(&mut project, scale(0.5), &mut new_id);
        let op = VjOp::ClipPosition {
            clip: "g13".into(),
            x: 0.25,
            y: -9.0,
        };
        run(&mut project, op, &mut new_id);
        assert_eq!(clip(&project).position, [0.25, -MAX_OFFSET]);
        assert!(apply(&mut project, scale(f32::NAN), &mut new_id).is_err());
        project.vj.layers[0].clips[0].position[1] = f32::NAN;
        assert!(project.validate().is_err(), "a file can carry anything");
        project.vj.layers[0].clips[0].position[1] = -MAX_OFFSET;

        let json = serde_json::to_value(clip(&project)).unwrap();
        assert_eq!(
            (json["scale"].as_f64(), json["position"][0].as_f64()),
            (Some(0.5), Some(0.25))
        );
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(back, clip(&project));
    }

    fn transport(project: &Project) -> &Clip {
        project.vj.find_clip("g13").unwrap().1
    }

    #[test]
    fn a_clip_should_start_with_the_transport_of_a_plain_clip_and_store_none_of_it() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let clip = transport(&project);
        assert_eq!(clip.speed, 1.0);
        assert_eq!(clip.direction, Direction::Forward);
        assert_eq!(clip.tempo, Tempo::Free);
        assert_eq!(clip.region, Region::default());
        assert_eq!(clip.cues, [None; CUES]);
        let json = serde_json::to_value(clip).unwrap();
        assert_eq!(clip.duration, DEFAULT_DURATION);
        for field in ["speed", "duration", "direction", "tempo", "region", "cues"] {
            assert!(json.get(field).is_none(), "{field} is left out");
        }
        // A clip written before the transport has the same.
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(&back, clip);
    }

    #[test]
    fn the_transport_ops_should_set_bring_into_range_and_round_trip() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let clip = || "g13".to_owned();
        run(
            &mut project,
            VjOp::ClipSpeed {
                clip: clip(),
                value: 50.0,
            },
            &mut new_id,
        );
        assert_eq!(transport(&project).speed, MAX_SPEED);
        run(
            &mut project,
            VjOp::ClipSpeed {
                clip: clip(),
                value: 0.0,
            },
            &mut new_id,
        );
        assert_eq!(transport(&project).speed, MIN_SPEED);
        assert!(
            apply(
                &mut project,
                VjOp::ClipSpeed {
                    clip: clip(),
                    value: f32::NAN
                },
                &mut new_id
            )
            .is_err()
        );
        run(
            &mut project,
            VjOp::ClipDirection {
                clip: clip(),
                direction: Direction::Reverse,
            },
            &mut new_id,
        );
        run(
            &mut project,
            VjOp::ClipTempo {
                clip: clip(),
                tempo: Tempo::Beats { beats: 5 },
            },
            &mut new_id,
        );
        assert_eq!(transport(&project).tempo, Tempo::Beats { beats: 4 });
        run(
            &mut project,
            VjOp::ClipRegion {
                clip: clip(),
                start: 0.3,
                end: 0.2,
            },
            &mut new_id,
        );
        let region = transport(&project).region;
        assert_eq!(region.start, 0.3);
        assert!(region.span() >= MIN_REGION * 0.999, "a minimum span");
        run(
            &mut project,
            VjOp::ClipRegion {
                clip: clip(),
                start: -2.0,
                end: 9.0,
            },
            &mut new_id,
        );
        assert_eq!(transport(&project).region, Region::default());
        for (index, position) in [(0, Some(0.25)), (7, Some(7.0)), (3, None)] {
            let op = VjOp::ClipCue {
                clip: clip(),
                index,
                position,
            };
            run(&mut project, op, &mut new_id);
        }
        let cues = transport(&project).cues;
        assert_eq!((cues[0], cues[7], cues[3]), (Some(0.25), Some(1.0), None));
        let outside = VjOp::ClipCue {
            clip: clip(),
            index: CUES,
            position: Some(0.5),
        };
        assert!(apply(&mut project, outside, &mut new_id).is_err());
        // What was set is stored, and read back the same.
        let json = serde_json::to_value(transport(&project)).unwrap();
        assert_eq!(json["direction"], "reverse");
        assert_eq!(json["tempo"]["kind"], "beats");
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(&back, transport(&project));
    }

    #[test]
    fn the_duration_op_should_bring_into_range_round_trip_and_refuse_what_is_not_a_number() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let duration = |value| VjOp::ClipDuration {
            clip: "g13".into(),
            value,
        };
        run(&mut project, duration(3.0), &mut new_id);
        assert_eq!(transport(&project).duration, 3.0);
        run(&mut project, duration(0.0), &mut new_id);
        assert_eq!(transport(&project).duration, MIN_DURATION);
        run(&mut project, duration(1e9), &mut new_id);
        assert_eq!(transport(&project).duration, MAX_DURATION);
        assert!(apply(&mut project, duration(f32::NAN), &mut new_id).is_err());
        run(&mut project, duration(2.5), &mut new_id);
        let json = serde_json::to_value(transport(&project)).unwrap();
        assert_eq!(json["duration"].as_f64(), Some(2.5));
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(&back, transport(&project));
        let mut without = serde_json::to_value(transport(&project)).unwrap();
        without.as_object_mut().unwrap().remove("duration");
        let old: Clip = serde_json::from_value(without).unwrap();
        assert_eq!(
            old.duration, DEFAULT_DURATION,
            "an old file has the default"
        );
    }

    #[test]
    fn validate_should_refuse_a_transport_out_of_range() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let broken = |change: &dyn Fn(&mut Clip)| {
            let mut project = project.clone();
            change(project.vj.clip_mut("g13").unwrap());
            project.validate().is_err()
        };
        assert!(broken(&|c| c.speed = 9.0));
        assert!(broken(&|c| c.speed = 0.0));
        assert!(broken(&|c| c.duration = 0.0));
        assert!(broken(&|c| c.duration = MAX_DURATION + 1.0));
        assert!(broken(&|c| c.duration = f32::NAN));
        assert!(broken(&|c| c.tempo = Tempo::Beats { beats: 3 }));
        assert!(broken(&|c| c.region = Region {
            start: 0.5,
            end: 0.5
        }));
        assert!(broken(&|c| c.region = Region {
            start: 0.1,
            end: 1.5
        }));
        assert!(broken(&|c| c.cues[2] = Some(1.5)));
        assert!(!broken(&|c| c.tempo = Tempo::Beats { beats: 128 }));
    }

    #[test]
    fn move_clip_should_swap_with_the_clip_already_in_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        place(&mut project, "g2", "g6", &mut new_id); // g14
        let op = VjOp::MoveClip {
            clip: "g13".into(),
            layer: "g2".into(),
            column: "g6".into(),
        };
        run(&mut project, op, &mut new_id);
        let (layer, clip) = project.vj.find_clip("g13").unwrap();
        assert_eq!(
            (layer.id.as_str(), clip.column.as_deref()),
            ("g2", Some("g6"))
        );
        let (layer, clip) = project.vj.find_clip("g14").unwrap();
        assert_eq!(
            (layer.id.as_str(), clip.column.as_deref()),
            ("g1", Some("g5"))
        );
    }

    #[test]
    fn move_clip_should_turn_a_column_into_a_slot_on_a_global_layer() {
        let (mut project, mut new_id) = vj_project();
        run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
        place(&mut project, "g1", "g8", &mut new_id); // g14
        let op = VjOp::MoveClip {
            clip: "g14".into(),
            layer: "g13".into(),
            column: "g9".into(),
        };
        run(&mut project, op, &mut new_id);
        let (layer, clip) = project.vj.find_clip("g14").unwrap();
        assert_eq!((layer.id.as_str(), clip.slot), ("g13", Some(4)));
    }

    #[test]
    fn remove_deck_should_take_its_clips_and_show_another_deck() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        run(&mut project, VjOp::AddDeck, &mut new_id); // g14
        run(
            &mut project,
            VjOp::SelectDeck { deck: "g4".into() },
            &mut new_id,
        );
        run(
            &mut project,
            VjOp::RemoveDeck { deck: "g4".into() },
            &mut new_id,
        );
        assert_eq!(project.vj.deck, "g14");
        assert!(project.vj.layers[0].clips.is_empty());
        let op = VjOp::RemoveDeck { deck: "g14".into() };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn remove_column_should_take_its_clips_and_keep_the_last_column() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        run(
            &mut project,
            VjOp::RemoveColumn {
                column: "g5".into(),
            },
            &mut new_id,
        );
        assert!(project.vj.layers[0].clips.is_empty());
        project.vj.decks[0].columns.truncate(1);
        let last = project.vj.decks[0].columns[0].id.clone();
        let op = VjOp::RemoveColumn { column: last };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn move_layer_should_clamp_the_place() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::MoveLayer {
            layer: "g1".into(),
            to: 99,
        };
        run(&mut project, op, &mut new_id);
        let order: Vec<_> = project.vj.layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(order, ["g2", "g3", "g1"]);
    }

    #[test]
    fn a_collapsed_layer_should_be_kept_in_the_file_and_unfold_again() {
        let (mut project, mut new_id) = vj_project();
        let collapse = |on| VjOp::LayerCollapse {
            layer: "g1".into(),
            on,
        };
        run(&mut project, collapse(true), &mut new_id);
        let json = serde_json::to_string(&project.vj.layers[0]).unwrap();
        assert!(json.contains(r#""collapsed":true"#), "{json}");
        run(&mut project, collapse(false), &mut new_id);
        let json = serde_json::to_string(&project.vj.layers[0]).unwrap();
        assert!(!json.contains("collapsed"), "absent when unfolded: {json}");
    }

    #[test]
    fn the_tempo_should_be_set_nudged_scaled_and_kept_in_range() {
        let (mut project, mut new_id) = vj_project();
        let bpm = |p: &Project| p.timelines[0].tempo.bpm;
        run(&mut project, VjOp::SetBpm { bpm: 128.456 }, &mut new_id);
        assert_eq!(bpm(&project), 128.46);
        run(&mut project, VjOp::NudgeBpm { by: -1.0 }, &mut new_id);
        assert_eq!(bpm(&project), 127.46);
        run(&mut project, VjOp::ScaleBpm { factor: 0.5 }, &mut new_id);
        assert_eq!(bpm(&project), 63.73);
        run(&mut project, VjOp::ScaleBpm { factor: 100.0 }, &mut new_id);
        assert_eq!(bpm(&project), MAX_BPM);
        let nan = VjOp::SetBpm { bpm: f64::NAN };
        assert!(apply(&mut project, nan, &mut new_id).is_err());
    }

    #[test]
    fn tapping_should_give_the_tempo_of_the_last_run_of_taps() {
        assert_eq!(tapped_bpm(&[]), None);
        assert_eq!(tapped_bpm(&[1000.0]), None);
        // Half a second apart is 120 BPM; the tap long before starts no run with them.
        assert_eq!(
            tapped_bpm(&[0.0, 5000.0, 5500.0, 6000.0, 6500.0]),
            Some(120.0)
        );
        let (mut project, mut new_id) = vj_project();
        let taps = VjOp::TapBpm {
            taps: vec![0.0, 400.0, 800.0],
        };
        run(&mut project, taps, &mut new_id);
        assert_eq!(project.timelines[0].tempo.bpm, 150.0);
    }

    #[test]
    fn the_clock_source_should_be_kept_in_the_file() {
        let (mut project, mut new_id) = vj_project();
        let json = serde_json::to_string(&project.vj).unwrap();
        assert!(!json.contains("clock"), "internal is absent: {json}");
        let ltc = VjOp::SetClock {
            source: ClockSource::Ltc,
        };
        run(&mut project, ltc, &mut new_id);
        let json = serde_json::to_string(&project.vj).unwrap();
        assert!(json.contains(r#""clock":"ltc""#), "{json}");
    }

    #[test]
    fn link_should_be_kept_in_the_file_only_while_on() {
        let (mut project, mut new_id) = vj_project();
        assert!(!serde_json::to_string(&project.vj).unwrap().contains("link"));
        run(&mut project, VjOp::SetLink { on: true }, &mut new_id);
        let json = serde_json::to_string(&project.vj).unwrap();
        assert!(json.contains(r#""link":true"#), "{json}");
    }

    #[test]
    fn rename_should_take_an_empty_name_back_to_the_default() {
        let (mut project, mut new_id) = vj_project();
        for (name, expected) in [(" Intro ", Some("Intro")), ("  ", None)] {
            let op = VjOp::RenameLayer {
                layer: "g1".into(),
                name: name.into(),
            };
            run(&mut project, op, &mut new_id);
            assert_eq!(project.vj.layers[0].name.as_deref(), expected);
        }
    }

    #[test]
    fn levels_should_be_brought_into_range() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::LayerMaster {
            layer: "g1".into(),
            value: 1.5,
        };
        run(&mut project, op, &mut new_id);
        let op = VjOp::LayerTransition {
            layer: "g1".into(),
            value: 30.0,
        };
        run(&mut project, op, &mut new_id);
        let layer = &project.vj.layers[0];
        assert_eq!((layer.master, layer.transition), (1.0, MAX_TRANSITION));
        let op = VjOp::LayerVolume {
            layer: "g1".into(),
            value: f32::NAN,
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn a_reset_should_bring_a_fader_back_to_its_default() {
        let (mut project, mut new_id) = vj_project();
        let layer = project.vj.layers[0].id.clone();
        for (op, value) in [
            ("layerMaster", 0.2),
            ("layerOpacity", 0.3),
            ("layerVolume", 0.4),
            ("layerTransition", 5.0),
        ] {
            let op = serde_json::from_value(
                serde_json::json!({"op": op, "layer": layer, "value": value}),
            )
            .unwrap();
            run(&mut project, op, &mut new_id);
        }
        for level in [
            Level::Master,
            Level::Opacity,
            Level::Volume,
            Level::Transition,
        ] {
            let op = VjOp::LayerLevelReset {
                layer: layer.clone(),
                level,
            };
            run(&mut project, op, &mut new_id);
        }
        let l = &project.vj.layers[0];
        assert_eq!(
            (l.master, l.opacity, l.volume, l.transition),
            (1.0, 1.0, 1.0, 0.0)
        );
        let op = VjOp::GroupLevelReset {
            group: "nope".into(),
            level: Level::Master,
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn a_layer_transition_should_be_random_only_when_asked() {
        let (mut project, mut new_id) = vj_project();
        let op = |random| VjOp::LayerTransitionEffect {
            layer: "g1".into(),
            effect: Blend::Add,
            random,
        };
        run(&mut project, op(true), &mut new_id);
        let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
        assert_eq!(json["transitionRandom"], true);
        assert_eq!(json["transitionEffect"], "add");
        run(&mut project, op(false), &mut new_id);
        let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
        assert!(json.get("transitionRandom").is_none());
    }

    #[test]
    fn every_blend_should_be_listed_once_for_the_random_transition() {
        let names: std::collections::HashSet<String> = Blend::ALL
            .iter()
            .map(|b| {
                serde_json::to_value(b)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(names.len(), Blend::ALL.len());
        // A new variant stops this from compiling until it is counted here and in `ALL`.
        let count = |b: Blend| match b {
            Blend::Alpha
            | Blend::Add
            | Blend::Subtract
            | Blend::Multiply
            | Blend::Screen
            | Blend::Lighten
            | Blend::Darken
            | Blend::Difference
            | Blend::Exclusion
            | Blend::Overlay
            | Blend::SoftLight
            | Blend::HardLight
            | Blend::ColorDodge
            | Blend::ColorBurn
            | Blend::LinearLight
            | Blend::VividLight
            | Blend::PinLight
            | Blend::Hue
            | Blend::Saturation
            | Blend::Color
            | Blend::Luminosity
            | Blend::AlphaCut
            | Blend::LumaKey
            | Blend::LumaKeyInverted
            | Blend::LumaIsAlpha
            | Blend::DifferenceInverted
            | Blend::Mask50
            | Blend::Rgb
            | Blend::Displace => 1,
        };
        assert_eq!(Blend::ALL.iter().map(|b| count(*b)).sum::<usize>(), 29);
    }

    #[test]
    fn a_layer_transition_should_take_any_blend_and_default_to_alpha() {
        let (mut project, mut new_id) = vj_project();
        assert_eq!(project.vj.layers[0].transition_effect, Blend::Alpha);
        let op = VjOp::LayerTransitionEffect {
            layer: "g1".into(),
            effect: Blend::Multiply,
            random: false,
        };
        run(&mut project, op, &mut new_id);
        let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
        assert_eq!(json["transitionEffect"], "multiply");
        let back: Layer = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.transition_effect, Blend::Multiply);
        // Older files have `alpha` or `add`, and none at all for the default.
        let mut older = json;
        older["transitionEffect"] = "add".into();
        let older: Layer = serde_json::from_value(older.clone()).unwrap();
        assert_eq!(older.transition_effect, Blend::Add);
        let mut none = serde_json::to_value(&older).unwrap();
        none.as_object_mut().unwrap().remove("transitionEffect");
        let none: Layer = serde_json::from_value(none).unwrap();
        assert_eq!(none.transition_effect, Blend::Alpha);
    }

    #[test]
    fn validate_should_refuse_levels_out_of_range() {
        let (mut project, _) = vj_project();
        project.vj.layers[0].opacity = 1.5;
        assert!(project.validate().is_err());
        project.vj.layers[0].opacity = 0.5;
        project.vj.layers[0].transition = -1.0;
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_refuse_a_slot_on_a_deck_layer() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        project.vj.layers[0].clips[0].put(Cell::Slot(0));
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_refuse_cue_content_in_a_vj_show() {
        let (project, _) = vj_project();
        let mut with_track = project.clone();
        with_track.timelines[0].tracks = video_project().timelines[0].tracks.clone();
        assert!(with_track.validate().is_err());
        let mut two = project.clone();
        two.timelines
            .push(Timeline::new("Other", crate::FrameRate::Fps25));
        assert!(two.validate().is_err());
    }

    #[test]
    fn json_should_leave_out_a_timeline_project_without_a_grid() {
        let value = serde_json::to_value(video_project()).unwrap();
        assert!(value.get("mode").is_none());
        assert!(value.get("vj").is_none());
    }

    #[test]
    fn json_should_round_trip_a_grid() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        let layer = &mut project.vj.layers[0];
        layer.crossfade = Some(Side::B);
        layer.blend = Blend::Screen;
        layer.transition = 1.5;
        layer.clips[0].playback = Playback::PingPong;
        layer.clips[0].trigger = Trigger::Piano;
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains(r#""mode":"vj""#));
        assert!(json.contains(r#""trigger":"piano""#));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, project);
    }

    #[test]
    fn every_blend_should_keep_its_camel_case_name_and_old_names_should_still_load() {
        for (blend, name) in [
            (Blend::Alpha, "alpha"),
            (Blend::Darken, "darken"),
            (Blend::SoftLight, "softLight"),
            (Blend::ColorDodge, "colorDodge"),
            (Blend::VividLight, "vividLight"),
            (Blend::Luminosity, "luminosity"),
            (Blend::AlphaCut, "alphaCut"),
            (Blend::LumaKey, "lumaKey"),
            (Blend::LumaKeyInverted, "lumaKeyInverted"),
            (Blend::LumaIsAlpha, "lumaIsAlpha"),
            (Blend::DifferenceInverted, "differenceInverted"),
            (Blend::Mask50, "mask50"),
            (Blend::Rgb, "rgb"),
            (Blend::Displace, "displace"),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::to_string(&blend).unwrap(), json);
            assert_eq!(serde_json::from_str::<Blend>(&json).unwrap(), blend);
        }
        assert!(serde_json::from_str::<Blend>("\"nothing\"").is_err());
    }

    #[test]
    fn used_assets_should_count_the_clips_of_the_grid() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        assert!(project.used_assets().contains("v1"));
    }

    fn add_effect(
        project: &mut Project,
        target: EffectTarget,
        kind: &str,
        new_id: impl FnMut() -> String,
    ) {
        let op = VjOp::AddEffect {
            target,
            kind: kind.into(),
        };
        run(project, op, new_id);
    }

    #[test]
    fn effects_should_be_added_moved_bypassed_and_removed_on_clips_and_layers() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let clip = || EffectTarget::Clip("g13".into());
        add_effect(&mut project, clip(), "brightness", &mut new_id); // g14
        add_effect(&mut project, clip(), "mirror", &mut new_id); // g15
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "blur",
            &mut new_id,
        ); // g16
        let names = |project: &Project| -> Vec<String> {
            project.vj.layers[0].clips[0]
                .effects
                .iter()
                .map(|e| e.kind.name())
                .collect()
        };
        assert_eq!(names(&project), ["brightness", "mirror"]);
        assert_eq!(project.vj.layers[0].effects[0].id, "g16");

        let move_to = |effect: &str, index| VjOp::MoveEffect {
            effect: effect.into(),
            index,
        };
        run(&mut project, move_to("g15", 0), &mut new_id);
        assert_eq!(names(&project), ["mirror", "brightness"]);
        run(&mut project, move_to("g15", 99), &mut new_id);
        assert_eq!(names(&project), ["brightness", "mirror"]);

        let bypass = VjOp::EffectBypass {
            effect: "g14".into(),
            bypass: true,
        };
        run(&mut project, bypass, &mut new_id);
        assert!(project.vj.layers[0].clips[0].effects[0].bypass);

        let remove = |effect: &str| VjOp::RemoveEffect {
            effect: effect.into(),
        };
        run(&mut project, remove("g14"), &mut new_id);
        run(&mut project, remove("g16"), &mut new_id);
        assert_eq!(names(&project), ["mirror"]);
        assert!(project.vj.layers[0].effects.is_empty());
        assert!(apply(&mut project, remove("g14"), &mut new_id).is_err());
    }

    #[test]
    fn an_effect_should_take_its_settings_in_range_and_keep_its_kind() {
        let (mut project, mut new_id) = vj_project();
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "zoom",
            &mut new_id,
        ); // g13
        let params = |kind| VjOp::EffectParams {
            effect: "g13".into(),
            kind,
        };
        run(
            &mut project,
            params(EffectKind::Zoom { amount: 50.0 }),
            &mut new_id,
        );
        assert_eq!(
            project.vj.layers[0].effects[0].kind,
            EffectKind::Zoom {
                amount: crate::vj_effects::MAX_ZOOM
            }
        );
        let other = params(EffectKind::Hue { degrees: 10.0 });
        assert!(apply(&mut project, other, &mut new_id).is_err());
        let nan = params(EffectKind::Zoom { amount: f32::NAN });
        assert!(apply(&mut project, nan, &mut new_id).is_err());
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "tint",
            &mut new_id,
        ); // g14
        let bad = VjOp::EffectParams {
            effect: "g14".into(),
            kind: EffectKind::Tint {
                colour: "orange".into(),
                amount: 0.5,
            },
        };
        assert!(apply(&mut project, bad, &mut new_id).is_err());
        let unknown = VjOp::AddEffect {
            target: EffectTarget::Layer("g1".into()),
            kind: "sparkle".into(),
        };
        assert!(apply(&mut project, unknown, &mut new_id).is_err());
    }

    #[test]
    fn a_stack_should_hold_at_most_sixteen_effects() {
        let (mut project, mut new_id) = vj_project();
        for _ in 0..MAX_EFFECTS {
            add_effect(
                &mut project,
                EffectTarget::Layer("g1".into()),
                "invert",
                &mut new_id,
            );
        }
        let seventeenth = VjOp::AddEffect {
            target: EffectTarget::Layer("g1".into()),
            kind: "invert".into(),
        };
        assert!(apply(&mut project, seventeenth, &mut new_id).is_err());
        // Another stack is its own.
        add_effect(
            &mut project,
            EffectTarget::Layer("g2".into()),
            "invert",
            &mut new_id,
        );
        let repeated = project.vj.layers[0].effects[0].clone();
        project.vj.layers[0].effects.push(repeated);
        assert!(project.validate().is_err(), "seventeen, and a repeated id");
    }

    #[test]
    fn the_composition_should_take_every_effect_and_the_history_ones_only_there() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let json = serde_json::to_string(&project).unwrap();
        assert!(!json.contains("compositionEffects") && !json.contains("compositionBypass"));
        // The kinds that need the frames before them are refused on a clip and on a layer.
        for kind in ["feedback", "trails", "bloom"] {
            for target in [
                EffectTarget::Clip("g13".into()),
                EffectTarget::Layer("g1".into()),
            ] {
                let op = VjOp::AddEffect {
                    target,
                    kind: kind.into(),
                };
                assert!(apply(&mut project, op, &mut new_id).is_err(), "{kind}");
            }
        }
        // Every kind goes on the composition, up to the limit of a stack.
        let names: Vec<&str> = EffectKind::names().collect();
        for kind in names.iter().take(MAX_EFFECTS) {
            add_effect(&mut project, EffectTarget::Composition, kind, &mut new_id);
        }
        let seventeenth = VjOp::AddEffect {
            target: EffectTarget::Composition,
            kind: "bloom".into(),
        };
        assert!(apply(&mut project, seventeenth, &mut new_id).is_err());
        // A stack of the composition is its own: the layers' are not counted in it.
        assert_eq!(project.vj.composition_effects.len(), MAX_EFFECTS);
        let hand_made = Effect {
            id: "z1".into(),
            bypass: false,
            modulation: Default::default(),
            kind: EffectKind::default_of("feedback").unwrap(),
        };
        project.vj.layers[0].effects.push(hand_made);
        assert!(project.validate().is_err(), "feedback on a layer");
    }

    #[test]
    fn composition_effects_should_be_edited_like_any_other() {
        let (mut project, mut new_id) = vj_project();
        let composition = || EffectTarget::Composition;
        add_effect(&mut project, composition(), "feedback", &mut new_id); // g13
        add_effect(&mut project, composition(), "bloom", &mut new_id); // g14
        add_effect(&mut project, composition(), "invert", &mut new_id); // g15
        let ids = |project: &Project| -> Vec<String> {
            project
                .vj
                .composition_effects
                .iter()
                .map(|e| e.id.clone())
                .collect()
        };
        assert_eq!(ids(&project), ["g13", "g14", "g15"]);
        let move_to = VjOp::MoveEffect {
            effect: "g15".into(),
            index: 0,
        };
        run(&mut project, move_to, &mut new_id);
        assert_eq!(ids(&project), ["g15", "g13", "g14"]);
        // Settings are brought into range and keep their kind.
        let params = VjOp::EffectParams {
            effect: "g13".into(),
            kind: EffectKind::Feedback {
                amount: 3.0,
                zoom: 9.0,
                rotate: 10.0,
                x: 0.0,
                y: 0.0,
                hue: -500.0,
                blend: crate::vj_effects::FeedbackBlend::Add,
            },
        };
        run(&mut project, params, &mut new_id);
        let EffectKind::Feedback {
            amount, zoom, hue, ..
        } = project.vj.composition_effects[1].kind
        else {
            unreachable!()
        };
        assert_eq!((amount, zoom, hue), (1.0, 2.0, -180.0));
        let wrong = VjOp::EffectParams {
            effect: "g13".into(),
            kind: EffectKind::default_of("trails").unwrap(),
        };
        assert!(apply(&mut project, wrong, &mut new_id).is_err());
        // Bypass, duplicate, reset, modulation and removal find the effect in the composition.
        let bypass = VjOp::EffectBypass {
            effect: "g14".into(),
            bypass: true,
        };
        run(&mut project, bypass, &mut new_id);
        let duplicate = VjOp::DuplicateEffect {
            effect: "g14".into(),
        };
        run(&mut project, duplicate, &mut new_id); // g16
        assert_eq!(ids(&project), ["g15", "g13", "g14", "g16"]);
        let modulate = VjOp::SetModulation {
            target: ModTarget::Effect {
                effect: "g13".into(),
                param: "amount".into(),
            },
            modulation: Some(sine(0.2, 0.8)),
        };
        run(&mut project, modulate, &mut new_id);
        assert!(project.vj.is_modulated());
        let reset = VjOp::ResetEffect {
            effect: "g13".into(),
        };
        run(&mut project, reset, &mut new_id);
        assert!(project.vj.composition_effects[1].modulation.is_empty());
        let remove = VjOp::RemoveEffect {
            effect: "g15".into(),
        };
        run(&mut project, remove, &mut new_id);
        assert_eq!(ids(&project), ["g13", "g14", "g16"]);
        // The stack, its bypass and its effects are in the file, and only when there are any.
        let bypass_all = VjOp::CompositionBypass { bypass: true };
        run(&mut project, bypass_all, &mut new_id);
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains(r#""compositionBypass":true"#), "{json}");
        assert!(json.contains(r#""compositionEffects":[{"id":"g13","effect":"feedback""#));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vj, project.vj);
        back.validate().unwrap();
        let off = VjOp::CompositionBypass { bypass: false };
        run(&mut project, off, &mut new_id);
        assert!(
            !serde_json::to_string(&project)
                .unwrap()
                .contains("compositionBypass")
        );
    }

    #[test]
    fn validate_should_refuse_a_repeated_effect_id_and_a_setting_out_of_range() {
        let (mut project, mut new_id) = vj_project();
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "blur",
            &mut new_id,
        ); // g13
        add_effect(
            &mut project,
            EffectTarget::Layer("g2".into()),
            "blur",
            &mut new_id,
        ); // g14
        project.vj.layers[1].effects[0].id = "g13".into();
        assert!(project.validate().is_err());
        project.vj.layers[1].effects[0].id = "g14".into();
        project.validate().unwrap();
        project.vj.layers[1].effects[0].kind = EffectKind::Blur { radius: 2.0 };
        assert!(project.validate().is_err());
        project.vj.layers[1].effects[0].kind = EffectKind::Blur { radius: f32::NAN };
        assert!(project.validate().is_err());
    }

    #[test]
    fn effects_should_round_trip_in_the_file_and_be_absent_when_there_are_none() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let json = serde_json::to_string(&project).unwrap();
        assert!(!json.contains("effects"), "{json}");
        let clip = || EffectTarget::Clip("g13".into());
        // Every kind goes on the clip, its layer or another layer, 16 to each.
        let names: Vec<&str> = EffectKind::names()
            .filter(|name| !crate::vj_effects::spec_of(name).unwrap().composition_only)
            .collect();
        for (at, kind) in names.iter().enumerate() {
            let target = match at / MAX_EFFECTS {
                0 => clip(),
                1 => EffectTarget::Layer("g1".into()),
                _ => EffectTarget::Layer("g2".into()),
            };
            add_effect(&mut project, target, kind, &mut new_id);
        }
        // The tint is the fifth of the clip: g14 is the first effect.
        let tint = VjOp::EffectParams {
            effect: "g18".into(),
            kind: EffectKind::Tint {
                colour: "#102030".into(),
                amount: 0.25,
            },
        };
        run(&mut project, tint, &mut new_id);
        let bypass = VjOp::EffectBypass {
            effect: "g14".into(),
            bypass: true,
        };
        run(&mut project, bypass, &mut new_id);
        let json = serde_json::to_string(&project).unwrap();
        assert!(
            json.contains(r##"{"id":"g18","effect":"tint","colour":"#102030","amount":0.25}"##),
            "{json}"
        );
        assert!(
            json.contains(r#"{"id":"g14","bypass":true,"effect":"brightness","amount":0.0}"#),
            "{json}"
        );
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vj, project.vj);
        back.validate().unwrap();
        let on_clip = back.vj.layers[0].clips[0].effects.len();
        let on_layers: usize = back.vj.layers.iter().map(|l| l.effects.len()).sum();
        assert_eq!(on_clip + on_layers, names.len());
    }

    #[test]
    fn an_effect_should_be_duplicated_after_itself_and_reset_to_its_defaults() {
        let (mut project, mut new_id) = vj_project();
        let layer = || EffectTarget::Layer("g1".into());
        add_effect(&mut project, layer(), "twirl", &mut new_id); // g13
        add_effect(&mut project, layer(), "hue", &mut new_id); // g14
        let twirl = VjOp::EffectParams {
            effect: "g13".into(),
            kind: EffectKind::Twirl {
                angle: 40.0,
                radius: 0.2,
                x: 0.1,
                y: 0.9,
            },
        };
        run(&mut project, twirl.clone(), &mut new_id);
        let duplicate = VjOp::DuplicateEffect {
            effect: "g13".into(),
        };
        run(&mut project, duplicate, &mut new_id);
        let stack = &project.vj.layers[0].effects;
        assert_eq!(stack.len(), 3);
        assert_eq!(stack[1].kind, stack[0].kind);
        assert_ne!(stack[1].id, stack[0].id);
        assert_eq!(stack[2].id, "g14");
        let reset = VjOp::ResetEffect {
            effect: "g13".into(),
        };
        run(&mut project, reset, &mut new_id);
        let stack = &project.vj.layers[0].effects;
        assert_eq!(stack[0].kind, EffectKind::default_of("twirl").unwrap());
        assert_ne!(stack[1].kind, stack[0].kind);
        // A full stack takes no copy.
        while project.vj.layers[0].effects.len() < MAX_EFFECTS {
            add_effect(&mut project, layer(), "invert", &mut new_id);
        }
        let again = VjOp::DuplicateEffect {
            effect: "g13".into(),
        };
        assert!(apply(&mut project, again, &mut new_id).is_err());
        let missing = VjOp::ResetEffect {
            effect: "nope".into(),
        };
        assert!(apply(&mut project, missing, &mut new_id).is_err());
    }

    fn sine(min: f32, max: f32) -> Modulation {
        use crate::vj_effects::Pace;
        use crate::vj_modulation::{Shape, Source};
        Modulation {
            source: Source::Lfo {
                shape: Shape::Sine,
                pace: Pace {
                    sync: Sync::Beats,
                    rate: 0.5,
                    beats: 1.0,
                },
                phase: 0.0,
            },
            min,
            max,
        }
    }

    fn set(target: ModTarget, modulation: Option<Modulation>) -> VjOp {
        VjOp::SetModulation { target, modulation }
    }

    #[test]
    fn set_modulation_should_store_and_remove_it_on_effects_layers_and_clips() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        add_effect(
            &mut project,
            EffectTarget::Clip("g13".into()),
            "blur",
            &mut new_id,
        ); // g14
        let effect = || ModTarget::Effect {
            effect: "g14".into(),
            param: "radius".into(),
        };
        let layer = |field: &str| ModTarget::Layer {
            layer: "g1".into(),
            field: field.into(),
        };
        let clip = |field: &str| ModTarget::Clip {
            clip: "g13".into(),
            field: field.into(),
        };
        run(
            &mut project,
            set(effect(), Some(sine(0.0, 1.0))),
            &mut new_id,
        );
        run(
            &mut project,
            set(layer("opacity"), Some(sine(1.0, 0.2))),
            &mut new_id,
        );
        run(
            &mut project,
            set(clip("positionX"), Some(sine(-1.0, 1.0))),
            &mut new_id,
        );
        let layer0 = &project.vj.layers[0];
        assert_eq!(
            layer0.clips[0].effects[0].modulation["radius"],
            sine(0.0, 1.0)
        );
        assert_eq!(
            layer0.modulation["opacity"].min, 1.0,
            "min may be above max"
        );
        assert!(layer0.clips[0].modulation.contains_key("positionX"));
        // The stored values stay as they were.
        assert_eq!(layer0.opacity, 1.0);

        // Out of range is brought into it; a number that is not one is refused.
        run(
            &mut project,
            set(layer("master"), Some(sine(-3.0, 9.0))),
            &mut new_id,
        );
        let master = project.vj.layers[0].modulation["master"];
        assert_eq!((master.min, master.max), (0.0, 1.0));
        let mut nan = sine(0.0, 1.0);
        nan.max = f32::NAN;
        assert!(apply(&mut project, set(layer("master"), Some(nan)), &mut new_id).is_err());

        // Null removes.
        run(&mut project, set(layer("master"), None), &mut new_id);
        assert!(!project.vj.layers[0].modulation.contains_key("master"));
    }

    #[test]
    fn a_generator_number_should_be_modulated_and_go_when_the_generator_changes_kind() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::PlaceGenerator {
            generator: Generator::Text {
                text: "FLUX".into(),
                font: "Inter".into(),
                style: "Bold".into(),
                size: 0.12,
                colour: "#ffffff".into(),
                opacity: 1.0,
                align: Default::default(),
                position_x: 0.5,
                position_y: 0.5,
                rotation: 0.0,
                tracking: 0.0,
                leading: 0.0,
                lines: Default::default(),
                line: 0,
                advance: 0,
            },
            layer: "g1".into(),
            column: "g5".into(),
        };
        run(&mut project, op, &mut new_id);
        let id = project.vj.layers[0].clips[0].id.clone();
        let target = |field: &str| ModTarget::Clip {
            clip: id.clone(),
            field: field.into(),
        };
        run(
            &mut project,
            set(target("generator.rotation"), Some(sine(-9999.0, 90.0))),
            &mut new_id,
        );
        run(
            &mut project,
            set(target("generator.size"), Some(sine(0.1, 0.3))),
            &mut new_id,
        );
        let stored = &project.vj.layers[0].clips[0].modulation;
        assert_eq!(
            stored["generator.rotation"].min, -360.0,
            "brought into range"
        );
        // A strobe's rate is not a text block's number.
        assert!(
            apply(
                &mut project,
                set(target("generator.rate"), Some(sine(1.0, 2.0))),
                &mut new_id
            )
            .is_err()
        );
        assert!(
            apply(
                &mut project,
                set(target("generator.colour"), Some(sine(0.0, 1.0))),
                &mut new_id
            )
            .is_err()
        );
        project.validate().unwrap();
        // Another kind of generator keeps what it has too, and drops what it does not.
        let strobe = VjOp::ClipGenerator {
            clip: id.clone(),
            generator: Generator::Strobe {
                colour: "#ffffff".into(),
                rate: 8.0,
                width: 0.5,
            },
        };
        run(&mut project, strobe, &mut new_id);
        assert!(project.vj.layers[0].clips[0].modulation.is_empty());
        project.validate().unwrap();
    }

    #[test]
    fn set_modulation_should_refuse_what_cannot_be_modulated() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        add_effect(
            &mut project,
            EffectTarget::Clip("g13".into()),
            "kaleidoscope",
            &mut new_id,
        ); // g14
        add_effect(
            &mut project,
            EffectTarget::Clip("g13".into()),
            "mirror",
            &mut new_id,
        ); // g15
        let effect = |id: &str, param: &str| ModTarget::Effect {
            effect: id.into(),
            param: param.into(),
        };
        let mut refused = |target: ModTarget| {
            assert!(apply(&mut project, set(target, Some(sine(0.0, 1.0))), &mut new_id).is_err());
        };
        // A choice, a rate, a length of beats, a setting the kind lacks, an unknown effect.
        refused(effect("g15", "axis"));
        refused(effect("g14", "rate"));
        refused(effect("g14", "beats"));
        refused(effect("g14", "nope"));
        refused(effect("nope", "x"));
        refused(ModTarget::Layer {
            layer: "g1".into(),
            field: "transition".into(),
        });
        refused(ModTarget::Clip {
            clip: "g13".into(),
            field: "opacity".into(),
        });
        refused(ModTarget::Layer {
            layer: "nope".into(),
            field: "master".into(),
        });
    }

    #[test]
    fn modulation_should_follow_duplicate_and_clear_on_reset_and_survive_a_round_trip() {
        let (mut project, mut new_id) = vj_project();
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "zoom",
            &mut new_id,
        ); // g13
        let target = ModTarget::Effect {
            effect: "g13".into(),
            param: "amount".into(),
        };
        run(&mut project, set(target, Some(sine(0.5, 2.0))), &mut new_id);
        let duplicate = VjOp::DuplicateEffect {
            effect: "g13".into(),
        };
        run(&mut project, duplicate, &mut new_id);
        let stack = &project.vj.layers[0].effects;
        assert_eq!(stack[1].modulation, stack[0].modulation);
        assert_eq!(stack[1].modulation.len(), 1);

        // Changing the setting keeps the modulation; the stored value is the base.
        let params = VjOp::EffectParams {
            effect: "g13".into(),
            kind: EffectKind::Zoom { amount: 1.5 },
        };
        run(&mut project, params, &mut new_id);
        assert_eq!(project.vj.layers[0].effects[0].modulation.len(), 1);

        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vj, project.vj);
        back.validate().unwrap();
        // A plain effect writes no modulation key.
        let plain = serde_json::to_value(&project.vj.layers[1]).unwrap();
        assert!(plain.get("modulation").is_none());

        let reset = VjOp::ResetEffect {
            effect: "g13".into(),
        };
        run(&mut project, reset, &mut new_id);
        let stack = &project.vj.layers[0].effects;
        assert!(stack[0].modulation.is_empty());
        assert_eq!(
            stack[1].modulation.len(),
            1,
            "only the reset effect is cleared"
        );
    }

    #[test]
    fn validate_should_refuse_a_modulation_out_of_range_or_on_a_wrong_field() {
        let (mut project, _) = vj_project();
        project.vj.layers[0]
            .modulation
            .insert("opacity".into(), sine(0.0, 2.0));
        assert!(project.validate().is_err());
        project.vj.layers[0].modulation.clear();
        project.vj.layers[0]
            .modulation
            .insert("blend".into(), sine(0.0, 1.0));
        assert!(project.validate().is_err());
        project.vj.layers[0].modulation.clear();
        project.vj.layers[0]
            .modulation
            .insert("volume".into(), sine(0.0, 1.0));
        project.validate().unwrap();
    }

    #[test]
    fn a_project_saved_before_modulation_should_load_without_any() {
        let (project, _) = vj_project();
        let json = serde_json::to_string(&project).unwrap();
        assert!(!json.contains("modulation"));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert!(!back.vj.is_modulated());
    }

    #[test]
    fn autopilot_should_pick_next_previous_and_random_skipping_empty_cells() {
        let cells = [true, false, true, true, false];
        let pick = |mode: Autopilot, current, roll| mode.pick(&cells, current, roll);
        assert_eq!(pick(Autopilot::Next, Some(0), 0), Some(2));
        assert_eq!(pick(Autopilot::Next, Some(3), 0), Some(0), "wraps");
        assert_eq!(pick(Autopilot::Previous, Some(2), 0), Some(0));
        assert_eq!(pick(Autopilot::Previous, Some(0), 0), Some(3), "wraps");
        assert_eq!(pick(Autopilot::Next, None, 0), Some(0));
        assert_eq!(pick(Autopilot::Previous, None, 0), Some(3));
        assert_eq!(pick(Autopilot::Off, Some(0), 0), None);
        for roll in 0..8 {
            assert_ne!(pick(Autopilot::Random, Some(2), roll), Some(2));
        }
        assert_eq!(pick(Autopilot::Random, Some(0), 1), Some(3));
        assert_eq!(Autopilot::Next.pick(&[false, true], Some(1), 0), Some(1));
        assert_eq!(Autopilot::Random.pick(&[false, true], Some(1), 5), Some(1));
        assert_eq!(Autopilot::Next.pick(&[false, false], None, 0), None);
        assert_eq!(Autopilot::Next.pick(&[], None, 0), None);
    }

    #[test]
    fn set_modulation_should_set_and_clear_a_knob_and_refuse_a_ninth() {
        let (mut project, new_id) = vj_project();
        let m = sine(0.1, 0.9);
        let knob = |n| ModTarget::Knob { knob: n };
        run(&mut project, set(knob(7), Some(m)), new_id);
        assert_eq!(project.vj.dashboard[7].modulation, Some(m));
        assert!(apply(&mut project, set(knob(8), Some(m)), String::new).is_err());
        run(&mut project, set(knob(7), None), String::new);
        assert_eq!(project.vj.dashboard[7].modulation, None);
    }
}
