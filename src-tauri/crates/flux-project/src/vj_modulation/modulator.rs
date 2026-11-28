//! The smoothing of the audio sources.

use std::collections::HashMap;

use super::*;
use crate::vj::Vj;

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
    pub(super) fn prune(&mut self, vj: &Vj) {
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vj_modulation::support::*;

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
    fn a_knob_should_follow_the_sound_and_be_known_to_the_pruning() {
        let mut vj = modulated_knob(0.0, 1.0);
        crate::vj_mapping::modulate_knob(&mut vj, 2, Some(audio(Band::Low, 1.0, 0.0, 0.0)))
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
}
