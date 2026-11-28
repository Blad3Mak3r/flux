//! Test helpers shared by the tests of the preset modules.

use serde_json::{Map, Value};

use super::*;
use crate::model::Project;
use crate::vj::{EffectKind, EffectTarget, ProjectMode, Vj, VjOp};

pub fn counter() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("id{n}")
    }
}

pub fn show() -> Project {
    let mut project = Project::new("Gira", crate::FrameRate::Fps30);
    project.mode = ProjectMode::Vj;
    project.vj = Vj::starting(counter());
    project
}

pub fn run(project: &mut Project, op: VjOp) {
    crate::vj::apply(project, op, counter_from(100)).unwrap();
}

/// Ids no other call of the tests gets.
pub fn counter_from(_: u32) -> impl FnMut() -> String {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);
    || {
        format!(
            "n{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
}

pub fn add_effect(project: &mut Project, target: EffectTarget, kind: &str) -> String {
    run(
        project,
        VjOp::AddEffect {
            target: target.clone(),
            kind: kind.into(),
        },
    );
    stack(&project.vj, &target)
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone()
}

pub fn tint(project: &mut Project, effect: &str, colour: &str, amount: f32) {
    let kind: EffectKind = serde_json::from_value(
        serde_json::json!({"effect": "tint", "colour": colour, "amount": amount}),
    )
    .unwrap();
    run(
        project,
        VjOp::EffectParams {
            effect: effect.into(),
            kind,
        },
    );
}

pub fn settings(project: &Project, effect: &str) -> Map<String, Value> {
    stacks(&project.vj)
        .flatten()
        .find(|e| e.id == effect)
        .unwrap()
        .kind
        .settings()
}

pub fn add_clip_layer(project: &mut Project) -> String {
    run(project, VjOp::AddLayer { global: false });
    project.vj.layers.last().unwrap().id.clone()
}
