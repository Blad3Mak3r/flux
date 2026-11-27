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

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::generator::rgb;
use crate::vj_modulation::Modulation;
use crate::{ProjectError, Result};

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

fn is_false(value: &bool) -> bool {
    !*value
}

/// Which side of the picture an effect mirrors onto the other, or which way it waves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MirrorAxis {
    /// The left half onto the right.
    #[default]
    Horizontal,
    /// The top half onto the bottom.
    Vertical,
    /// Both: the top left quarter onto the other three.
    Both,
}

/// What an animated effect's rate is counted in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sync {
    /// Cycles a second (`rate`).
    #[default]
    Free,
    /// One cycle every `beats` beats of the show's tempo.
    Beats,
}

/// How fast an animated effect runs: `rate` cycles a second when free, or one cycle every `beats`
/// beats of the tempo when synced. What a cycle is depends on the effect (a turn of a spin, a
/// period of a wave, a step of a glitch).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pace {
    pub sync: Sync,
    /// Cycles a second, 0 to 30; 0 stands still.
    pub rate: f32,
    /// Beats a cycle: one of [`BEAT_CYCLES`].
    pub beats: f32,
}

/// What a strobe shows while it is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrobeMode {
    /// The strobe's colour over the picture.
    #[default]
    Flash,
    /// Black.
    Blackout,
}

/// How the feedback of a [`EffectKind::Feedback`] meets the new frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeedbackBlend {
    /// The new frame and the old one are mixed, `amount` of the way to the old.
    #[default]
    Mix,
    /// The old frame, scaled by `amount`, is added to the new.
    Add,
    /// The lighter of the new frame and the old one scaled by `amount`.
    Lighten,
}

/// How the trail of a [`EffectKind::Trails`] meets the new frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrailMode {
    /// The new frame and the trail are mixed, `decay` of the way to the trail.
    #[default]
    Blend,
    /// The lighter of the new frame and the trail scaled by `decay`.
    Lighten,
    /// The trail scaled by `decay` is added to the new frame.
    Add,
}

/// What an edge detect shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeMode {
    /// Only the edges, light on black.
    #[default]
    Edges,
    /// The edges added to the picture.
    Glow,
}

