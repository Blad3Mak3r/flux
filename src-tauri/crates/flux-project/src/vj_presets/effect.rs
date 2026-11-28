//! Effect presets: one effect's settings, or a whole stack, under a name.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::*;
use crate::serde_util::is_false;
use crate::vj::{Effect, EffectKind, EffectTarget, MAX_EFFECTS, Modulation, Vj};
use crate::{ProjectError, Result};

/// One effect of a preset: its kind and settings as the map the kind is stored as, so a preset
/// written when a kind had fewer settings still applies (the settings it lacks keep their value).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetEffect {
    /// The settings, with `effect` naming the kind.
    pub settings: Map<String, Value>,
    /// What drives a numeric setting by itself, by the setting's key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
}

impl PresetEffect {
    fn of(effect: &Effect) -> Self {
        Self {
            settings: effect.kind.settings(),
            modulation: effect.modulation.clone(),
            bypass: effect.bypass,
        }
    }

    /// The kind's name, empty if the preset lacks it.
    pub fn name(&self) -> &str {
        self.settings
            .get("effect")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// A new effect with the preset's settings over the ones the kind starts with.
    fn effect(&self, id: String) -> Result<Effect> {
        let kind = EffectKind::default_of(self.name())?.with_settings(&self.settings)?;
        Ok(Effect {
            id,
            bypass: self.bypass,
            modulation: self.modulation_for(&kind),
            kind,
        })
    }

    /// The modulation the kind can take: a setting that is no longer modulatable is left out.
    fn modulation_for(&self, kind: &EffectKind) -> BTreeMap<String, Modulation> {
        self.modulation
            .iter()
            .filter(|(key, _)| kind.modulatable_range(key).is_ok())
            .map(|(key, modulation)| (key.clone(), *modulation))
            .collect()
    }
}

/// A named set of effect settings: one effect, or a whole stack when it holds several.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectPreset {
    pub id: String,
    pub name: String,
    /// At least one, at most [`MAX_EFFECTS`].
    pub effects: Vec<PresetEffect>,
}

impl EffectPreset {
    /// The preset of one effect, or of the whole stack of a clip, a layer, a group or the
    /// composition.
    pub fn of_effect(vj: &Vj, effect: &str, id: String, name: &str) -> Result<Self> {
        let found = stacks(vj)
            .flatten()
            .find(|e| e.id == effect)
            .ok_or_else(|| ProjectError::Invalid(format!("no effect {effect}")))?;
        Self::new(id, name, vec![PresetEffect::of(found)])
    }

    pub fn of_stack(vj: &Vj, target: &EffectTarget, id: String, name: &str) -> Result<Self> {
        let effects = stack(vj, target)?.iter().map(PresetEffect::of).collect();
        Self::new(id, name, effects)
    }

    fn new(id: String, name: &str, effects: Vec<PresetEffect>) -> Result<Self> {
        if effects.is_empty() || effects.len() > MAX_EFFECTS {
            return Err(ProjectError::Invalid(format!(
                "a preset holds 1 to {MAX_EFFECTS} effects"
            )));
        }
        Ok(Self {
            id,
            name: name_of(name)?,
            effects,
        })
    }

    /// The kind of its first effect when it is the preset of one effect.
    pub fn single(&self) -> Option<&str> {
        (self.effects.len() == 1).then(|| self.effects[0].name())
    }
}

pub(super) fn stack<'a>(vj: &'a Vj, target: &EffectTarget) -> Result<&'a Vec<Effect>> {
    let missing = |what: &str, id: &str| ProjectError::Invalid(format!("no {what} {id}"));
    Ok(match target {
        EffectTarget::Composition => &vj.composition_effects,
        EffectTarget::Layer(id) => {
            &vj.layers
                .iter()
                .find(|l| l.id == *id)
                .ok_or_else(|| missing("layer", id))?
                .effects
        }
        EffectTarget::Group(id) => {
            &vj.groups
                .iter()
                .find(|g| g.id == *id)
                .ok_or_else(|| missing("group", id))?
                .effects
        }
        EffectTarget::Clip(id) => {
            &vj.layers
                .iter()
                .flat_map(|l| &l.clips)
                .find(|c| c.id == *id)
                .ok_or_else(|| missing("clip", id))?
                .effects
        }
    })
}

pub(super) fn stacks(vj: &Vj) -> impl Iterator<Item = &Vec<Effect>> {
    std::iter::once(&vj.composition_effects)
        .chain(vj.groups.iter().map(|g| &g.effects))
        .chain(vj.layers.iter().flat_map(|layer| {
            std::iter::once(&layer.effects).chain(layer.clips.iter().map(|c| &c.effects))
        }))
}

/// Gives `effect` the settings of the one-effect `preset`: the settings the preset has, in the
/// range of the kind; the ones it lacks keep their value. The effect and the preset must be the
/// same kind, and the effect keeps its place, id and bypass.
pub(crate) fn apply_to_effect(vj: &mut Vj, effect: &str, preset: &EffectPreset) -> Result<()> {
    let [source] = preset.effects.as_slice() else {
        return Err(ProjectError::Invalid(
            "a stack preset is added to a stack, not applied to an effect".into(),
        ));
    };
    let (stack, at) = vj.effect_mut(effect)?;
    let target = &mut stack[at];
    if target.kind.name() != source.name() {
        return Err(ProjectError::Invalid(format!(
            "the preset {} is for {}, not {}",
            preset.name,
            source.name(),
            target.kind.name()
        )));
    }
    target.kind = target.kind.with_settings(&source.settings)?;
    target.modulation = source.modulation_for(&target.kind);
    Ok(())
}

