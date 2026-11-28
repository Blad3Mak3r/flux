//! The dashboards of a layer and of a clip, as Resolume has them: each owns [`KNOBS`] knobs of the
//! kind the composition dashboard has (see [`crate::vj_mapping`]), and a knob moves the numbers
//! linked to it, each into the range of its link.
//!
//! A [`Scope`] says whose dashboard an edit is on. A layer's links name numbers of the layer: its
//! own faders, its effects, its clips' speed, generators and effects. A clip's name numbers of the
//! clip. So a number of a clip can be on the dashboard of the clip, of its layer or of the
//! composition, and one of a layer on the layer's or the composition's. The knobs are stored in the
//! layer and the clip, so a clip carries its dashboard wherever it is moved.
//!
//! Everything is the same code as the composition's: turning a knob is one edit that moves the
//! linked numbers, and a modulated knob moves them in the drawn copy of the grid.

use serde::Deserialize;

use crate::model::Project;
use crate::vj::{Clip, Layer, Vj};
use crate::vj_mapping::{KNOBS, Knob, Link, MAX_LINKS, Target, finite, linkable, value_op};
use crate::vj_modulation::Modulation;
use crate::{ProjectError, Result};

/// Whose dashboard an edit is on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Scope {
    #[default]
    Composition,
    Layer {
        layer: String,
    },
    Clip {
        clip: String,
    },
}

fn knobs<'a>(vj: &'a mut Vj, scope: &Scope) -> Result<&'a mut Vec<Knob>> {
    Ok(match scope {
        Scope::Composition => &mut vj.dashboard,
        Scope::Layer { layer } => &mut vj.layer_mut(layer)?.dashboard,
        Scope::Clip { clip } => &mut vj.clip_mut(clip)?.dashboard,
    })
}

/// The knob `knob` of the dashboard, made if the dashboard is not that long yet.
fn knob_mut<'a>(vj: &'a mut Vj, scope: &Scope, knob: usize) -> Result<&'a mut Knob> {
    if knob >= KNOBS {
        return Err(ProjectError::Invalid(format!(
            "a dashboard has {KNOBS} knobs"
        )));
    }
    let list = knobs(vj, scope)?;
    if list.len() <= knob {
        list.resize_with(knob + 1, Knob::default);
    }
    Ok(&mut list[knob])
}

fn layer_has(layer: &Layer, target: &Target) -> bool {
    match target {
        Target::LayerMaster { layer: id }
        | Target::LayerVolume { layer: id }
        | Target::LayerOpacity { layer: id }
        | Target::LayerTransition { layer: id } => *id == layer.id,
        Target::ClipSpeed { clip } | Target::GeneratorNumber { clip, .. } => {
            layer.clips.iter().any(|c| c.id == *clip)
        }
        Target::EffectParam { effect, .. } => layer
            .effects
            .iter()
            .chain(layer.clips.iter().flat_map(|c| &c.effects))
            .any(|e| e.id == *effect),
        _ => false,
    }
}

fn clip_has(clip: &Clip, target: &Target) -> bool {
    match target {
        Target::ClipSpeed { clip: id } | Target::GeneratorNumber { clip: id, .. } => *id == clip.id,
        Target::EffectParam { effect, .. } => clip.effects.iter().any(|e| e.id == *effect),
        _ => false,
    }
}

/// Whether the dashboard of `scope` may drive `target`: a number stored with the grid that the
/// owner of the dashboard has.
fn in_scope(vj: &Vj, scope: &Scope, target: &Target) -> bool {
    linkable(target)
        && match scope {
            Scope::Composition => true,
            Scope::Layer { layer } => vj
                .layers
                .iter()
                .find(|l| l.id == *layer)
                .is_some_and(|l| layer_has(l, target)),
            Scope::Clip { clip } => vj.find_clip(clip).is_some_and(|(_, c)| clip_has(c, target)),
        }
}

pub(crate) fn link(
    vj: &mut Vj,
    scope: &Scope,
    knob: usize,
    target: Target,
    min: Option<f32>,
    max: Option<f32>,
) -> Result<()> {
    if !in_scope(vj, scope, &target) {
        return Err(ProjectError::Invalid(
            "a dashboard drives a number of its own layer or clip".into(),
        ));
    }
    let (lo, hi) = target
        .range(vj)
        .ok_or_else(|| ProjectError::Invalid(format!("there is nothing to link: {target:?}")))?;
    let link = Link {
        min: finite(min.unwrap_or(lo))?.clamp(lo, hi),
        max: finite(max.unwrap_or(hi))?.clamp(lo, hi),
        target,
    };
    let knob = knob_mut(vj, scope, knob)?;
    if let Some(old) = knob.links.iter_mut().find(|l| l.target == link.target) {
        *old = link;
    } else if knob.links.len() >= MAX_LINKS {
        return Err(ProjectError::Invalid(format!(
            "a knob drives at most {MAX_LINKS} numbers"
        )));
    } else {
        knob.links.push(link);
    }
    Ok(())
}