/// What an effect does, with its settings. Colour effects work on the light of the picture (see
/// `flux-render`); the distortions move it; the blurs and the split soften or separate it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "camelCase")]
pub enum EffectKind {
    /// Adds `amount` to every colour, -1 to 1; 0 is none.
    Brightness { amount: f32 },
    /// Pushes the colours away from (above 1) or towards (below 1) mid grey, 0 to 4; 1 is none.
    Contrast { amount: f32 },
    /// Pushes the colours away from (above 1) or towards (below 1) their grey, 0 to 4; 1 is none.
    Saturation { amount: f32 },
    /// Turns the colours about the grey axis, -180 to 180 degrees; 0 is none.
    Hue { degrees: f32 },
    /// Mixes the picture's grey with `colour` (`#rrggbb`), `amount` 0 to 1 of the way.
    Tint { colour: String, amount: f32 },
    /// Mixes the colours with their negative, `amount` 0 to 1 of the way.
    Invert { amount: f32 },
    /// Moves the black and the white point of the picture and bends its mid tones by `gamma`.
    Levels { black: f32, white: f32, gamma: f32 },
    /// Paints the picture's grey from `dark` to `light`, `amount` 0 to 1 of the way (a gradient
    /// map).
    Colorize {
        dark: String,
        light: String,
        amount: f32,
    },
    /// Turns the colours above `threshold` into their negative.
    Solarize { threshold: f32 },
    /// Cuts each colour to `levels` steps.
    Posterize { levels: f32 },
    /// Turns the picture into `dark` and `light` where its grey is below or above `level`, over a
    /// `softness` wide transition.
    Threshold {
        level: f32,
        softness: f32,
        dark: String,
        light: String,
    },
    /// Finds the edges of the picture and shows them, or adds them to it.
    EdgeDetect { strength: f32, mode: EdgeMode },
    /// Darkens the corners; `roundness` 0 follows the frame and 1 is an ellipse.
    Vignette {
        amount: f32,
        softness: f32,
        roundness: f32,
    },
    /// Film grain; `size` 0 to 1 is how coarse, and it moves when `animated`.
    Noise {
        amount: f32,
        size: f32,
        animated: bool,
    },
    /// Dark lines across the picture, `count` of them, that scroll `rate` lines a cycle.
    Scanlines {
        count: f32,
        intensity: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Softens the picture over a radius of `radius` 0 to 1 of the largest blur.
    Blur { radius: f32 },
    /// Smears the picture round the point `x`, `y` (0 to 1 across and down).
    RadialBlur { amount: f32, x: f32, y: f32 },
    /// Smears the picture along rays from the point `x`, `y`.
    ZoomBlur { amount: f32, x: f32, y: f32 },
    /// Folds the picture onto itself.
    Mirror { axis: MirrorAxis },
    /// Draws the picture `amount` times larger about its centre, 0.25 to 8; outside it is
    /// transparent.
    Zoom { amount: f32 },
    /// Moves the picture by `x` and `y` of its size, right and down, -1 to 1; what leaves one side
    /// comes back on the other.
    Shift { x: f32, y: f32 },
    /// Turns the picture by `angle` degrees and `spin` turns every cycle; the corners it leaves
    /// are transparent.
    Rotate {
        angle: f32,
        spin: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Repeats a wedge of the picture round the point `x`, `y`: `segments` of them, turned by
    /// `rotation` degrees and `spin` turns every cycle.
    Kaleidoscope {
        segments: f32,
        rotation: f32,
        spin: f32,
        x: f32,
        y: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Bends the picture from a rectangle into a disc, `amount` 0 to 1 of the way.
    Polar { amount: f32 },
    /// Ripples the picture: `amplitude` of its size, `frequency` waves across it, travelling one
    /// wave every cycle.
    Wave {
        amplitude: f32,
        frequency: f32,
        direction: MirrorAxis,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Spins the picture about the point `x`, `y`, most at the centre and not at all `radius`
    /// (in picture heights) away.
    Twirl {
        angle: f32,
        radius: f32,
        x: f32,
        y: f32,
    },
    /// Swells (positive) or pinches (negative) the picture round the point `x`, `y`.
    Bulge {
        amount: f32,
        radius: f32,
        x: f32,
        y: f32,
    },
    /// Repeats the picture `columns` by `rows` times; alternate tiles are turned over when
    /// `mirror`.
    Tile {
        columns: f32,
        rows: f32,
        mirror: bool,
    },
    /// Breaks the picture into square cells of `size` of its height.
    Pixelate { size: f32 },
    /// Pushes the picture about with a drifting noise, `amplitude` of its size, in blobs of
    /// `scale` across it.
    Displace {
        amplitude: f32,
        scale: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Scrolls the picture `x` and `y` of its size every cycle, round or leaving transparent.
    Slide {
        x: f32,
        y: f32,
        wrap: bool,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Shifts random bands of the picture sideways, now and then splitting their colours; the
    /// bands change every cycle.
    ShiftGlitch {
        amount: f32,
        block: f32,
        probability: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Every cycle, with a `probability`, jumps the picture's position, size and light at random.
    Twitch {
        amount: f32,
        probability: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Rolls the picture up like a TV out of vertical hold, with `jitter` shaking its lines.
    Roll {
        jitter: f32,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Pulls the red and the blue of the picture apart, along `angle`.
    RgbSplit {
        amount: f32,
        angle: f32,
        animate: bool,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Flashes the picture on a `duty` of each cycle.
    Strobe {
        colour: String,
        duty: f32,
        mode: StrobeMode,
        #[serde(flatten)]
        pace: Pace,
    },
    /// Draws the previous output again under (or over) the new frame, zoomed by `zoom` and turned
    /// by `rotate` degrees a second, moved by `x` and `y` of its size a second and its hue turned
    /// by `hue` degrees a second: tunnels and spirals. `amount` is the share of the old frame kept
    /// every 60th of a second. Only on the composition.
    Feedback {
        amount: f32,
        zoom: f32,
        rotate: f32,
        x: f32,
        y: f32,
        hue: f32,
        blend: FeedbackBlend,
    },
    /// Keeps `decay` of the previous output every 60th of a second under the new frame: the
    /// trails of whatever moves. Only on the composition.
    Trails { decay: f32, mode: TrailMode },
    /// Adds back a glow of what is brighter than `threshold`, `intensity` times, spread over
    /// `radius`. Only on the composition.
    Bloom {
        threshold: f32,
        intensity: f32,
        radius: f32,
    },
}

/// A setting of an effect: its key in the file, and what it takes.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ParamSpec {
    /// A number from `min` to `max`. With `snap`, only those values are taken (the nearest).
    Number {
        key: &'static str,
        min: f32,
        max: f32,
        step: f32,
        default: f32,
        /// Only whole numbers are taken.
        whole: bool,
        /// What it is counted in: `%` (the number times 100), `°`, `Hz`, or nothing.
        unit: &'static str,
        snap: Vec<f32>,
        /// Whether a modulation can drive it: not a rate or a length of beats, which a modulation
        /// is itself paced by.
        modulatable: bool,
    },
    /// One of `options`.
    Choice {
        key: &'static str,
        options: Vec<&'static str>,
        default: &'static str,
    },
    /// `#rrggbb`.
    Colour {
        key: &'static str,
        default: &'static str,
    },
    /// On or off.
    Flag { key: &'static str, default: bool },
}

impl ParamSpec {
    fn key(&self) -> &'static str {
        match self {
            Self::Number { key, .. }
            | Self::Choice { key, .. }
            | Self::Colour { key, .. }
            | Self::Flag { key, .. } => key,
        }
    }

    fn default_value(&self) -> Value {
        match self {
            Self::Number { default, .. } => Value::from(f64::from(*default)),
            Self::Choice { default, .. } | Self::Colour { default, .. } => Value::from(*default),
            Self::Flag { default, .. } => Value::from(*default),
        }
    }

    /// Brings `value` into range in place; a wrong type or a bad colour is refused.
    fn tidy(&self, value: &mut Value) -> Result<()> {
        let bad = |what: &str| {
            Err(ProjectError::Invalid(format!(
                "the effect setting {} {what}",
                self.key()
            )))
        };
        match self {
            Self::Number {
                min,
                max,
                whole,
                snap,
                ..
            } => {
                let Some(number) = value.as_f64().filter(|n| n.is_finite()) else {
                    return bad("is a number");
                };
                let mut number = (number as f32).clamp(*min, *max);
                if *whole {
                    number = number.round();
                }
                if let Some(nearest) = snap
                    .iter()
                    .copied()
                    .min_by(|a, b| (a - number).abs().total_cmp(&(b - number).abs()))
                {
                    number = nearest;
                }
                *value = Value::from(f64::from(number));
            }
            Self::Choice { options, .. } => {
                if !value.as_str().is_some_and(|v| options.contains(&v)) {
                    return bad("is not one of its options");
                }
            }
            Self::Colour { .. } => {
                if !value.as_str().is_some_and(|v| rgb(v).is_some()) {
                    return bad("must look like #rrggbb");
                }
            }
            Self::Flag { .. } => {
                if !value.is_boolean() {
                    return bad("is on or off");
                }
            }
        }
        Ok(())
    }
}

/// The groups the inspector lists effects in, in order.
pub const CATEGORIES: [&str; 7] = [
    "colour", "distort", "blur", "glitch", "stylise", "time", "feedback",
];

/// What one kind of effect is: its name, its category and its settings.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectSpec {
    pub name: &'static str,
    pub category: &'static str,
    /// Whether it moves with time, and so has a sync, a rate and a beat length.
    pub animated: bool,
    /// Whether it needs the frames before it or the pixels around it, and so can only be on the
    /// composition's stack, never a clip's or a layer's.
    pub composition_only: bool,
    pub params: Vec<ParamSpec>,
}

fn num(key: &'static str, min: f32, max: f32, step: f32, default: f32) -> ParamSpec {
    let unit = match key {
        "degrees" | "angle" | "rotation" => "°",
        "rate" => "Hz",
        "gamma" | "spin" | "frequency" | "scale" | "strength" => "",
        _ => "%",
    };
    ParamSpec::Number {
        key,
        min,
        max,
        step,
        default,
        whole: false,
        unit,
        snap: Vec::new(),
        modulatable: key != "rate",
    }
}

fn whole(key: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec::Number {
        key,
        min,
        max,
        step: 1.0,
        default,
        whole: true,
        unit: "",
        snap: Vec::new(),
        modulatable: true,
    }
}

fn choice(key: &'static str, options: &[&'static str], default: &'static str) -> ParamSpec {
    ParamSpec::Choice {
        key,
        options: options.to_vec(),
        default,
    }
}

fn colour(key: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec::Colour { key, default }
}

fn flag(key: &'static str, default: bool) -> ParamSpec {
    ParamSpec::Flag { key, default }
}

/// The three settings of a [`Pace`], starting at `rate` cycles a second or a cycle of `beats`.
fn pace(rate: f32, beats: f32) -> [ParamSpec; 3] {
    [
        choice("sync", &["free", "beats"], "free"),
        num("rate", 0.0, 30.0, 0.01, rate),
        ParamSpec::Number {
            key: "beats",
            min: BEAT_CYCLES[0],
            max: BEAT_CYCLES[BEAT_CYCLES.len() - 1],
            step: 0.0,
            default: beats,
            whole: false,
            unit: "",
            snap: BEAT_CYCLES.to_vec(),
            modulatable: false,
        },
    ]
}

fn kind(
    name: &'static str,
    category: &'static str,
    mut params: Vec<ParamSpec>,
    paced: Option<(f32, f32)>,
) -> EffectSpec {
    if let Some((rate, beats)) = paced {
        params.extend(pace(rate, beats));
    }
    EffectSpec {
        name,
        category,
        animated: paced.is_some(),
        composition_only: category == "feedback",
        params,
    }
}

/// A number setting counted in `unit`.
fn num_in(
    key: &'static str,
    unit: &'static str,
    (min, max, step, default): (f32, f32, f32, f32),
) -> ParamSpec {
    ParamSpec::Number {
        key,
        min,
        max,
        step,
        default,
        whole: false,
        unit,
        snap: Vec::new(),
        modulatable: true,
    }
}

/// Every kind of effect, in the order the user is offered them within their category.
pub fn catalog() -> &'static [EffectSpec] {
    static CATALOG: OnceLock<Vec<EffectSpec>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let axes = ["horizontal", "vertical", "both"];
        let x = |default| num("x", 0.0, 1.0, 0.01, default);
        let y = |default| num("y", 0.0, 1.0, 0.01, default);
        vec![
            kind(
                "brightness",
                "colour",
                vec![num("amount", -1.0, 1.0, 0.01, 0.0)],
                None,
            ),
            kind(
                "contrast",
                "colour",
                vec![num("amount", 0.0, 4.0, 0.01, 1.0)],
                None,
            ),
            kind(
                "saturation",
                "colour",
                vec![num("amount", 0.0, 4.0, 0.01, 1.0)],
                None,
            ),
            kind(
                "hue",
                "colour",
                vec![num("degrees", -180.0, 180.0, 1.0, 0.0)],
                None,
            ),
            kind(
                "tint",
                "colour",
                vec![
                    colour("colour", "#ff8800"),
                    num("amount", 0.0, 1.0, 0.01, 0.5),
                ],
                None,
            ),
            kind(
                "invert",
                "colour",
                vec![num("amount", 0.0, 1.0, 0.01, 1.0)],
                None,
            ),
            kind(
                "levels",
                "colour",
                vec![
                    num("black", 0.0, 1.0, 0.01, 0.0),
                    num("white", 0.0, 1.0, 0.01, 1.0),
                    num("gamma", 0.1, 4.0, 0.01, 1.0),
                ],
                None,
            ),
            kind(
                "colorize",
                "colour",
                vec![
                    colour("dark", "#1a0b3d"),
                    colour("light", "#ffb347"),
                    num("amount", 0.0, 1.0, 0.01, 1.0),
                ],
                None,
            ),
            kind(
                "solarize",
                "colour",
                vec![num("threshold", 0.0, 1.0, 0.01, 0.5)],
                None,
            ),
            kind(
                "mirror",
                "distort",
                vec![choice("axis", &axes, "horizontal")],
                None,
            ),
            kind(
                "zoom",
                "distort",
                vec![num("amount", MIN_ZOOM, MAX_ZOOM, 0.01, 1.0)],
                None,
            ),
            kind(
                "shift",
                "distort",
                vec![
                    num("x", -1.0, 1.0, 0.01, 0.0),
                    num("y", -1.0, 1.0, 0.01, 0.0),
                ],
                None,
            ),
            kind(
                "rotate",
                "distort",
                vec![
                    num("angle", -180.0, 180.0, 1.0, 0.0),
                    num("spin", -4.0, 4.0, 0.01, 0.0),
                ],
                Some((0.25, 4.0)),
            ),
            kind(
                "kaleidoscope",
                "distort",
                vec![
                    whole("segments", 2.0, 32.0, 6.0),
                    num("rotation", -180.0, 180.0, 1.0, 0.0),
                    num("spin", -4.0, 4.0, 0.01, 0.25),
                    x(0.5),
                    y(0.5),
                ],
                Some((0.5, 4.0)),
            ),
            kind(
                "polar",
                "distort",
                vec![num("amount", 0.0, 1.0, 0.01, 1.0)],
                None,
            ),
            kind(
                "wave",
                "distort",
                vec![
                    num("amplitude", 0.0, 0.25, 0.001, 0.03),
                    num("frequency", 0.0, 32.0, 0.1, 4.0),
                    choice("direction", &axes, "horizontal"),
                ],
                Some((0.5, 1.0)),
            ),
            kind(
                "twirl",
                "distort",
                vec![
                    num("angle", -1440.0, 1440.0, 1.0, 180.0),
                    num("radius", 0.0, 2.0, 0.01, 0.5),
                    x(0.5),
                    y(0.5),
                ],
                None,
            ),
            kind(
                "bulge",
                "distort",
                vec![
                    num("amount", -1.0, 1.0, 0.01, 0.5),
                    num("radius", 0.05, 2.0, 0.01, 0.6),
                    x(0.5),
                    y(0.5),
                ],
                None,
            ),
            kind(
                "tile",
                "distort",
                vec![
                    whole("columns", 1.0, 16.0, 2.0),
                    whole("rows", 1.0, 16.0, 2.0),
                    flag("mirror", true),
                ],
                None,
            ),
            kind(
                "pixelate",
                "distort",
                vec![num("size", 0.0, 0.25, 0.001, 0.02)],
                None,
            ),
            kind(
                "displace",
                "distort",
                vec![
                    num("amplitude", 0.0, 0.25, 0.001, 0.03),
                    num("scale", 0.5, 32.0, 0.1, 6.0),
                ],
                Some((0.5, 2.0)),
            ),
            kind(
                "blur",
                "blur",
                vec![num("radius", 0.0, 1.0, 0.01, 0.1)],
                None,
            ),
            kind(
                "radialBlur",
                "blur",
                vec![num("amount", 0.0, 1.0, 0.01, 0.3), x(0.5), y(0.5)],
                None,
            ),
            kind(
                "zoomBlur",
                "blur",
                vec![num("amount", 0.0, 1.0, 0.01, 0.3), x(0.5), y(0.5)],
                None,
            ),
            kind(
                "shiftGlitch",
                "glitch",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.15),
                    num("block", 0.005, 0.5, 0.001, 0.06),
                    num("probability", 0.0, 1.0, 0.01, 0.35),
                ],
                Some((8.0, 0.5)),
            ),
            kind(
                "twitch",
                "glitch",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.5),
                    num("probability", 0.0, 1.0, 0.01, 0.5),
                ],
                Some((4.0, 0.5)),
            ),
            kind(
                "rgbSplit",
                "glitch",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.3),
                    num("angle", -180.0, 180.0, 1.0, 0.0),
                    flag("animate", false),
                ],
                Some((0.5, 4.0)),
            ),
            kind(
                "posterize",
                "stylise",
                vec![whole("levels", 2.0, 32.0, 4.0)],
                None,
            ),
            kind(
                "threshold",
                "stylise",
                vec![
                    num("level", 0.0, 1.0, 0.01, 0.5),
                    num("softness", 0.0, 1.0, 0.01, 0.05),
                    colour("dark", "#000000"),
                    colour("light", "#ffffff"),
                ],
                None,
            ),
            kind(
                "edgeDetect",
                "stylise",
                vec![
                    num("strength", 0.0, 8.0, 0.01, 1.0),
                    choice("mode", &["edges", "glow"], "edges"),
                ],
                None,
            ),
            kind(
                "vignette",
                "stylise",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.6),
                    num("softness", 0.0, 1.0, 0.01, 0.5),
                    num("roundness", 0.0, 1.0, 0.01, 0.5),
                ],
                None,
            ),
            kind(
                "noise",
                "stylise",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.25),
                    num("size", 0.0, 1.0, 0.01, 0.1),
                    flag("animated", true),
                ],
                None,
            ),
            kind(
                "scanlines",
                "stylise",
                vec![
                    whole("count", 2.0, 540.0, 120.0),
                    num("intensity", 0.0, 1.0, 0.01, 0.5),
                ],
                Some((0.0, 4.0)),
            ),
            kind(
                "strobe",
                "time",
                vec![
                    colour("colour", "#ffffff"),
                    num("duty", 0.02, 1.0, 0.01, 0.25),
                    choice("mode", &["flash", "blackout"], "flash"),
                ],
                Some((4.0, 0.5)),
            ),
            kind(
                "slide",
                "time",
                vec![
                    num("x", -4.0, 4.0, 0.01, 0.25),
                    num("y", -4.0, 4.0, 0.01, 0.0),
                    flag("wrap", true),
                ],
                Some((1.0, 4.0)),
            ),
            kind(
                "roll",
                "time",
                vec![num("jitter", 0.0, 1.0, 0.01, 0.2)],
                Some((0.3, 4.0)),
            ),
            kind(
                "feedback",
                "feedback",
                vec![
                    num("amount", 0.0, 1.0, 0.01, 0.6),
                    num_in("zoom", "×", (0.5, 2.0, 0.001, 1.02)),
                    num_in("rotate", "°/s", (-180.0, 180.0, 1.0, 0.0)),
                    num_in("x", "%", (-1.0, 1.0, 0.01, 0.0)),
                    num_in("y", "%", (-1.0, 1.0, 0.01, 0.0)),
                    num_in("hue", "°/s", (-180.0, 180.0, 1.0, 0.0)),
                    choice("blend", &["mix", "add", "lighten"], "mix"),
                ],
                None,
            ),
            kind(
                "trails",
                "feedback",
                vec![
                    num("decay", 0.0, 1.0, 0.01, 0.9),
                    choice("mode", &["blend", "lighten", "add"], "blend"),
                ],
                None,
            ),
            kind(
                "bloom",
                "feedback",
                vec![
                    num("threshold", 0.0, 1.0, 0.01, 0.6),
                    num_in("intensity", "%", (0.0, 4.0, 0.01, 1.0)),
                    num("radius", 0.0, 1.0, 0.01, 0.5),
                ],
                None,
            ),
        ]
    })
}

