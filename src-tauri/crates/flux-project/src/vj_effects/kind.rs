//! The kinds of effect, with the settings of each.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::*;
use crate::{ProjectError, Result};

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
}
