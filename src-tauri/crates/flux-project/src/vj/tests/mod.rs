use super::*;
use crate::model::tests::video_project;

mod clip;
mod effects;
mod grid;
mod layer;
mod modulation;

fn ids() -> impl FnMut() -> String {
    let mut next = 0;
    move || {
        next += 1;
        format!("g{next}")
    }
}

/// A VJ project with the starting grid (layers g1 to g3, deck g4 with columns g5 to g12).
fn vj_project() -> (Project, impl FnMut() -> String) {
    let mut project = Project::new("Show", crate::FrameRate::Fps25);
    project.videos = video_project().videos;
    let mut new_id = ids();
    project.mode = ProjectMode::Vj;
    project.vj = Vj::starting(&mut new_id);
    (project, new_id)
}

fn run(project: &mut Project, op: VjOp, new_id: impl FnMut() -> String) {
    apply(project, op, new_id).unwrap();
    project.validate().unwrap();
}

fn place(project: &mut Project, layer: &str, column: &str, new_id: impl FnMut() -> String) {
    let op = VjOp::PlaceVideo {
        asset: "v1".into(),
        layer: layer.into(),
        column: column.into(),
    };
    run(project, op, new_id);
}

fn add_effect(
    project: &mut Project,
    target: EffectTarget,
    kind: &str,
    new_id: impl FnMut() -> String,
) {
    let op = VjOp::AddEffect {
        target,
        kind: kind.into(),
    };
    run(project, op, new_id);
}

fn sine(min: f32, max: f32) -> Modulation {
    use crate::vj_effects::Pace;
    use crate::vj_modulation::{Shape, Source};
    Modulation {
        source: Source::Lfo {
            shape: Shape::Sine,
            pace: Pace {
                sync: Sync::Beats,
                rate: 0.5,
                beats: 1.0,
            },
            phase: 0.0,
        },
        min,
        max,
    }
}
