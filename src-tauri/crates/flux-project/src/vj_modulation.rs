//! Parameter modulation of the VJ grid, as in Resolume: a numeric setting is driven automatically,
//! between a minimum and a maximum, by an LFO (synced to the tempo or free in Hz) or by the sound
//! (the low, mid or high band, or the level).
//!
//! A [`Modulation`] is stored with the thing it moves: an effect (any numeric setting of its kind),
//! a layer (`master`, `opacity`, `volume`) or a clip (`scale`, `positionX`, `positionY`, and the
//! numbers of its generator as `generator.size`, `generator.rotation`...). It never
//! changes the stored value: the screens and the sound ask [`modulate`] for a copy of the grid
//! with every modulated value in place of the stored one, and the stored grid stays as it was.
//!
//! The value is `min + (max - min) * s`, `s` from 0 to 1. `min` may be above `max`, which inverts
//! the movement. An LFO runs on the composition's clock, in seconds or in beats, never on a clip's
//! own time, so every LFO of the grid stays in phase whatever clips are launched when. Evaluating
//! is pure: the one state is the smoothing of an audio source ([`Modulator`]).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{PI, TAU};

use serde::{Deserialize, Serialize};

use crate::vj::{Clip, MAX_OFFSET, MAX_SCALE, Vj};
use crate::vj_effects::{BEAT_CYCLES, Effect, Pace, Sync};
use crate::{ProjectError, Result};

/// The largest gain of an audio source: its band is multiplied by it before it is cut at 1.
pub const MAX_GAIN: f32 = 8.0;
/// The longest attack or release of an audio source, in seconds.
pub const MAX_SMOOTHING: f32 = 5.0;
/// The most cycles a second a free LFO runs at.
pub const MAX_RATE: f32 = 30.0;

/// The settings of a layer that can be modulated, with their range.
pub const LAYER_FIELDS: [&str; 3] = ["master", "opacity", "volume"];
/// The settings of a clip that can be modulated.
pub const CLIP_FIELDS: [&str; 3] = ["scale", "positionX", "positionY"];

/// The wave an LFO draws over one cycle, 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    /// Starts at 0, peaks half way.
    Sine,
    Triangle,
    /// Rises from 0 to 1, then drops back.
    SawUp,
    /// Falls from 1 to 0, then jumps back.
    SawDown,
    /// 1 for the first half of the cycle, 0 for the second.
    Square,
    /// A new level each cycle, held.
    Random,
    /// A new level each cycle, reached by a smooth ramp from the last.
    SmoothRandom,
}

/// Which part of the sound an audio source follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Band {
    /// Below 250 Hz.
    Low,
    /// 250 Hz to 4 kHz.
    Mid,
    /// Above 4 kHz.
    High,
    /// The whole sound.
    Level,
}

/// What moves the value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Source {
    Lfo {
        shape: Shape,
        /// Cycles a second when free, or one cycle every so many beats when synced.
        pace: Pace,
        /// Where in its cycle the wave starts, 0 to 1.
        phase: f32,
    },
    Audio {
        band: Band,
        /// The band's level times this, cut at 1; 0 to [`MAX_GAIN`], 1 leaves it as it is.
        gain: f32,
        /// Seconds the value takes to follow the sound up, and down.
        attack: f32,
        release: f32,
    },
}

/// A numeric setting driven by a [`Source`] between `min` and `max`, in the setting's own units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Modulation {
    pub source: Source,
    pub min: f32,
    pub max: f32,
}

fn finite(value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ProjectError::Invalid(
            "a modulation is made of numbers".into(),
        ))
    }
}

impl Modulation {
    /// The modulation with everything brought into range: `min` and `max` into `lo..=hi` (the
    /// setting's range), the beats of a synced LFO to the nearest length it can have. Anything not
    /// a number is refused.
    pub fn tidy(self, lo: f32, hi: f32) -> Result<Self> {
        let source = match self.source {
            Source::Lfo { shape, pace, phase } => {
                let beats = finite(pace.beats)?;
                Source::Lfo {
                    shape,
                    pace: Pace {
                        sync: pace.sync,
                        rate: finite(pace.rate)?.clamp(0.0, MAX_RATE),
                        beats: BEAT_CYCLES
                            .iter()
                            .copied()
                            .min_by(|a, b| (a - beats).abs().total_cmp(&(b - beats).abs()))
                            .unwrap_or(1.0),
                    },
                    phase: finite(phase)?.clamp(0.0, 1.0),
                }
            }
            Source::Audio {
                band,
                gain,
                attack,
                release,
            } => Source::Audio {
                band,
                gain: finite(gain)?.clamp(0.0, MAX_GAIN),
                attack: finite(attack)?.clamp(0.0, MAX_SMOOTHING),
                release: finite(release)?.clamp(0.0, MAX_SMOOTHING),
            },
        };
        Ok(Self {
            source,
            min: finite(self.min)?.clamp(lo, hi),
            max: finite(self.max)?.clamp(lo, hi),
        })
    }

    /// Checks the modulation is as [`Self::tidy`] would leave it.
    pub fn validate(&self, lo: f32, hi: f32) -> Result<()> {
        if (*self).tidy(lo, hi)? == *self {
            Ok(())
        } else {
            Err(ProjectError::Invalid(
                "a modulation has a setting out of range".into(),
            ))
        }
    }

    /// The value for `s`, 0 to 1 of the way from `min` to `max`.
    pub fn at(&self, s: f32) -> f32 {
        self.min + (self.max - self.min) * s
    }
}

/// What a modulation is set on: a numeric setting of an effect, or a field of a layer or a clip.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ModTarget {
    Effect {
        effect: String,
        param: String,
    },
    Layer {
        layer: String,
        field: String,
    },
    Clip {
        clip: String,
        field: String,
    },
    /// A knob of a layer's dashboard, 0 to 1.
    LayerKnob {
        layer: String,
        knob: usize,
    },
    /// A knob of a clip's dashboard, 0 to 1.
    ClipKnob {
        clip: String,
        knob: usize,
    },
    /// A knob of the composition dashboard, 0 to 1. Last: it would also read the two above.
    Knob {
        knob: usize,
    },
}

/// The range of a layer's modulatable field.
pub fn layer_range(field: &str) -> Option<(f32, f32)> {
    LAYER_FIELDS.contains(&field).then_some((0.0, 1.0))
}

/// The prefix of the fields of a clip that move a number of its generator.
pub const GENERATOR_FIELD: &str = "generator.";