/// The description of the kind called `name`.
pub fn spec_of(name: &str) -> Result<&'static EffectSpec> {
    catalog()
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| ProjectError::Invalid(format!("no effect called {name}")))
}

impl EffectKind {
    /// The names of the kinds, in the order of the [`catalog`].
    pub fn names() -> impl Iterator<Item = &'static str> {
        catalog().iter().map(|spec| spec.name)
    }

    /// The kind named `name` with the settings it starts with, which change nothing of the picture
    /// where they can (a tint starts half way to orange, a blur at a tenth).
    pub fn default_of(name: &str) -> Result<Self> {
        let spec = spec_of(name)?;
        let mut map = Map::new();
        map.insert("effect".into(), Value::from(name));
        for param in &spec.params {
            map.insert(param.key().into(), param.default_value());
        }
        Self::from_map(map)
    }

    fn from_map(map: Map<String, Value>) -> Result<Self> {
        serde_json::from_value(Value::Object(map))
            .map_err(|err| ProjectError::Invalid(format!("an effect is not valid: {err}")))
    }

    fn to_map(&self) -> Map<String, Value> {
        match serde_json::to_value(self) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        }
    }

    /// Its name, as in the [`catalog`].
    pub fn name(&self) -> String {
        match self.to_map().get("effect") {
            Some(Value::String(name)) => name.clone(),
            _ => String::new(),
        }
    }

    /// Its name and settings as the map it is stored as.
    pub(crate) fn settings(&self) -> Map<String, Value> {
        self.to_map()
    }

    /// The kind with the settings of `settings` that are settings of this kind (the others are
    /// ignored), each brought into range; the ones `settings` lacks keep their value.
    pub(crate) fn with_settings(&self, settings: &Map<String, Value>) -> Result<Self> {
        let mut map = self.to_map();
        for param in &spec_of(&self.name())?.params {
            if let Some(value) = settings.get(param.key()) {
                map.insert(param.key().into(), value.clone());
            }
        }
        Self::from_tidy_map(map)
    }

    /// Whether it can only be on the composition's stack (see [`EffectSpec::composition_only`]).
    pub fn composition_only(&self) -> bool {
        spec_of(&self.name()).is_ok_and(|spec| spec.composition_only)
    }

    /// Whether `other` is the same kind of effect, possibly with other settings.
    pub fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// The kind with its settings brought into range; a setting that is not a number or a colour
    /// that is not `#rrggbb` is refused.
    pub fn tidy(self) -> Result<Self> {
        Self::from_tidy_map(self.to_map())
    }

    /// The kind of `map` with every setting brought into range: one trip through the map instead
    /// of one per step.
    fn from_tidy_map(mut map: Map<String, Value>) -> Result<Self> {
        let name = match map.get("effect") {
            Some(Value::String(name)) => name.clone(),
            _ => String::new(),
        };
        for param in &spec_of(&name)?.params {
            let Some(value) = map.get_mut(param.key()) else {
                return Err(ProjectError::Invalid(format!(
                    "a {name} effect lacks {}",
                    param.key()
                )));
            };
            param.tidy(value)?;
        }
        Self::from_map(map)
    }

    /// The range of the setting `param` if it can be modulated: a number that is not a rate or a
    /// length of beats, which are what a modulation itself is paced by.
    pub fn modulatable_range(&self, param: &str) -> Result<(f32, f32)> {
        let name = self.name();
        let range = spec_of(&name)?.params.iter().find_map(|spec| match spec {
            ParamSpec::Number {
                key,
                min,
                max,
                modulatable: true,
                ..
            } if *key == param => Some((*min, *max)),
            _ => None,
        });
        range.ok_or_else(|| {
            ProjectError::Invalid(format!("the {name} setting {param} cannot be modulated"))
        })
    }

    /// The range of the numeric setting `param`, whether or not a modulation can drive it; `None`
    /// for a setting that is not a number of this kind.
    pub fn number_range(&self, param: &str) -> Option<(f32, f32)> {
        spec_of(&self.name())
            .ok()?
            .params
            .iter()
            .find_map(|spec| match spec {
                ParamSpec::Number { key, min, max, .. } if *key == param => Some((*min, *max)),
                _ => None,
            })
    }

    /// The kind with the numeric settings `values` (key and number) set, each brought into range
    /// (and to a whole number where it is one); `None` if a key is not a number of this kind.
    pub fn with_numbers(&self, values: &[(String, f32)]) -> Option<Self> {
        let mut map = self.to_map();
        for (key, value) in values {
            *map.get_mut(key)? = Value::from(f64::from(*value));
        }
        Self::from_tidy_map(map).ok()
    }

    /// Checks the settings are in range, as [`Self::tidy`] would bring them.
    pub(crate) fn validate(&self) -> Result<()> {
        if self.clone().tidy()? == *self {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!(
                "a {} effect has a setting out of range",
                self.name()
            )))
        }
    }
}

