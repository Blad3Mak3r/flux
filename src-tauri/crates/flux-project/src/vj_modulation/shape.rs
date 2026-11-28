//! The waves of an LFO and the clock it runs on.

use std::f32::consts::{PI, TAU};

use super::*;
use crate::vj_effects::{Pace, Sync};

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
pub(super) fn seed_of(key: &str) -> u64 {
    key.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vj_modulation::support::*;

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
}
