//! How a setting of an effect is described: its range, its default, its words.

use serde::Serialize;
use serde_json::Value;

use super::*;
use crate::generator::rgb;
use crate::{ProjectError, Result};

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
    pub(super) fn key(&self) -> &'static str {
        match self {
            Self::Number { key, .. }
            | Self::Choice { key, .. }
            | Self::Colour { key, .. }
            | Self::Flag { key, .. } => key,
        }
    }

    pub(super) fn default_value(&self) -> Value {
        match self {
            Self::Number { default, .. } => Value::from(f64::from(*default)),
            Self::Choice { default, .. } | Self::Colour { default, .. } => Value::from(*default),
            Self::Flag { default, .. } => Value::from(*default),
        }
    }

    /// Brings `value` into range in place; a wrong type or a bad colour is refused.
    pub(super) fn tidy(&self, value: &mut Value) -> Result<()> {
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

pub(super) fn num(key: &'static str, min: f32, max: f32, step: f32, default: f32) -> ParamSpec {
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

pub(super) fn whole(key: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
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

pub(super) fn choice(
    key: &'static str,
    options: &[&'static str],
    default: &'static str,
) -> ParamSpec {
    ParamSpec::Choice {
        key,
        options: options.to_vec(),
        default,
    }
}

pub(super) fn colour(key: &'static str, default: &'static str) -> ParamSpec {
    ParamSpec::Colour { key, default }
}

pub(super) fn flag(key: &'static str, default: bool) -> ParamSpec {
    ParamSpec::Flag { key, default }
}

/// The three settings of a [`Pace`], starting at `rate` cycles a second or a cycle of `beats`.
pub(super) fn pace(rate: f32, beats: f32) -> [ParamSpec; 3] {
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

pub(super) fn kind(
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
pub(super) fn num_in(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn a_beat_cycle_should_snap_to_a_length_the_tempo_divides() {
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
    pub(super) fn a_pace_should_sit_flat_in_the_effect_in_the_file() {
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
}