/// What an effect is added to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectTarget {
    /// The clip with this id.
    Clip(String),
    /// The layer with this id.
    Layer(String),
    /// The group with this id: its effects apply to the picture the group's layers make together.
    Group(String),
    /// The composition: the final mix, before the output mapping.
    Composition,
}

/// Checks one stack: at most [`MAX_EFFECTS`], each valid and with an id no other effect has
/// (`seen` holds those met so far). The kinds that are for the composition only are refused unless
/// this is its stack (`composition`).
pub(crate) fn validate_stack<'a>(
    stack: &'a [Effect],
    composition: bool,
    seen: &mut std::collections::HashSet<&'a str>,
) -> Result<()> {
    if stack.len() > MAX_EFFECTS {
        return Err(ProjectError::Invalid(format!(
            "a stack has at most {MAX_EFFECTS} effects"
        )));
    }
    for effect in stack {
        if !seen.insert(&effect.id) {
            return Err(ProjectError::Invalid(format!(
                "duplicate effect id: {}",
                effect.id
            )));
        }
        if effect.kind.composition_only() && !composition {
            return Err(ProjectError::Invalid(format!(
                "a {} effect only works on the composition",
                effect.kind.name()
            )));
        }
        effect.kind.validate()?;
        for (param, modulation) in &effect.modulation {
            let (lo, hi) = effect.kind.modulatable_range(param)?;
            modulation.validate(lo, hi)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_should_start_with_its_defaults_and_be_valid() {
        let names: Vec<&str> = EffectKind::names().collect();
        assert_eq!(names.len(), 39);
        for name in &names {
            let kind = EffectKind::default_of(name).unwrap();
            assert_eq!(kind.name(), *name);
            kind.validate().unwrap();
            // The enum carries exactly the settings the catalog describes.
            let map = kind.to_map();
            let spec = spec_of(name).unwrap();
            assert_eq!(map.len(), spec.params.len() + 1, "{name}");
            for param in &spec.params {
                assert!(map.contains_key(param.key()), "{name}: {}", param.key());
            }
            assert!(CATEGORIES.contains(&spec.category), "{name}");
        }
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(EffectKind::default_of("sparkle").is_err());
    }

    #[test]
    fn the_catalog_defaults_should_lie_within_their_ranges() {
        for spec in catalog() {
            let paced = spec.params.iter().any(|p| p.key() == "sync");
            assert_eq!(paced, spec.animated, "{}", spec.name);
            for param in &spec.params {
                if let ParamSpec::Number {
                    min,
                    max,
                    default,
                    snap,
                    key,
                    ..
                } = param
                {
                    assert!(min <= default && default <= max, "{}.{key}", spec.name);
                    assert!(
                        snap.is_empty() || snap.contains(default),
                        "{}.{key}",
                        spec.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_setting_should_be_brought_into_range_and_a_wrong_one_refused() {
        let clamped = EffectKind::Zoom { amount: 50.0 }.tidy().unwrap();
        assert_eq!(clamped, EffectKind::Zoom { amount: MAX_ZOOM });
        assert!(EffectKind::Zoom { amount: f32::NAN }.tidy().is_err());
        let bad = EffectKind::Tint {
            colour: "orange".into(),
            amount: 0.5,
        };
        assert!(bad.tidy().is_err());
        // Whole numbers where the setting counts things.
        let kaleidoscope = EffectKind::Kaleidoscope {
            segments: 99.6,
            rotation: 0.0,
            spin: 0.0,
            x: 0.5,
            y: 0.5,
            pace: Pace {
                sync: Sync::Free,
                rate: 1.0,
                beats: 1.0,
            },
        };
        let EffectKind::Kaleidoscope { segments, .. } = kaleidoscope.tidy().unwrap() else {
            unreachable!()
        };
        assert_eq!(segments, 32.0);
        let low = EffectKind::Posterize { levels: 0.2 };
        assert_eq!(low.tidy().unwrap(), EffectKind::Posterize { levels: 2.0 });
    }

    #[test]
    fn a_beat_cycle_should_snap_to_a_length_the_tempo_divides() {
        let EffectKind::Strobe { mut pace, .. } = EffectKind::default_of("strobe").unwrap() else {
            unreachable!()
        };
        pace.beats = 3.1;
        let strobe = EffectKind::Strobe {
            colour: "#ffffff".into(),
            duty: 0.5,
            mode: StrobeMode::Flash,
            pace,
        };
        let EffectKind::Strobe { pace, .. } = strobe.tidy().unwrap() else {
            unreachable!()
        };
        assert_eq!(pace.beats, 4.0);
    }

    #[test]
    fn a_pace_should_sit_flat_in_the_effect_in_the_file() {
        let wave = EffectKind::default_of("wave").unwrap();
        let json = serde_json::to_string(&wave).unwrap();
        assert!(
            json.contains(r#""sync":"free""#) && json.contains(r#""rate":0.5"#),
            "{json}"
        );
        assert!(!json.contains("pace"), "{json}");
        let back: EffectKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, wave);
    }

    #[test]
    fn the_kind_of_an_effect_should_be_told_from_its_settings() {
        let a = EffectKind::default_of("twirl").unwrap();
        let b = EffectKind::Twirl {
            angle: 1.0,
            radius: 1.0,
            x: 0.1,
            y: 0.2,
        };
        assert!(a.same_kind(&b));
        assert!(!a.same_kind(&EffectKind::default_of("bulge").unwrap()));
    }

    /// Every word the interface draws from the catalogue is in both catalogues of messages.
    #[test]
    fn every_effect_should_have_its_words_in_english_and_in_spanish() {
        let catalogues = [
            ("en", include_str!("../../../../src/i18n/en.ts")),
            ("es", include_str!("../../../../src/i18n/es.ts")),
        ];
        for (language, text) in catalogues {
            let has = |key: &str| text.contains(&format!("\n  \"{key}\": "));
            for category in CATEGORIES {
                assert!(
                    has(&format!("vj.effect.cat.{category}")),
                    "{language}: {category}"
                );
            }
            for spec in catalog() {
                let name = spec.name;
                assert!(has(&format!("vj.effect.{name}")), "{language}: {name}");
                assert!(
                    has(&format!("vj.effect.{name}.hint")),
                    "{language}: {name} hint"
                );
                let mut keys = spec.params.iter().peekable();
                while let Some(param) = keys.next() {
                    let key = param.key();
                    // A pair of centre or count settings is drawn as one row with one label.
                    let key = match (key, keys.peek().map(|p| p.key())) {
                        ("x", Some("y")) => {
                            keys.next();
                            "xy".to_owned()
                        }
                        ("columns", Some("rows")) => {
                            keys.next();
                            "columnsrows".to_owned()
                        }
                        _ => key.to_owned(),
                    };
                    assert!(
                        has(&format!("vj.effect.{name}.{key}"))
                            || has(&format!("vj.effect.param.{key}")),
                        "{language}: {name}.{key}"
                    );
                    if let ParamSpec::Choice { key, options, .. } = param {
                        for option in options {
                            assert!(
                                has(&format!("vj.effect.param.{key}.{option}")),
                                "{language}: {key}.{option}"
                            );
                        }
                    }
                }
            }
        }
    }
}
