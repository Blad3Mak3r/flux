//! Keeps the VJ guides in step with the code: every effect, generator and blend mode must have an
//! entry headed `Name · `fileName`` (its serde name in backticks) in its page of `docs/`.

use std::fs;

use flux_project::generator::Generator;
use flux_project::vj::Blend;
use flux_project::vj_effects::EffectKind;
use serde_json::Value;

fn page(name: &str) -> String {
    let path = format!("{}/../../../docs/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {path}: {err}"))
}

/// Fails naming each item of `what` that has no heading ending in `· `item`` in `docs/<file>`.
fn assert_documented(file: &str, what: &str, items: impl IntoIterator<Item = String>) {
    let text = page(file);
    let missing: Vec<String> = items
        .into_iter()
        .filter(|item| {
            let tail = format!("· `{item}`");
            !text
                .lines()
                .any(|line| line.starts_with('#') && line.trim_end().ends_with(&tail))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "docs/{file} lacks an entry for the {what}: {}. Add a heading `### Name · `{}`` for each and its settings.",
        missing.join(", "),
        missing[0]
    );
}

/// The serde name of a value: its `effect` or `kind` field, or the string itself.
fn serde_name(value: &impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(name) => name,
        Value::Object(map) => map
            .get("kind")
            .or_else(|| map.get("effect"))
            .and_then(Value::as_str)
            .expect("a kind or an effect field")
            .to_owned(),
        other => panic!("no name in {other}"),
    }
}

#[test]
fn every_effect_should_be_documented_in_vj_effects() {
    assert_documented(
        "vj-effects.md",
        "effect(s)",
        EffectKind::names().map(str::to_owned),
    );
}

/// One of each generator. The match has no wildcard, so a new generator stops this from compiling
/// until it is listed here (and then documented).
fn generators() -> Vec<Generator> {
    let all: Vec<Generator> = [
        "solid",
        "strobe",
        "gradient",
        "bars",
        "noise",
        "text",
        "liveInput",
    ]
    .iter()
    .map(|kind| serde_json::from_value(serde_json::json!({ "kind": kind })).unwrap())
    .collect();
    for generator in &all {
        match generator {
            Generator::Solid { .. }
            | Generator::Strobe { .. }
            | Generator::Gradient { .. }
            | Generator::Bars
            | Generator::Noise { .. }
            | Generator::Text { .. }
            | Generator::LiveInput { .. } => {}
        }
    }
    all
}

#[test]
fn every_generator_should_be_documented_in_vj_generators() {
    assert_documented(
        "vj-generators.md",
        "generator(s)",
        generators().iter().map(serde_name),
    );
}

/// Every blend mode; the match has no wildcard, so a new one stops this from compiling.
fn blends() -> Vec<Blend> {
    use Blend::*;
    let all = vec![
        Alpha,
        Add,
        Subtract,
        Multiply,
        Screen,
        Lighten,
        Darken,
        Difference,
        Exclusion,
        Overlay,
        SoftLight,
        HardLight,
        ColorDodge,
        ColorBurn,
        LinearLight,
        VividLight,
        PinLight,
        Hue,
        Saturation,
        Color,
        Luminosity,
        AlphaCut,
        LumaKey,
        LumaKeyInverted,
        LumaIsAlpha,
        DifferenceInverted,
        Mask50,
        Rgb,
        Displace,
    ];
    for blend in &all {
        match blend {
            Alpha | Add | Subtract | Multiply | Screen | Lighten | Darken | Difference
            | Exclusion | Overlay | SoftLight | HardLight | ColorDodge | ColorBurn
            | LinearLight | VividLight | PinLight | Hue | Saturation | Color | Luminosity
            | AlphaCut | LumaKey | LumaKeyInverted | LumaIsAlpha | DifferenceInverted | Mask50
            | Rgb | Displace => {}
        }
    }
    all
}

#[test]
fn every_blend_mode_should_be_documented_in_vj_blend_modes() {
    assert_documented(
        "vj-blend-modes.md",
        "blend mode(s)",
        blends().iter().map(serde_name),
    );
}
