//! What can be modulated and in which range, and the catalog the window shows.

use std::collections::BTreeMap;

use super::*;
use crate::vj::{Clip, MAX_OFFSET, MAX_SCALE};
use crate::vj_effects::{BEAT_CYCLES, Pace, Sync};
use crate::{ProjectError, Result};

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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vj_modulation::support::*;

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
}