/// Puts the effects of `preset` at the end of the stack of `target`, with new ids.
pub(crate) fn add_to_stack(
    vj: &mut Vj,
    target: &EffectTarget,
    preset: &EffectPreset,
    mut new_id: impl FnMut() -> String,
) -> Result<()> {
    let effects = preset
        .effects
        .iter()
        .map(|e| e.effect(new_id()))
        .collect::<Result<Vec<_>>>()?;
    if effects
        .iter()
        .any(|e| e.kind.composition_only() && *target != EffectTarget::Composition)
    {
        return Err(ProjectError::Invalid(
            "an effect of the preset only works on the composition".into(),
        ));
    }
    let stack = match target {
        EffectTarget::Clip(id) => &mut vj.clip_mut(id)?.effects,
        EffectTarget::Layer(id) => &mut vj.layer_mut(id)?.effects,
        EffectTarget::Group(id) => &mut vj.group_mut(id)?.effects,
        EffectTarget::Composition => &mut vj.composition_effects,
    };
    if stack.len() + effects.len() > MAX_EFFECTS {
        return Err(ProjectError::Invalid(format!(
            "a stack has at most {MAX_EFFECTS} effects"
        )));
    }
    stack.extend(effects);
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    use crate::vj::{EffectTarget, VjOp};

    use crate::vj_presets::support::*;

    #[test]
    fn applying_a_preset_should_set_its_settings_and_keep_the_effects_place() {
        let mut project = show();
        let (a, b) = (
            project.vj.layers[0].id.clone(),
            add_clip_layer(&mut project),
        );
        let from = add_effect(&mut project, EffectTarget::Layer(a), "tint");
        tint(&mut project, &from, "#ff0000", 0.9);
        let to = add_effect(&mut project, EffectTarget::Layer(b), "tint");
        let preset = EffectPreset::of_effect(&project.vj, &from, "p".into(), "Red").unwrap();
        crate::vj::apply(
            &mut project,
            VjOp::ApplyEffectPreset {
                effect: to.clone(),
                preset: preset.clone(),
            },
            counter(),
        )
        .unwrap();
        assert_eq!(settings(&project, &to), settings(&project, &from));
        // A kind other than the preset's is refused.
        let blur = add_effect(&mut project, EffectTarget::Composition, "brightness");
        let wrong = VjOp::ApplyEffectPreset {
            effect: blur,
            preset,
        };
        assert!(crate::vj::apply(&mut project, wrong, counter()).is_err());
    }

    #[test]
    fn the_settings_a_preset_lacks_should_keep_their_value() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let effect = add_effect(&mut project, EffectTarget::Layer(layer), "tint");
        tint(&mut project, &effect, "#00ff00", 0.25);
        let mut preset = EffectPreset::of_effect(&project.vj, &effect, "p".into(), "Old").unwrap();
        preset.effects[0].settings.remove("colour");
        preset.effects[0]
            .settings
            .insert("amount".into(), 0.75.into());
        // A setting this build does not know is ignored, a value out of range brought into it.
        preset.effects[0].settings.insert("legacy".into(), 3.into());
        crate::vj::apply(
            &mut project,
            VjOp::ApplyEffectPreset {
                effect: effect.clone(),
                preset,
            },
            counter(),
        )
        .unwrap();
        let now = settings(&project, &effect);
        assert_eq!(now["colour"], "#00ff00");
        assert_eq!(now["amount"], 0.75);
        assert!(!now.contains_key("legacy"));
    }

    #[test]
    fn a_stack_preset_should_add_its_effects_with_new_ids_where_they_are_allowed() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let first = add_effect(
            &mut project,
            EffectTarget::Layer(layer.clone()),
            "brightness",
        );
        add_effect(&mut project, EffectTarget::Layer(layer.clone()), "tint");
        let target = EffectTarget::Layer(layer);
        let preset = EffectPreset::of_stack(&project.vj, &target, "s".into(), "Look").unwrap();
        assert_eq!(preset.single(), None);
        assert!(
            crate::vj::apply(
                &mut project,
                VjOp::ApplyEffectPreset {
                    effect: first,
                    preset: preset.clone()
                },
                counter()
            )
            .is_err()
        );
        let op = VjOp::AddEffectPreset {
            target: target.clone(),
            preset: preset.clone(),
        };
        crate::vj::apply(&mut project, op, counter_from(500)).unwrap();
        let ids: Vec<_> = stack(&project.vj, &target)
            .unwrap()
            .iter()
            .map(|e| &e.id)
            .collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        project.validate().unwrap();
        let empty =
            EffectPreset::of_stack(&project.vj, &EffectTarget::Composition, "e".into(), "None");
        assert!(empty.is_err());
    }
}
