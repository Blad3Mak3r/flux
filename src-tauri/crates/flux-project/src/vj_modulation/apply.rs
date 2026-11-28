//! The grid with its modulated values.

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::Serialize;

use super::*;
use crate::vj::{MAX_OFFSET, MAX_SCALE, Vj};
use crate::vj_effects::Effect;

impl Vj {
    /// Whether anything of the grid is modulated.
    pub fn is_modulated(&self) -> bool {
        self.dashboard.iter().any(|k| k.modulation.is_some())
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
    pub(super) fn has_modulation_key(&self, key: &str) -> bool {
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
}

/// The modulated values of a grid at an instant, for the inspector to mark on its controls: by
/// effect id and setting, by layer id and field, by clip id and field.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Values {
    pub effects: BTreeMap<String, BTreeMap<String, f32>>,
    pub layers: BTreeMap<String, BTreeMap<String, f32>>,
    pub clips: BTreeMap<String, BTreeMap<String, f32>>,
    /// The modulated knobs of the dashboard, by place.
    pub knobs: BTreeMap<usize, f32>,
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::vj_effects::EffectKind;

    use crate::vj_modulation::support::*;

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
}