/// The range of the modulatable field `field` of `clip`: its transform, or a number its generator
/// has.
pub fn clip_field_range(clip: &Clip, field: &str) -> Option<(f32, f32)> {
    match field.strip_prefix(GENERATOR_FIELD) {
        Some(name) => clip
            .generator
            .as_ref()
            .filter(|generator| generator.numbers().contains(&name))
            .and_then(|_| crate::generator::number_range(name)),
        None => clip_range(field),
    }
}

/// The range of a clip's own modulatable field (its transform).
pub fn clip_range(field: &str) -> Option<(f32, f32)> {
    match field {
        "scale" => Some((0.0, MAX_SCALE)),
        "positionX" | "positionY" => Some((-MAX_OFFSET, MAX_OFFSET)),
        _ => None,
    }
}

/// Checks the modulations stored with one thing: each on a field it has, within its range.
pub(crate) fn validate_map(
    map: &BTreeMap<String, Modulation>,
    range: impl Fn(&str) -> Option<(f32, f32)>,
) -> Result<()> {
    for (field, modulation) in map {
        let (lo, hi) = range(field)
            .ok_or_else(|| ProjectError::Invalid(format!("{field} cannot be modulated")))?;
        modulation.validate(lo, hi)?;
    }
    Ok(())
}

/// Each number of a generator with the value it starts at and where a new modulation of it starts.
const GENERATOR_STARTS: [(&str, f32, (f32, f32)); 10] = [
    ("rate", 8.0, (1.0, 15.0)),
    ("width", 0.5, (0.1, 0.9)),
    ("angle", 0.0, (0.0, 360.0)),
    ("size", 0.12, (0.06, 0.2)),
    ("opacity", 1.0, (0.0, 1.0)),
    ("positionX", 0.5, (0.25, 0.75)),
    ("positionY", 0.5, (0.25, 0.75)),
    ("rotation", 0.0, (-30.0, 30.0)),
    ("tracking", 0.0, (0.0, 0.3)),
    ("leading", 0.0, (0.0, 0.5)),
];

/// What the inspector draws its modulation controls from, as the effect catalogue is: the shapes,
/// bands and limits, the modulation a new one starts as, and the range of each field of a layer
/// and a clip (and where a new modulation of it starts). The settings of effects are in the effect
/// catalogue.
pub fn catalog() -> serde_json::Value {
    let field = |(min, max): (f32, f32), default: f32, start: (f32, f32)| serde_json::json!({ "min": min, "max": max, "default": default, "start": [start.0, start.1] });
    let layer = |name: &str| field(layer_range(name).unwrap_or((0.0, 1.0)), 1.0, (0.0, 1.0));
    let clip = |name: &str, default: f32, start: (f32, f32)| {
        field(clip_range(name).unwrap_or((0.0, 1.0)), default, start)
    };
    serde_json::json!({
        "shapes": ["sine", "triangle", "sawUp", "sawDown", "square", "random", "smoothRandom"],
        "bands": ["low", "mid", "high", "level"],
        "maxGain": MAX_GAIN,
        "maxSmoothing": MAX_SMOOTHING,
        "maxRate": MAX_RATE,
        "beats": BEAT_CYCLES,
        "start": Modulation {
            source: Source::Lfo {
                shape: Shape::Sine,
                pace: Pace { sync: Sync::Beats, rate: 0.5, beats: 1.0 },
                phase: 0.0,
            },
            min: 0.0,
            max: 1.0,
        },
        "audio": Source::Audio { band: Band::Low, gain: 1.0, attack: 0.02, release: 0.25 },
        "layer": {
            "master": layer("master"),
            "opacity": layer("opacity"),
            "volume": layer("volume"),
        },
        "clip": {
            "scale": clip("scale", 1.0, (0.5, 2.0)),
            "positionX": clip("positionX", 0.0, (-0.5, 0.5)),
            "positionY": clip("positionY", 0.0, (-0.5, 0.5)),
        },
        "generator": GENERATOR_STARTS.iter().map(|&(name, default, start)| {
            let range = crate::generator::number_range(name).unwrap_or((0.0, 1.0));
            (name, field(range, default, start))
        }).collect::<BTreeMap<_, _>>(),
    })
}

// ---------------------------------------------------------------------------------------------
// Evaluation

/// What the composition's clock reads: the instant a modulation is evaluated at.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Clock {
    /// Seconds since the grid's clock started.
    pub seconds: f64,
    /// The beats it has counted, at the tempo each had.
    pub beats: f64,
}

/// How loud each band of the sound is, 0 to 1, as the analysis reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bands {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    pub level: f32,
}

impl Bands {
    pub fn get(&self, band: Band) -> f32 {
        match band {
            Band::Low => self.low,
            Band::Mid => self.mid,
            Band::High => self.high,
            Band::Level => self.level,
        }
    }
}

