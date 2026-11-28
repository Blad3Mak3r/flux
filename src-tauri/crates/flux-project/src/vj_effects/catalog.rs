//! The catalog of the kinds of effect.

use std::sync::OnceLock;

use super::*;
use crate::{ProjectError, Result};

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_effect_should_have_its_words_in_english_and_in_spanish() {
        let catalogues = [
            (
                "en",
                include_str!("../../../../../src/i18n/en/vj-effects.ts"),
            ),
            (
                "es",
                include_str!("../../../../../src/i18n/es/vj-effects.ts"),
            ),
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