pub(crate) fn unlink(vj: &mut Vj, scope: &Scope, knob: usize, target: &Target) -> Result<()> {
    knob_mut(vj, scope, knob)?
        .links
        .retain(|l| &l.target != target);
    Ok(())
}

pub(crate) fn rename(vj: &mut Vj, scope: &Scope, knob: usize, name: Option<String>) -> Result<()> {
    knob_mut(vj, scope, knob)?.name = name;
    Ok(())
}

/// Sets or takes away the modulation of a knob, brought into 0 to 1.
pub(crate) fn modulate(
    vj: &mut Vj,
    scope: &Scope,
    knob: usize,
    modulation: Option<Modulation>,
) -> Result<()> {
    let modulation = modulation.map(|m| m.tidy(0.0, 1.0)).transpose()?;
    knob_mut(vj, scope, knob)?.modulation = modulation;
    Ok(())
}

/// Turns the knob and moves what it is linked to, as one edit. A link that cannot move (its
/// target is gone) is skipped; pruning takes it away at the next removal.
pub(crate) fn turn(project: &mut Project, scope: &Scope, knob: usize, value: f32) -> Result<()> {
    let value = finite(value)?.clamp(0.0, 1.0);
    let links = {
        let knob = knob_mut(&mut project.vj, scope, knob)?;
        knob.value = value;
        knob.links.clone()
    };
    for link in links {
        if let Some(op) = value_op(&project.vj, &link.target, link.at(value)) {
            // The ops of a link make no ids, and `String::new` keeps the recursion one type deep.
            crate::vj::apply(project, op, String::new)?;
        }
    }
    Ok(())
}

/// Moves what the knob `index` of `scope` in `vj` is linked to, as if it stood at `value`.
pub(crate) fn drive(vj: &mut Vj, scope: &Scope, index: usize, value: f32) {
    let Ok(list) = knobs(vj, scope) else {
        return;
    };
    let Some(knob) = list.get_mut(index) else {
        return;
    };
    knob.value = value.clamp(0.0, 1.0);
    let moves: Vec<(Target, f32)> = knob
        .links
        .iter()
        .map(|link| (link.target.clone(), link.at(value)))
        .collect();
    for (target, v) in moves {
        crate::vj_mapping::drive(vj, &target, v);
    }
}

/// The knobs of every layer and clip, with the scope each is on and its place.
pub(crate) fn scoped(vj: &Vj) -> impl Iterator<Item = (Scope, usize, &Knob)> {
    vj.layers.iter().flat_map(|layer| {
        let own = layer.dashboard.iter().enumerate().map(|(at, knob)| {
            let scope = Scope::Layer {
                layer: layer.id.clone(),
            };
            (scope, at, knob)
        });
        let clips = layer.clips.iter().flat_map(|clip| {
            clip.dashboard.iter().enumerate().map(|(at, knob)| {
                let scope = Scope::Clip {
                    clip: clip.id.clone(),
                };
                (scope, at, knob)
            })
        });
        own.chain(clips)
    })
}

/// What a modulated knob of a layer or a clip is called to the modulator: unique in the grid.
pub(crate) fn key(scope: &Scope, at: usize) -> String {
    match scope {
        Scope::Composition => format!("k:{at}:value"),
        Scope::Layer { layer } => format!("lk:{layer}:{at}:value"),
        Scope::Clip { clip } => format!("ck:{clip}:{at}:value"),
    }
}

/// Whether `head` (a modulation key without its last part) is a modulated knob of a layer or a clip.
pub(crate) fn has_key(vj: &Vj, head: &str) -> bool {
    let Some((id, at)) = head.rsplit_once(':') else {
        return false;
    };
    let Ok(at) = at.parse::<usize>() else {
        return false;
    };
    let modulated = |list: &[Knob]| list.get(at).is_some_and(|k| k.modulation.is_some());
    if let Some(id) = id.strip_prefix("lk:") {
        vj.layers
            .iter()
            .any(|l| l.id == id && modulated(&l.dashboard))
    } else if let Some(id) = id.strip_prefix("ck:") {
        vj.layers
            .iter()
            .flat_map(|l| &l.clips)
            .any(|c| c.id == id && modulated(&c.dashboard))
    } else {
        false
    }
}

