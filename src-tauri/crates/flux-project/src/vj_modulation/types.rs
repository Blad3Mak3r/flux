//! What a modulation is: its shape, its source and what it moves.

use serde::{Deserialize, Serialize};

use crate::vj_effects::{BEAT_CYCLES, Pace};
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
    /// A knob of the dashboard, 0 to 1.
    Knob {
        knob: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj::Vj;
    use crate::vj_effects::Sync;
    use crate::vj_modulation::support::*;
    use crate::vj_modulation::*;

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
    fn a_knob_modulation_should_be_kept_in_range_stored_and_shown_as_a_value() {
        let mut vj = modulated_knob(-3.0, 4.0);
        let m = vj.dashboard[2].modulation.unwrap();
        assert_eq!((m.min, m.max), (0.0, 1.0));
        let json = serde_json::to_string(&vj).unwrap();
        assert_eq!(serde_json::from_str::<Vj>(&json).unwrap(), vj);
        let mut modulator = Modulator::default();
        let shown = values(&vj, &mut modulator, at(0.0, 0.25), Bands::default());
        near(shown.knobs[&2], 0.25);
        crate::vj_mapping::modulate_knob(&mut vj, 2, None).unwrap();
        assert!(!vj.is_modulated());
    }

    #[test]
    fn a_knob_target_should_read_from_the_json_the_window_sends() {
        let target: ModTarget = serde_json::from_str(r#"{"knob":7}"#).unwrap();
        assert_eq!(target, ModTarget::Knob { knob: 7 });
    }
}
