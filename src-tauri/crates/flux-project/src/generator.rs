//! Generators: clips of the VJ grid whose picture Flux makes itself instead of reading a video, such
//! as a solid colour or a strobe. A generator fills the composition, and its parameters are stored
//! with the clip; a parameter missing from a file takes its default, so a new clip is named by its
//! kind alone.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

/// Fastest a strobe flashes or a noise changes, a second.
pub const MAX_RATE: f32 = 30.0;

/// What a generator draws, with its parameters. Colours are `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Generator {
    /// One colour over the whole composition.
    Solid {
        #[serde(default = "white")]
        colour: String,
    },
    /// Flashes of `colour`, `rate` a second, each lasting `width` of the time between two (0 to 1);
    /// between flashes what is below shows.
    Strobe {
        #[serde(default = "white")]
        colour: String,
        #[serde(default = "strobe_rate")]
        rate: f32,
        #[serde(default = "half")]
        width: f32,
    },
    /// From `from` to `to` across the composition, along `angle` degrees: 0 is left to right, 90
    /// top to bottom.
    Gradient {
        #[serde(default = "black")]
        from: String,
        #[serde(default = "white")]
        to: String,
        #[serde(default)]
        angle: f32,
    },
    /// The colour bars of a video test card.
    Bars,
    /// Random snow of `colour`, new `rate` times a second.
    Noise {
        #[serde(default = "white")]
        colour: String,
        #[serde(default = "noise_rate")]
        rate: f32,
    },
}

fn white() -> String {
    "#ffffff".into()
}

fn black() -> String {
    "#000000".into()
}

fn strobe_rate() -> f32 {
    8.0
}

fn noise_rate() -> f32 {
    MAX_RATE
}

fn half() -> f32 {
    0.5
}

impl Generator {
    /// Its colours, to check.
    fn colours(&self) -> Vec<&str> {
        match self {
            Self::Solid { colour } | Self::Strobe { colour, .. } | Self::Noise { colour, .. } => {
                vec![colour]
            }
            Self::Gradient { from, to, .. } => vec![from, to],
            Self::Bars => Vec::new(),
        }
    }

    /// Itself with each number brought into its range, as a fader stands; a number that is not one
    /// stays, for [`Generator::validate`] to refuse.
    pub(crate) fn tidy(mut self) -> Self {
        let into = |v: &mut f32, min: f32, max: f32| *v = v.clamp(min, max);
        // The smallest rate and width that still flash.
        const LEAST: f32 = 0.01;
        match &mut self {
            Self::Strobe { rate, width, .. } => {
                into(rate, LEAST, MAX_RATE);
                into(width, LEAST, 1.0);
            }
            Self::Noise { rate, .. } => into(rate, LEAST, MAX_RATE),
            Self::Gradient { angle, .. } => into(angle, -360.0, 360.0),
            Self::Solid { .. } | Self::Bars => {}
        }
        self
    }

    /// Checks its parameters: known colours, a rate above 0 up to [`MAX_RATE`], a flash width
    /// above 0 up to 1 and an angle of -360 to 360.
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(bad) = self.colours().into_iter().find(|c| rgb(c).is_none()) {
            return Err(ProjectError::Invalid(format!(
                "a generator colour must look like #rrggbb: {bad}"
            )));
        }
        let fine = match *self {
            Self::Strobe { rate, width, .. } => {
                rate > 0.0 && rate <= MAX_RATE && width > 0.0 && width <= 1.0
            }
            Self::Noise { rate, .. } => rate > 0.0 && rate <= MAX_RATE,
            Self::Gradient { angle, .. } => (-360.0..=360.0).contains(&angle),
            Self::Solid { .. } | Self::Bars => true,
        };
        if fine {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!(
                "a generator needs a rate of 0 to {MAX_RATE}, a width of 0 to 1 and an angle of -360 to 360"
            )))
        }
    }
}

/// The red, green and blue of `#rrggbb`.
pub fn rgb(colour: &str) -> Option<[u8; 3]> {
    let digits = colour.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_alone_should_read_with_the_default_parameters() {
        let strobe: Generator = serde_json::from_str(r#"{"kind":"strobe"}"#).unwrap();
        assert_eq!(
            strobe,
            Generator::Strobe {
                colour: "#ffffff".into(),
                rate: 8.0,
                width: 0.5
            }
        );
        assert!(strobe.validate().is_ok());
        let bars: Generator = serde_json::from_str(r#"{"kind":"bars"}"#).unwrap();
        assert_eq!(bars, Generator::Bars);
    }

    #[test]
    fn validate_should_refuse_bad_colours_and_rates() {
        let colour = Generator::Solid {
            colour: "red".into(),
        };
        assert!(colour.validate().is_err());
        for rate in [0.0, MAX_RATE + 1.0, f32::NAN] {
            let noise = Generator::Noise {
                colour: "#ffffff".into(),
                rate,
            };
            assert!(noise.validate().is_err(), "{rate}");
        }
    }

    #[test]
    fn tidy_should_bring_numbers_into_range() {
        let fast = Generator::Strobe {
            colour: "#ffffff".into(),
            rate: 100.0,
            width: -1.0,
        };
        assert!(fast.clone().tidy().validate().is_ok());
        let nan = Generator::Gradient {
            from: "#000000".into(),
            to: "#ffffff".into(),
            angle: f32::NAN,
        };
        assert!(nan.tidy().validate().is_err());
    }

    #[test]
    fn rgb_should_read_a_hex_triplet() {
        assert_eq!(rgb("#ff8000"), Some([255, 128, 0]));
        assert_eq!(rgb("#ff80"), None);
        assert_eq!(rgb("ff8000"), None);
        assert_eq!(rgb("#ééé"), None);
    }
}