/// Takes away the links of layers and clips whose target is gone or is no longer theirs (a clip
/// moved to another layer leaves its old layer's dashboard).
pub(crate) fn prune(vj: &mut Vj) {
    let mut keep: Vec<(Scope, usize, Vec<bool>)> = Vec::new();
    for (scope, at, knob) in scoped(vj) {
        let flags = knob
            .links
            .iter()
            .map(|l| l.target.exists(vj) && in_scope(vj, &scope, &l.target))
            .collect();
        keep.push((scope, at, flags));
    }
    for (scope, at, flags) in keep {
        if flags.iter().all(|f| *f) {
            continue;
        }
        if let Ok(list) = knobs(vj, &scope) {
            let mut flags = flags.into_iter();
            list[at].links.retain(|_| flags.next().unwrap_or(true));
        }
    }
}

/// Checks the dashboards of the layers and the clips: knobs, names, modulation and links.
pub(crate) fn validate(vj: &Vj) -> Result<()> {
    for layer in &vj.layers {
        if layer.dashboard.len() > KNOBS || layer.clips.iter().any(|c| c.dashboard.len() > KNOBS) {
            return Err(ProjectError::Invalid(format!(
                "a dashboard has at most {KNOBS} knobs"
            )));
        }
    }
    for (scope, _, knob) in scoped(vj) {
        if !(0.0..=1.0).contains(&knob.value) || knob.links.len() > MAX_LINKS {
            return Err(ProjectError::Invalid(
                "a knob is 0 to 1 and drives at most 16 numbers".into(),
            ));
        }
        crate::vj::check_name(knob.name.as_deref())?;
        if let Some(m) = &knob.modulation {
            m.validate(0.0, 1.0)?;
        }
        for link in &knob.links {
            let (lo, hi) = link.target.range(vj).unwrap_or((1.0, 0.0));
            let inside = |v: f32| v.is_finite() && (lo..=hi).contains(&v);
            if !in_scope(vj, &scope, &link.target) || !inside(link.min) || !inside(link.max) {
                return Err(ProjectError::Invalid(
                    "a knob is linked to a number that is not its own, or out of its range".into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OscArg;
    use crate::generator::Generator;
    use crate::vj::{EffectTarget, VjOp};
    use crate::vj_mapping::{Command, osc_command};
    use crate::{Project, ProjectMode};
    use flux_core::FrameRate;

    fn show() -> Project {
        let mut ids = (0..).map(|n| format!("id{n}"));
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(|| ids.next().unwrap());
        project
    }

    fn try_run(project: &mut Project, op: VjOp) -> Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(5000);
        crate::vj::apply(project, op, || {
            format!("n{}", NEXT.fetch_add(1, Ordering::Relaxed))
        })
    }

    fn run(project: &mut Project, op: VjOp) {
        try_run(project, op).unwrap();
    }

    fn tint(project: &mut Project, target: EffectTarget) -> String {
        run(
            project,
            VjOp::AddEffect {
                target: target.clone(),
                kind: "tint".into(),
            },
        );
        let vj = &project.vj;
        let stack = match &target {
            EffectTarget::Layer(id) => &vj.layers.iter().find(|l| l.id == *id).unwrap().effects,
            EffectTarget::Clip(id) => &vj.find_clip(id).unwrap().1.effects,
            _ => unreachable!(),
        };
        stack.last().unwrap().id.clone()
    }

    fn generator(project: &mut Project, layer: usize) -> String {
        let layer_id = project.vj.layers[layer].id.clone();
        let column = project.vj.decks[0].columns[0].id.clone();
        run(
            project,
            VjOp::PlaceGenerator {
                generator: Generator::Strobe {
                    colour: "#ffffff".into(),
                    rate: 8.0,
                    width: 0.5,
                },
                layer: layer_id,
                column,
            },
        );
        project.vj.layers[layer].clips[0].id.clone()
    }

    fn param(effect: &str) -> Target {
        Target::EffectParam {
            effect: effect.into(),
            param: "amount".into(),
        }
    }

    /// The `amount` an effect has.
    fn amount(project: &Project, effect: &str) -> f32 {
        let kind = &project.vj.find_effect(effect).unwrap().kind;
        serde_json::to_value(kind).unwrap()["amount"]
            .as_f64()
            .unwrap() as f32
    }

    fn link(project: &mut Project, scope: &Scope, knob: usize, target: Target) {
        try_link(project, scope, knob, target).unwrap();
    }

    fn try_link(project: &mut Project, scope: &Scope, knob: usize, target: Target) -> Result<()> {
        let op = VjOp::DashboardLink {
            scope: scope.clone(),
            knob,
            target,
            min: None,
            max: None,
        };
        try_run(project, op)
    }

    fn turn(project: &mut Project, scope: &Scope, knob: usize, value: f32) {
        let scope = scope.clone();
        run(project, VjOp::DashboardValue { scope, knob, value });
    }

    fn layer_scope(project: &Project, at: usize) -> Scope {
        Scope::Layer {
            layer: project.vj.layers[at].id.clone(),
        }
    }

    #[test]
    fn a_layer_knob_and_a_clip_knob_should_move_their_links_into_the_range_of_each() {
        let mut project = show();
        let clip = generator(&mut project, 0);
        let layer_fx = tint(&mut project, EffectTarget::Layer("id0".into()));
        let clip_fx = tint(&mut project, EffectTarget::Clip(clip.clone()));
        let layer = layer_scope(&project, 0);
        let on_clip = Scope::Clip { clip: clip.clone() };
        let op = VjOp::DashboardLink {
            scope: layer.clone(),
            knob: 2,
            target: param(&layer_fx),
            min: Some(0.2),
            max: Some(0.6),
        };
        run(&mut project, op);
        link(&mut project, &on_clip, 0, param(&clip_fx));
        // A layer's knob also reaches the effects of its clips.
        link(&mut project, &layer, 3, param(&clip_fx));

        turn(&mut project, &layer, 2, 0.5);
        assert!((amount(&project, &layer_fx) - 0.4).abs() < 1e-5);
        assert_eq!(project.vj.layers[0].dashboard[2].value, 0.5);

        let (lo, hi) = param(&clip_fx).range(&project.vj).unwrap();
        assert!(lo < hi);
        turn(&mut project, &on_clip, 0, 1.0);
        assert!((amount(&project, &clip_fx) - hi).abs() < 1e-5);
        turn(&mut project, &on_clip, 0, 0.0);
        assert!((amount(&project, &clip_fx) - lo).abs() < 1e-5);
        turn(&mut project, &layer, 3, 0.5);
        assert!((amount(&project, &clip_fx) - (lo + hi) / 2.0).abs() < 1e-5);
        project.validate().unwrap();
    }

    #[test]
    fn a_dashboard_should_refuse_a_number_that_is_not_its_owners() {
        let mut project = show();
        let clip = generator(&mut project, 0);
        let other_fx = tint(&mut project, EffectTarget::Layer("id1".into()));
        let on_clip = Scope::Clip { clip };
        let on_layer = layer_scope(&project, 0);
        assert!(try_link(&mut project, &on_layer, 0, param(&other_fx)).is_err());
        assert!(try_link(&mut project, &on_clip, 0, param(&other_fx)).is_err());
        let opacity = |layer: &str| Target::LayerOpacity {
            layer: layer.into(),
        };
        assert!(try_link(&mut project, &on_clip, 0, opacity("id0")).is_err());
        assert!(try_link(&mut project, &on_layer, 0, opacity("id1")).is_err());
        let knob = Target::Dashboard { knob: 0 };
        assert!(try_link(&mut project, &on_layer, 0, knob).is_err());
        assert!(try_link(&mut project, &on_layer, 0, opacity("id0")).is_ok());
        let ninth = VjOp::DashboardValue {
            scope: on_layer,
            knob: KNOBS,
            value: 0.5,
        };
        assert!(try_run(&mut project, ninth).is_err());
    }

    #[test]
    fn the_dashboards_should_round_trip_and_stay_with_a_clip_that_is_moved() {
        let mut project = show();
        let clip = generator(&mut project, 0);
        let fx = tint(&mut project, EffectTarget::Clip(clip.clone()));
        let on_clip = Scope::Clip { clip: clip.clone() };
        link(&mut project, &on_clip, 1, param(&fx));
        let rename = VjOp::RenameKnob {
            scope: on_clip.clone(),
            knob: 1,
            name: "Wash".into(),
        };
        run(&mut project, rename);
        turn(&mut project, &on_clip, 1, 0.25);
        let column = project.vj.decks[0].columns[3].id.clone();
        let layer = "id1".to_string();
        run(
            &mut project,
            VjOp::MoveClip {
                clip: clip.clone(),
                layer,
                column,
            },
        );
        let (layer, moved) = project.vj.find_clip(&clip).unwrap();
        assert_eq!(layer.id, "id1");
        assert_eq!(moved.dashboard[1].name.as_deref(), Some("Wash"));
        assert_eq!(moved.dashboard[1].links.len(), 1);
        assert_eq!(moved.dashboard[1].value, 0.25);
        project.validate().unwrap();

        let json = serde_json::to_string(&project.vj).unwrap();
        let back: Vj = serde_json::from_str(&json).unwrap();
        assert_eq!(back, project.vj);
        let bare = serde_json::to_string(&show().vj).unwrap();
        assert!(
            !bare.contains("dashboard"),
            "an empty dashboard is not saved"
        );
    }

    #[test]
    fn a_layer_link_should_go_when_its_clip_moves_to_another_layer() {
        let mut project = show();
        let clip = generator(&mut project, 0);
        let fx = tint(&mut project, EffectTarget::Clip(clip.clone()));
        let on_layer = layer_scope(&project, 0);
        link(&mut project, &on_layer, 0, param(&fx));
        assert_eq!(project.vj.layers[0].dashboard[0].links.len(), 1);
        let layer = "id1".to_string();
        let column = project.vj.decks[0].columns[1].id.clone();
        run(
            &mut project,
            VjOp::MoveClip {
                clip,
                layer,
                column,
            },
        );
        assert!(project.vj.layers[0].dashboard[0].links.is_empty());
        project.validate().unwrap();
    }

    #[test]
    fn the_osc_addresses_of_a_layer_and_a_clip_dashboard_should_name_their_knobs() {
        let mut project = show();
        let clip = generator(&mut project, 0);
        let level = [OscArg::Float(0.5)];
        let heard = |address: &str| osc_command(&project.vj, address, &level);
        let layer_knob = Target::LayerDashboard {
            layer: "id1".into(),
            knob: 2,
        };
        assert_eq!(
            heard("/composition/layers/2/dashboard/link3"),
            Some((layer_knob, Command::Value(0.5)))
        );
        let clip_knob = Target::ClipDashboard { clip, knob: 7 };
        assert_eq!(
            heard("/composition/layers/1/clips/1/dashboard/link8"),
            Some((clip_knob, Command::Value(0.5)))
        );
        assert_eq!(heard("/composition/layers/1/dashboard/link9"), None);
        assert_eq!(heard("/composition/layers/1/clips/2/dashboard/link1"), None);
        assert_eq!(heard("/composition/layers/9/dashboard/link1"), None);
    }

    #[test]
    fn a_mapped_layer_knob_should_turn_the_knob_and_follow_the_slot_of_its_layer() {
        let mut project = show();
        let on_layer = layer_scope(&project, 1);
        let opacity = Target::LayerOpacity {
            layer: "id1".into(),
        };
        link(&mut project, &on_layer, 4, opacity);
        let knob = Target::LayerDashboard {
            layer: "id1".into(),
            knob: 4,
        };
        assert_eq!(knob.range(&project.vj), Some((0.0, 1.0)));
        let op = value_op(&project.vj, &knob, 0.3).unwrap();
        run(&mut project, op);
        assert!((project.vj.layers[1].opacity - 0.3).abs() < 1e-6);
        let source = crate::vj_mapping::Source::Osc {
            address: "/x".into(),
        };
        let add = VjOp::AddMapping {
            source,
            target: knob,
        };
        run(&mut project, add);
        assert_eq!(project.vj.mappings.len(), 1);
        run(
            &mut project,
            VjOp::RemoveLayer {
                layer: "id1".into(),
            },
        );
        let next = Target::LayerDashboard {
            layer: "id2".into(),
            knob: 4,
        };
        assert_eq!(
            project.vj.mappings[0].target, next,
            "a mapping follows the slot of its layer"
        );
    }

    #[test]
    fn a_scope_should_default_to_the_composition_when_an_op_leaves_it_out() {
        let op: VjOp =
            serde_json::from_str(r#"{"op":"dashboardValue","knob":1,"value":0.5}"#).unwrap();
        assert!(matches!(
            op,
            VjOp::DashboardValue {
                scope: Scope::Composition,
                ..
            }
        ));
        let op: VjOp = serde_json::from_str(
            r#"{"op":"dashboardValue","scope":{"kind":"clip","clip":"c1"},"knob":1,"value":0.5}"#,
        )
        .unwrap();
        assert!(matches!(
            op,
            VjOp::DashboardValue {
                scope: Scope::Clip { .. },
                ..
            }
        ));
    }
}