fn hash_cycle(seed: u64, cycle: i64) -> f32 {
    let mut z = seed ^ (cycle as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The wave at `cycles` cycles from its start (whole cycles included), 0 to 1. The random shapes
/// draw their level of each cycle from `seed` and the cycle's number, so they repeat exactly.
pub fn shape_value(shape: Shape, cycles: f64, seed: u64) -> f32 {
    let whole = cycles.floor();
    let p = (cycles - whole) as f32;
    let n = whole as i64;
    match shape {
        Shape::Sine => 0.5 - 0.5 * (TAU * p).cos(),
        Shape::Triangle => 1.0 - (2.0 * p - 1.0).abs(),
        Shape::SawUp => p,
        Shape::SawDown => 1.0 - p,
        Shape::Square => {
            if p < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        Shape::Random => hash_cycle(seed, n),
        Shape::SmoothRandom => {
            let (a, b) = (hash_cycle(seed, n), hash_cycle(seed, n + 1));
            a + (b - a) * (0.5 - 0.5 * (PI * p).cos())
        }
    }
}

/// How many cycles an LFO has run at `clock`: by the beats counted when synced, by the seconds
/// when free, plus its phase.
pub fn lfo_cycles(pace: Pace, phase: f32, clock: Clock) -> f64 {
    let run = match pace.sync {
        Sync::Beats => clock.beats / f64::from(pace.beats.max(f32::MIN_POSITIVE)),
        Sync::Free => clock.seconds * f64::from(pace.rate),
    };
    run + f64::from(phase)
}

/// FNV-1a: tells two modulations apart for the random shapes.
fn seed_of(key: &str) -> u64 {
    key.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// The smoothing of the audio sources, by what they move: where each is and when it was last read.
#[derive(Debug, Default)]
pub struct Modulator {
    followers: HashMap<String, (f32, f64)>,
    /// Evaluations since the followers were last pruned.
    calls: u32,
}

/// How many evaluations pass between two prunings of the followers.
const PRUNE_EVERY: u32 = 600;

impl Modulator {
    /// Drops the followers of what `vj` no longer has modulated; it only looks every
    /// [`PRUNE_EVERY`] calls and never allocates.
    fn prune(&mut self, vj: &Vj) {
        self.calls += 1;
        if self.calls < PRUNE_EVERY {
            return;
        }
        self.calls = 0;
        self.followers.retain(|key, _| vj.has_modulation_key(key));
    }

    /// The 0 to 1 movement of `modulation`, which is `key` (what it moves, unique in the grid).
    pub fn signal(
        &mut self,
        key: &str,
        modulation: &Modulation,
        clock: Clock,
        bands: Bands,
    ) -> f32 {
        match modulation.source {
            Source::Lfo { shape, pace, phase } => {
                shape_value(shape, lfo_cycles(pace, phase, clock), seed_of(key))
            }
            Source::Audio {
                band,
                gain,
                attack,
                release,
            } => {
                let target = (bands.get(band) * gain).clamp(0.0, 1.0);
                if !self.followers.contains_key(key) {
                    self.followers
                        .insert(key.to_owned(), (target, clock.seconds));
                }
                let (value, at) = self.followers.get_mut(key).expect("just inserted");
                // Several screens ask, each a little after the last: a follower that moves by the
                // time since it was read gives the same answer however often it is.
                let dt = (clock.seconds - *at).max(0.0);
                if dt > 0.0 {
                    let tau = if target > *value { attack } else { release };
                    *value = if tau <= 0.0 {
                        target
                    } else {
                        *value + (target - *value) * (1.0 - (-dt / f64::from(tau)).exp() as f32)
                    };
                    *at = clock.seconds;
                }
                *value
            }
        }
    }

    /// The value of `modulation`, in the units of what it moves.
    pub fn value(&mut self, key: &str, modulation: &Modulation, clock: Clock, bands: Bands) -> f32 {
        let s = self.signal(key, modulation, clock, bands);
        modulation.at(s)
    }
}

// ---------------------------------------------------------------------------------------------
// The grid with its modulated values

impl Vj {
    /// Whether anything of the grid is modulated.
    pub fn is_modulated(&self) -> bool {
        self.dashboard.iter().any(|k| k.modulation.is_some())
            || crate::vj_dashboard::scoped(self).any(|(_, _, k)| k.modulation.is_some())
            || self
                .composition_effects
                .iter()
                .any(|e| !e.modulation.is_empty())
            || self.layers.iter().any(|layer| {
                !layer.modulation.is_empty()
                    || layer.effects.iter().any(|e| !e.modulation.is_empty())
                    || layer.clips.iter().any(|clip| {
                        !clip.modulation.is_empty()
                            || clip.effects.iter().any(|e| !e.modulation.is_empty())
                    })
            })
    }

    /// Whether a clip lasts a number of beats, so that its sound follows the tempo.
    pub fn has_beat_clips(&self) -> bool {
        self.layers
            .iter()
            .flat_map(|layer| &layer.clips)
            .any(|clip| matches!(clip.tempo, crate::vj::Tempo::Beats { .. }))
    }

    /// Whether any modulation of the grid listens to the sound.
    pub fn has_audio_modulation(&self) -> bool {
        let audio = |m: &BTreeMap<String, Modulation>| {
            m.values().any(|m| matches!(m.source, Source::Audio { .. }))
        };
        let stack = |s: &[Effect]| s.iter().any(|e| audio(&e.modulation));
        self.dashboard.iter().any(|k| {
            k.modulation
                .is_some_and(|m| matches!(m.source, Source::Audio { .. }))
        }) || crate::vj_dashboard::scoped(self).any(|(_, _, k)| {
            k.modulation
                .is_some_and(|m| matches!(m.source, Source::Audio { .. }))
        }) || stack(&self.composition_effects)
            || self.layers.iter().any(|l| {
                audio(&l.modulation)
                    || stack(&l.effects)
                    || l.clips
                        .iter()
                        .any(|c| audio(&c.modulation) || stack(&c.effects))
            })
    }

    /// Whether `key` (as the modulator names what it moves) is a modulation of this grid.
    fn has_modulation_key(&self, key: &str) -> bool {
        let Some((head, field)) = key.rsplit_once(':') else {
            return false;
        };
        if let Some(knob) = head.strip_prefix("k:") {
            return knob.parse::<usize>().is_ok_and(|k| {
                self.dashboard
                    .get(k)
                    .is_some_and(|k| k.modulation.is_some())
            });
        }
        if crate::vj_dashboard::has_key(self, head) {
            return field == "value";
        }
        let has = |m: &BTreeMap<String, Modulation>| m.contains_key(field);
        let in_stack =
            |id: &str, stack: &[Effect]| stack.iter().any(|e| e.id == id && has(&e.modulation));
        if let Some(id) = head.strip_prefix("l:") {
            self.layers.iter().any(|l| l.id == id && has(&l.modulation))
        } else if let Some(id) = head.strip_prefix("c:") {
            self.layers
                .iter()
                .flat_map(|l| &l.clips)
                .any(|c| c.id == id && has(&c.modulation))
        } else if let Some(id) = head.strip_prefix("e:") {
            in_stack(id, &self.composition_effects)
                || self.layers.iter().any(|l| {
                    in_stack(id, &l.effects) || l.clips.iter().any(|c| in_stack(id, &c.effects))
                })
        } else {
            false
        }
    }

    /// Whether a layer's master or volume is modulated: the sound follows them.
    pub fn is_sound_modulated(&self) -> bool {
        self.layers.iter().any(|layer| {
            layer.modulation.contains_key("master") || layer.modulation.contains_key("volume")
        })
    }
}

fn effect_key(effect: &Effect, param: &str) -> String {
    format!("e:{}:{param}", effect.id)
}

/// Puts the modulated settings of `base` in `out`, its copy.
fn modulate_effect(
    base: &Effect,
    out: &mut Effect,
    modulator: &mut Modulator,
    clock: Clock,
    bands: Bands,
) {
    if base.modulation.is_empty() {
        return;
    }
    let values: Vec<(String, f32)> = base
        .modulation
        .iter()
        .map(|(param, m)| {
            (
                param.clone(),
                modulator.value(&effect_key(base, param), m, clock, bands),
            )
        })
        .collect();
    if let Some(kind) = base.kind.with_numbers(&values) {
        out.kind = kind;
    }
}

fn modulate_stack(
    base: &[Effect],
    out: &mut [Effect],
    modulator: &mut Modulator,
    clock: Clock,
    bands: Bands,
) {
    for (base, out) in base.iter().zip(out) {
        modulate_effect(base, out, modulator, clock, bands);
    }
}

/// `vj` with every modulated value in place of the stored one at `clock`; borrowed as it is when
/// nothing is modulated. The stored grid is never touched.
pub fn modulate<'a>(
    vj: &'a Vj,
    modulator: &mut Modulator,
    clock: Clock,
    bands: Bands,
) -> Cow<'a, Vj> {
    if !vj.is_modulated() {
        return Cow::Borrowed(vj);
    }
    let mut out = vj.clone();
    modulate_into(vj, &mut out, modulator, clock, bands);
    Cow::Owned(out)
}

/// Writes every modulated value of `vj` at `clock` into `drawn`, a copy of `vj` (same layers,
/// clips and effects), which can be kept between frames: only the modulated fields are written,
/// each from the stored value, so the rest stays as `vj` has it.
pub fn modulate_into(
    vj: &Vj,
    drawn: &mut Vj,
    modulator: &mut Modulator,
    clock: Clock,
    bands: Bands,
) {
    modulator.prune(vj);
    modulate_stack(
        &vj.composition_effects,
        &mut drawn.composition_effects,
        modulator,
        clock,
        bands,
    );
    for (layer, out) in vj.layers.iter().zip(&mut drawn.layers) {
        for (field, m) in &layer.modulation {
            let value = modulator.value(&format!("l:{}:{field}", layer.id), m, clock, bands);
            match field.as_str() {
                "master" => out.master = value.clamp(0.0, 1.0),
                "opacity" => out.opacity = value.clamp(0.0, 1.0),
                "volume" => out.volume = value.clamp(0.0, 1.0),
                _ => {}
            }
        }
        modulate_stack(&layer.effects, &mut out.effects, modulator, clock, bands);
        for (clip, out) in layer.clips.iter().zip(&mut out.clips) {
            for (field, m) in &clip.modulation {
                let value = modulator.value(&format!("c:{}:{field}", clip.id), m, clock, bands);
                match field.as_str() {
                    "scale" => out.scale = value.clamp(0.0, MAX_SCALE),
                    "positionX" => out.position[0] = value.clamp(-MAX_OFFSET, MAX_OFFSET),
                    "positionY" => out.position[1] = value.clamp(-MAX_OFFSET, MAX_OFFSET),
                    other => {
                        let number = other.strip_prefix(GENERATOR_FIELD);
                        if let (Some(name), Some(generator)) = (number, &mut out.generator) {
                            generator.set_modulated(name, value);
                        }
                    }
                }
            }
            modulate_stack(&clip.effects, &mut out.effects, modulator, clock, bands);
        }
    }
    // The knobs last: they move numbers of the drawn copy, which the stacks above rewrite whole.
    for (at, knob) in vj.dashboard.iter().enumerate() {
        if let Some(m) = &knob.modulation {
            let value = modulator.value(&format!("k:{at}:value"), m, clock, bands);
            crate::vj_mapping::drive_knob(drawn, at, value);
        }
    }
    for (scope, at, knob) in crate::vj_dashboard::scoped(vj) {
        if let Some(m) = &knob.modulation {
            let value = modulator.value(&crate::vj_dashboard::key(&scope, at), m, clock, bands);
            crate::vj_dashboard::drive(drawn, &scope, at, value);
        }
    }
}

/// The modulated values of a grid at an instant, for the inspector to mark on its controls: by
/// effect id and setting, by layer id and field, by clip id and field.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Values {
    pub effects: BTreeMap<String, BTreeMap<String, f32>>,
    pub layers: BTreeMap<String, BTreeMap<String, f32>>,
    pub clips: BTreeMap<String, BTreeMap<String, f32>>,
    /// The modulated knobs of the dashboard, by place.
    pub knobs: BTreeMap<usize, f32>,
    /// The modulated knobs of a layer's dashboard, by layer id and place.
    pub layer_knobs: BTreeMap<String, BTreeMap<usize, f32>>,
    /// The modulated knobs of a clip's dashboard, by clip id and place.
    pub clip_knobs: BTreeMap<String, BTreeMap<usize, f32>>,
}

/// Every modulated value of `vj` at `clock`.
pub fn values(vj: &Vj, modulator: &mut Modulator, clock: Clock, bands: Bands) -> Values {
    modulator.prune(vj);
    let mut out = Values::default();
    let mut effects = |stack: &[Effect], modulator: &mut Modulator| {
        for effect in stack.iter().filter(|e| !e.modulation.is_empty()) {
            let map = effect
                .modulation
                .iter()
                .map(|(param, m)| {
                    (
                        param.clone(),
                        modulator.value(&effect_key(effect, param), m, clock, bands),
                    )
                })
                .collect();
            out.effects.insert(effect.id.clone(), map);
        }
    };
    effects(&vj.composition_effects, modulator);
    for layer in &vj.layers {
        effects(&layer.effects, modulator);
        for clip in &layer.clips {
            effects(&clip.effects, modulator);
        }
    }
    for layer in &vj.layers {
        if !layer.modulation.is_empty() {
            let map = layer
                .modulation
                .iter()
                .map(|(field, m)| {
                    (
                        field.clone(),
                        modulator.value(&format!("l:{}:{field}", layer.id), m, clock, bands),
                    )
                })
                .collect();
            out.layers.insert(layer.id.clone(), map);
        }
        for clip in layer.clips.iter().filter(|c| !c.modulation.is_empty()) {
            let map = clip
                .modulation
                .iter()
                .map(|(field, m)| {
                    (
                        field.clone(),
                        modulator.value(&format!("c:{}:{field}", clip.id), m, clock, bands),
                    )
                })
                .collect();
            out.clips.insert(clip.id.clone(), map);
        }
    }
    for (at, knob) in vj.dashboard.iter().enumerate() {
        if let Some(m) = &knob.modulation {
            let value = modulator.value(&format!("k:{at}:value"), m, clock, bands);
            out.knobs.insert(at, value);
        }
    }
    for (scope, at, knob) in crate::vj_dashboard::scoped(vj) {
        let Some(m) = &knob.modulation else { continue };
        let value = modulator.value(&crate::vj_dashboard::key(&scope, at), m, clock, bands);
        let (map, id) = match scope {
            crate::vj_dashboard::Scope::Layer { layer } => (&mut out.layer_knobs, layer),
            crate::vj_dashboard::Scope::Clip { clip } => (&mut out.clip_knobs, clip),
            crate::vj_dashboard::Scope::Composition => continue,
        };
        map.entry(id).or_default().insert(at, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj_effects::EffectKind;

    fn free(rate: f32) -> Pace {
        Pace {
            sync: Sync::Free,
            rate,
            beats: 1.0,
        }
    }

    fn beats(beats: f32) -> Pace {
        Pace {
            sync: Sync::Beats,
            rate: 0.0,
            beats,
        }
    }

    fn lfo(shape: Shape, pace: Pace, phase: f32, min: f32, max: f32) -> Modulation {
        Modulation {
            source: Source::Lfo { shape, pace, phase },
            min,
            max,
        }
    }

    fn audio(band: Band, gain: f32, attack: f32, release: f32) -> Modulation {
        Modulation {
            source: Source::Audio {
                band,
                gain,
                attack,
                release,
            },
            min: 0.0,
            max: 1.0,
        }
    }

    fn at(seconds: f64, beats: f64) -> Clock {
        Clock { seconds, beats }
    }

    fn grid_with_clip() -> Vj {
        let mut next = 0;
        let mut vj = Vj::starting(|| {
            next += 1;
            format!("g{next}")
        });
        let column = vj.decks[0].columns[0].id.clone();
        let mut clip = crate::vj::new_clip("c".into(), "v".into(), None);
        clip.column = Some(column);
        vj.layers[0].clips.push(clip);
        vj
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} is not {b}");
    }

    #[test]
    fn each_shape_should_draw_its_wave_at_known_phases() {
        let at = |shape, p: f64| shape_value(shape, p, 1);
        for (p, sine) in [(0.0, 0.0), (0.25, 0.5), (0.5, 1.0), (0.75, 0.5)] {
            near(at(Shape::Sine, p), sine);
        }
        for (p, triangle) in [(0.0, 0.0), (0.25, 0.5), (0.5, 1.0), (0.75, 0.5)] {
            near(at(Shape::Triangle, p), triangle);
        }
        for p in [0.0, 0.25, 0.5, 0.75] {
            near(at(Shape::SawUp, p), p as f32);
            near(at(Shape::SawDown, p), 1.0 - p as f32);
        }
        assert_eq!(
            [0.0, 0.25, 0.49, 0.5, 0.75].map(|p| at(Shape::Square, p)),
            [1.0, 1.0, 1.0, 0.0, 0.0]
        );
        // Whole cycles repeat the wave.
        near(at(Shape::Sine, 3.25), 0.5);
        near(at(Shape::SawUp, 7.5), 0.5);
    }

    #[test]
    fn random_shapes_should_be_deterministic_and_vary_by_cycle_and_seed() {
        let r = |cycles, seed| shape_value(Shape::Random, cycles, seed);
        // Held through a cycle, the same every time, different from cycle to cycle and seed to seed.
        assert_eq!(r(4.1, 9), r(4.9, 9));
        assert_eq!(r(4.5, 9), r(4.5, 9));
        assert_ne!(r(4.5, 9), r(5.5, 9));
        assert_ne!(r(4.5, 9), r(4.5, 10));
        for cycle in 0..200 {
            assert!((0.0..1.0).contains(&r(f64::from(cycle), 3)));
        }
        // The smooth one is continuous across a cycle and meets the next cycle's level.
        let s = |cycles| shape_value(Shape::SmoothRandom, cycles, 9);
        near(s(4.0), r(4.0, 9));
        assert!((s(4.999_999) - r(5.0, 9)).abs() < 1e-3);
        assert!(s(4.5) >= r(4.0, 9).min(r(5.0, 9)) - 1e-6);
        assert!(s(4.5) <= r(4.0, 9).max(r(5.0, 9)) + 1e-6);
    }

    #[test]
    fn beats_and_hz_should_pace_an_lfo_on_the_composition_clock() {
        let mut modulator = Modulator::default();
        let bands = Bands::default();
        // A sine of 2 beats a cycle: a quarter of it after half a beat, whatever the seconds.
        let synced = lfo(Shape::SawUp, beats(2.0), 0.0, 0.0, 1.0);
        near(modulator.value("k", &synced, at(99.0, 0.5), bands), 0.25);
        near(modulator.value("k", &synced, at(0.0, 1.0), bands), 0.5);
        // A free one of 2 Hz: a quarter cycle after an eighth of a second, whatever the beats.
        let hz = lfo(Shape::SawUp, free(2.0), 0.0, 0.0, 1.0);
        near(modulator.value("k", &hz, at(0.125, 99.0), bands), 0.25);
        // Stands still at 0 Hz, on its phase.
        let still = lfo(Shape::SawUp, free(0.0), 0.4, 0.0, 1.0);
        near(modulator.value("k", &still, at(50.0, 0.0), bands), 0.4);
    }

    #[test]
    fn the_phase_should_move_where_the_wave_starts() {
        let mut modulator = Modulator::default();
        let bands = Bands::default();
        let plain = lfo(Shape::SawUp, beats(1.0), 0.0, 0.0, 1.0);
        let shifted = lfo(Shape::SawUp, beats(1.0), 0.25, 0.0, 1.0);
        near(modulator.value("k", &plain, at(0.0, 0.5), bands), 0.5);
        near(modulator.value("k", &shifted, at(0.0, 0.5), bands), 0.75);
        // It wraps.
        near(modulator.value("k", &shifted, at(0.0, 0.9), bands), 0.15);
    }

    #[test]
    fn the_value_should_run_from_min_to_max_and_invert_when_min_is_above() {
        let mut modulator = Modulator::default();
        let bands = Bands::default();
        let up = lfo(Shape::SawUp, beats(1.0), 0.0, 10.0, 20.0);
        near(modulator.value("k", &up, at(0.0, 0.0), bands), 10.0);
        near(modulator.value("k", &up, at(0.0, 0.5), bands), 15.0);
        let inverted = lfo(Shape::SawUp, beats(1.0), 0.0, 20.0, 10.0);
        near(modulator.value("k", &inverted, at(0.0, 0.0), bands), 20.0);
        near(modulator.value("k", &inverted, at(0.0, 0.25), bands), 17.5);
    }

    #[test]
    fn an_audio_source_should_follow_its_band_times_gain_cut_at_one() {
        let mut modulator = Modulator::default();
        let bands = Bands {
            low: 0.9,
            mid: 0.2,
            high: 0.0,
            level: 0.5,
        };
        let value = |modulator: &mut Modulator, m: Modulation| {
            modulator.value("a", &m, at(0.0, 0.0), bands)
        };
        near(value(&mut modulator, audio(Band::Low, 1.0, 0.0, 0.0)), 0.9);
        let mut fresh = Modulator::default();
        near(value(&mut fresh, audio(Band::Mid, 2.0, 0.0, 0.0)), 0.4);
        let mut fresh = Modulator::default();
        near(value(&mut fresh, audio(Band::Low, 8.0, 0.0, 0.0)), 1.0);
        let mut fresh = Modulator::default();
        near(value(&mut fresh, audio(Band::Level, 0.0, 0.0, 0.0)), 0.0);
        let mut fresh = Modulator::default();
        near(value(&mut fresh, audio(Band::Level, 1.0, 0.0, 0.0)), 0.5);
    }

    #[test]
    fn an_audio_source_should_smooth_with_its_attack_and_release() {
        let mut modulator = Modulator::default();
        let m = audio(Band::Level, 1.0, 1.0, 2.0);
        let quiet = Bands::default();
        let loud = Bands {
            level: 1.0,
            ..Bands::default()
        };
        near(modulator.value("a", &m, at(0.0, 0.0), quiet), 0.0);
        // One time constant of attack gets 63% of the way up.
        let up = modulator.value("a", &m, at(1.0, 0.0), loud);
        assert!((up - 0.632).abs() < 0.01, "{up}");
        // Asking again at the same instant, or a moment before, changes nothing.
        near(modulator.value("a", &m, at(1.0, 0.0), loud), up);
        near(modulator.value("a", &m, at(0.5, 0.0), loud), up);
        // Release is slower: two seconds of it fall 63% of the way back.
        let down = modulator.value("a", &m, at(3.0, 0.0), quiet);
        assert!((down - up * (1.0 - 0.632)).abs() < 0.01, "{down}");
    }

    #[test]
    fn tidy_should_bring_a_modulation_into_range_and_refuse_what_is_not_a_number() {
        let wild = Modulation {
            source: Source::Lfo {
                shape: Shape::Sine,
                pace: Pace {
                    sync: Sync::Beats,
                    rate: 99.0,
                    beats: 3.0,
                },
                phase: 4.0,
            },
            min: -5.0,
            max: 9.0,
        };
        let tidy = wild.tidy(0.0, 1.0).unwrap();
        assert_eq!((tidy.min, tidy.max), (0.0, 1.0));
        let Source::Lfo { pace, phase, .. } = tidy.source else {
            panic!("an lfo");
        };
        assert_eq!((pace.rate, pace.beats, phase), (MAX_RATE, 2.0, 1.0));
        assert!(wild.validate(0.0, 1.0).is_err());
        tidy.validate(0.0, 1.0).unwrap();
        // min may be above max.
        lfo(Shape::Sine, free(1.0), 0.0, 1.0, 0.0)
            .validate(0.0, 1.0)
            .unwrap();
        let mut nan = audio(Band::Low, 1.0, 0.0, 0.0);
        nan.min = f32::NAN;
        assert!(nan.tidy(0.0, 1.0).is_err());
        let mut nan_beats = lfo(Shape::Sine, beats(1.0), 0.0, 0.0, 1.0);
        if let Source::Lfo { pace, .. } = &mut nan_beats.source {
            pace.beats = f32::NAN;
        }
        assert!(nan_beats.tidy(0.0, 1.0).is_err());
        let gain = audio(Band::Low, 99.0, 99.0, -1.0).tidy(0.0, 1.0).unwrap();
        assert_eq!(
            gain.source,
            Source::Audio {
                band: Band::Low,
                gain: MAX_GAIN,
                attack: MAX_SMOOTHING,
                release: 0.0
            }
        );
    }

    #[test]
    fn a_modulation_should_round_trip_through_json() {
        let sources = [
            lfo(Shape::SmoothRandom, beats(0.25), 0.5, -1.0, 1.0),
            audio(Band::High, 2.5, 0.05, 0.3),
        ];
        for m in sources {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<Modulation>(&json).unwrap(), m);
        }
        let json = serde_json::to_value(sources[0]).unwrap();
        assert_eq!(json["source"]["kind"], "lfo");
        assert_eq!(json["source"]["shape"], "smoothRandom");
        assert_eq!(json["source"]["pace"]["sync"], "beats");
    }

    #[test]
    fn modulating_should_put_values_in_a_copy_and_leave_the_stored_grid_alone() {
        let mut vj = grid_with_clip();
        let sine = |min, max| lfo(Shape::SawUp, beats(1.0), 0.0, min, max);
        vj.layers[0]
            .modulation
            .insert("opacity".into(), sine(0.0, 1.0));
        vj.layers[0].clips[0]
            .modulation
            .insert("scale".into(), sine(1.0, 3.0));
        vj.layers[0].clips[0].effects.push(Effect {
            id: "fx".into(),
            bypass: false,
            kind: EffectKind::default_of("blur").unwrap(),
            modulation: BTreeMap::from([("radius".to_owned(), sine(0.0, 1.0))]),
        });
        let before = vj.clone();
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        assert_eq!(vj, before, "the stored grid is untouched");
        let layer = &out.layers[0];
        near(layer.opacity, 0.5);
        near(layer.clips[0].scale, 2.0);
        let EffectKind::Blur { radius } = layer.clips[0].effects[0].kind else {
            panic!("a blur");
        };
        near(radius, 0.5);
        // The stored values are still the ones the user set.
        near(vj.layers[0].opacity, 1.0);
        near(vj.layers[0].clips[0].scale, 1.0);
    }

    fn text_clip_grid() -> Vj {
        let mut vj = grid_with_clip();
        vj.layers[0].clips[0].asset.clear();
        vj.layers[0].clips[0].generator =
            Some(serde_json::from_str(r#"{"kind":"text","text":"FLUX"}"#).unwrap());
        vj
    }

    fn text_number(vj: &Vj, name: &str) -> f32 {
        let json = serde_json::to_value(vj.layers[0].clips[0].generator.as_ref().unwrap()).unwrap();
        json[name].as_f64().unwrap() as f32
    }

    #[test]
    fn a_generators_numbers_should_be_modulated_in_the_copy_and_kept_in_their_range() {
        let mut vj = text_clip_grid();
        let ramp = |min, max| lfo(Shape::SawUp, beats(1.0), 0.0, min, max);
        let clip = &mut vj.layers[0].clips[0];
        clip.modulation
            .insert("generator.size".into(), ramp(0.1, 0.3));
        clip.modulation
            .insert("generator.rotation".into(), ramp(-90.0, 90.0));
        // Past the range of the setting, as a file could carry it.
        clip.modulation
            .insert("generator.opacity".into(), ramp(0.0, 5.0));
        let before = vj.clone();
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        assert_eq!(vj, before, "the stored grid is untouched");
        near(text_number(&out, "size"), 0.2);
        near(text_number(&out, "rotation"), 0.0);
        near(text_number(&out, "opacity"), 1.0);
        assert_eq!(text_number(&vj, "size"), 0.12, "the stored size stays");
        let values = values(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        near(values.clips["c"]["generator.size"], 0.2);
    }

    #[test]
    fn a_modulated_generator_number_should_be_rounded_so_a_small_move_keeps_the_picture() {
        let mut vj = text_clip_grid();
        vj.layers[0].clips[0].modulation.insert(
            "generator.size".into(),
            lfo(Shape::SawUp, beats(1.0), 0.0, 0.1, 0.3),
        );
        let mut modulator = Modulator::default();
        let size = |beats: f64, modulator: &mut Modulator| {
            let out = modulate(&vj, modulator, at(0.0, beats), Bands::default());
            text_number(&out, "size")
        };
        // 0.0001 of a beat moves the value by 0.00002: less than its step of 0.001.
        assert_eq!(size(0.5, &mut modulator), size(0.5001, &mut modulator));
        assert_ne!(size(0.5, &mut modulator), size(0.6, &mut modulator));
    }

    #[test]
    fn the_range_of_a_generator_field_should_be_the_generators_and_only_for_its_numbers() {
        let vj = text_clip_grid();
        let clip = &vj.layers[0].clips[0];
        assert_eq!(clip_field_range(clip, "generator.size"), Some((0.01, 2.0)));
        assert_eq!(
            clip_field_range(clip, "generator.rate"),
            None,
            "not a text block's"
        );
        assert_eq!(clip_field_range(clip, "generator.colour"), None);
        assert_eq!(clip_field_range(clip, "scale"), Some((0.0, MAX_SCALE)));
        let video = &grid_with_clip().layers[0].clips[0];
        assert_eq!(
            clip_field_range(video, "generator.size"),
            None,
            "a video has none"
        );
    }

    #[test]
    fn a_grid_without_modulation_should_be_borrowed() {
        let vj = grid_with_clip();
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.0), Bands::default());
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn modulated_whole_numbers_should_be_rounded_and_ranges_kept() {
        let mut vj = grid_with_clip();
        let ramp = lfo(Shape::SawUp, beats(1.0), 0.0, 2.0, 32.0);
        vj.layers[0].effects.push(Effect {
            id: "k".into(),
            bypass: false,
            kind: EffectKind::default_of("kaleidoscope").unwrap(),
            modulation: BTreeMap::from([("segments".to_owned(), ramp)]),
        });
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        let EffectKind::Kaleidoscope { segments, .. } = out.layers[0].effects[0].kind else {
            panic!("a kaleidoscope");
        };
        assert_eq!(segments, 17.0);
        let list = values(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        near(list.effects["k"]["segments"], 17.0);
    }

    #[test]
    fn modulating_into_a_kept_copy_should_equal_modulating_each_frame() {
        let mut vj = grid_with_clip();
        let ramp = lfo(Shape::SawUp, beats(1.0), 0.0, 0.0, 1.0);
        vj.layers[0].modulation.insert("opacity".into(), ramp);
        vj.layers[0].clips[0]
            .modulation
            .insert("scale".into(), ramp);
        vj.composition_effects.push(Effect {
            id: "f".into(),
            bypass: false,
            kind: EffectKind::default_of("feedback").unwrap(),
            modulation: BTreeMap::from([("amount".to_owned(), ramp)]),
        });
        let (mut a, mut b) = (Modulator::default(), Modulator::default());
        let mut kept = vj.clone();
        for beat in [0.1, 0.4, 0.9, 1.3] {
            let whole = modulate(&vj, &mut a, at(0.0, beat), Bands::default());
            modulate_into(&vj, &mut kept, &mut b, at(0.0, beat), Bands::default());
            assert_eq!(*whole, kept);
        }
    }

    #[test]
    fn followers_of_removed_modulations_should_be_pruned() {
        let mut vj = grid_with_clip();
        let m = audio(Band::Low, 1.0, 0.1, 0.1);
        vj.layers[0].modulation.insert("opacity".into(), m);
        let key = format!("l:{}:opacity", vj.layers[0].id);
        let mut modulator = Modulator::default();
        modulator.signal(
            &key,
            &vj.layers[0].modulation["opacity"],
            at(0.0, 0.0),
            Bands::default(),
        );
        assert!(vj.has_audio_modulation());
        assert!(vj.has_modulation_key(&key));
        vj.layers[0].modulation.clear();
        assert!(!vj.has_modulation_key(&key));
        modulator.calls = PRUNE_EVERY;
        modulator.prune(&vj);
        assert!(modulator.followers.is_empty());
    }

    #[test]
    fn the_effects_of_the_composition_should_be_modulated_too() {
        let mut vj = grid_with_clip();
        vj.composition_effects.push(Effect {
            id: "f".into(),
            bypass: false,
            kind: EffectKind::default_of("feedback").unwrap(),
            modulation: BTreeMap::from([(
                "amount".to_owned(),
                lfo(Shape::SawUp, beats(1.0), 0.0, 0.0, 1.0),
            )]),
        });
        assert!(vj.is_modulated());
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        let EffectKind::Feedback { amount, .. } = out.composition_effects[0].kind else {
            panic!("a feedback");
        };
        near(amount, 0.5);
        let list = values(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        near(list.effects["f"]["amount"], 0.5);
    }

    #[test]
    fn the_catalog_should_describe_what_the_ops_accept() {
        let catalog = catalog();
        for field in LAYER_FIELDS {
            let spec = &catalog["layer"][field];
            let range = layer_range(field).unwrap();
            assert_eq!(spec["min"], range.0);
            assert_eq!(spec["max"], range.1);
        }
        for (name, ..) in crate::generator::NUMBERS {
            let spec = &catalog["generator"][name];
            let (lo, hi) = crate::generator::number_range(name).unwrap();
            assert_eq!(spec["min"].as_f64(), Some(f64::from(lo)), "{name}");
            assert_eq!(spec["max"].as_f64(), Some(f64::from(hi)), "{name}");
        }
        for field in CLIP_FIELDS {
            let spec = &catalog["clip"][field];
            let (lo, hi) = clip_range(field).unwrap();
            assert_eq!(
                (spec["min"].as_f64(), spec["max"].as_f64()),
                (Some(f64::from(lo)), Some(f64::from(hi)))
            );
            let start = spec["start"].as_array().unwrap();
            assert!(
                start
                    .iter()
                    .all(|v| (f64::from(lo)..=f64::from(hi)).contains(&v.as_f64().unwrap()))
            );
        }
        // What a new modulation starts as is one the ops take.
        let start: Modulation = serde_json::from_value(catalog["start"].clone()).unwrap();
        start.validate(0.0, 1.0).unwrap();
        let audio: Source = serde_json::from_value(catalog["audio"].clone()).unwrap();
        Modulation {
            source: audio,
            min: 0.0,
            max: 1.0,
        }
        .validate(0.0, 1.0)
        .unwrap();
        assert_eq!(catalog["shapes"].as_array().unwrap().len(), 7);
        // The effect catalogue marks which settings can be modulated.
        let spec = crate::vj_effects::spec_of("rotate").unwrap();
        let flags: Vec<(&str, bool)> = spec
            .params
            .iter()
            .filter_map(|p| match p {
                crate::vj_effects::ParamSpec::Number {
                    key, modulatable, ..
                } => Some((*key, *modulatable)),
                _ => None,
            })
            .collect();
        assert!(
            flags.contains(&("angle", true))
                && flags.contains(&("rate", false))
                && flags.contains(&("beats", false))
        );
    }

    /// A grid whose knob 2 drives the opacity of layer 0 from 0.2 to 0.8, and is modulated by a
    /// saw over `min..max`.
    fn modulated_knob(min: f32, max: f32) -> Vj {
        let mut vj = grid_with_clip();
        let layer = vj.layers[0].id.clone();
        crate::vj_mapping::link_knob(
            &mut vj,
            2,
            crate::vj_mapping::Target::LayerOpacity { layer },
            Some(0.2),
            Some(0.8),
        )
        .unwrap();
        let m = lfo(Shape::SawUp, beats(1.0), 0.0, min, max);
        crate::vj_dashboard::modulate(&mut vj, &Default::default(), 2, Some(m)).unwrap();
        vj
    }

    #[test]
    fn a_modulated_knob_should_move_what_it_drives_in_the_drawn_grid_only() {
        let vj = modulated_knob(0.0, 1.0);
        assert!(vj.is_modulated());
        let mut modulator = Modulator::default();
        let out = modulate(&vj, &mut modulator, at(0.0, 0.5), Bands::default());
        near(out.dashboard[2].value, 0.5);
        near(out.layers[0].opacity, 0.5);
        assert_eq!(vj.dashboard[2].value, 0.0, "the stored knob is as it was");
        assert_eq!(vj.layers[0].opacity, 1.0, "and so is what it drives");
    }

    #[test]
    fn a_knob_modulation_should_be_kept_in_range_stored_and_shown_as_a_value() {
        let mut vj = modulated_knob(-3.0, 4.0);
        let m = vj.dashboard[2].modulation.unwrap();
        assert_eq!((m.min, m.max), (0.0, 1.0));
        let json = serde_json::to_string(&vj).unwrap();
        assert_eq!(serde_json::from_str::<Vj>(&json).unwrap(), vj);
        let mut modulator = Modulator::default();
        let shown = values(&vj, &mut modulator, at(0.0, 0.25), Bands::default());
        near(shown.knobs[&2], 0.25);
        crate::vj_dashboard::modulate(&mut vj, &Default::default(), 2, None).unwrap();
        assert!(!vj.is_modulated());
    }

    #[test]
    fn a_knob_should_follow_the_sound_and_be_known_to_the_pruning() {
        let mut vj = modulated_knob(0.0, 1.0);
        crate::vj_dashboard::modulate(
            &mut vj,
            &Default::default(),
            2,
            Some(audio(Band::Low, 1.0, 0.0, 0.0)),
        )
        .unwrap();
        assert!(vj.has_audio_modulation());
        assert!(vj.has_modulation_key("k:2:value"));
        assert!(!vj.has_modulation_key("k:3:value"));
        let bands = Bands {
            low: 0.5,
            ..Bands::default()
        };
        let out = modulate(&vj, &mut Modulator::default(), at(0.0, 0.0), bands);
        near(out.layers[0].opacity, 0.5);
    }

    #[test]
    fn a_knob_target_should_read_from_the_json_the_window_sends() {
        let target: ModTarget = serde_json::from_str(r#"{"knob":7}"#).unwrap();
        assert_eq!(target, ModTarget::Knob { knob: 7 });
    }

    #[test]
    fn a_layer_and_a_clip_knob_should_be_modulated_shown_and_known_to_the_pruning() {
        use crate::vj_dashboard::Scope;
        use crate::vj_mapping::Target;
        let mut vj = grid_with_clip();
        let layer = vj.layers[0].id.clone();
        let on_layer = Scope::Layer {
            layer: layer.clone(),
        };
        let on_clip = Scope::Clip { clip: "c".into() };
        crate::vj_dashboard::link(
            &mut vj,
            &on_layer,
            1,
            Target::LayerOpacity {
                layer: layer.clone(),
            },
            Some(0.2),
            Some(0.8),
        )
        .unwrap();
        crate::vj_dashboard::link(
            &mut vj,
            &on_clip,
            0,
            Target::ClipSpeed { clip: "c".into() },
            Some(1.0),
            Some(2.0),
        )
        .unwrap();
        let saw = lfo(Shape::SawUp, beats(1.0), 0.0, 0.0, 1.0);
        crate::vj_dashboard::modulate(&mut vj, &on_layer, 1, Some(saw)).unwrap();
        crate::vj_dashboard::modulate(&mut vj, &on_clip, 0, Some(saw)).unwrap();
        assert!(vj.is_modulated());
        assert!(vj.has_modulation_key(&format!("lk:{layer}:1:value")));
        assert!(vj.has_modulation_key("ck:c:0:value"));
        assert!(!vj.has_modulation_key("ck:c:1:value"));
        let out = modulate(
            &vj,
            &mut Modulator::default(),
            at(0.0, 0.5),
            Bands::default(),
        );
        near(out.layers[0].opacity, 0.5);
        near(out.layers[0].clips[0].speed, 1.5);
        assert_eq!(vj.layers[0].opacity, 1.0, "the stored grid is as it was");
        let shown = values(
            &vj,
            &mut Modulator::default(),
            at(0.0, 0.25),
            Bands::default(),
        );
        near(shown.layer_knobs[&layer][&1], 0.25);
        near(shown.clip_knobs["c"][&0], 0.25);
        let target: ModTarget = serde_json::from_str(r#"{"clip":"c","knob":3}"#).unwrap();
        assert_eq!(
            target,
            ModTarget::ClipKnob {
                clip: "c".into(),
                knob: 3
            }
        );
    }
}
