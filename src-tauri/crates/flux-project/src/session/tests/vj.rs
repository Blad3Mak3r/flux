use super::super::*;
use flux_core::FrameRate;

#[test]
fn preview_vj_should_move_a_level_in_place_and_refuse_other_edits() {
    let mut session = Session::default();
    let layer = session
        .new_project_of("Show", FrameRate::Fps25, crate::ProjectMode::Vj)
        .vj
        .layers[0]
        .id
        .clone();
    let (before, _) = session.revision();
    let op = crate::VjOp::LayerMaster {
        layer: layer.clone(),
        value: 0.25,
    };
    session.preview_vj(op).unwrap();
    let grid = session.grid_copy().unwrap();
    assert_eq!(grid.layers[0].master, 0.25);
    assert!(session.revision().0 > before, "the screens see it");
    let add = crate::VjOp::AddLayer { global: false };
    assert!(session.preview_vj(add).is_err());
    let effect = session
        .edit_vj(crate::VjOp::AddEffect {
            target: crate::vj::EffectTarget::Layer(layer.clone()),
            kind: "blur".into(),
        })
        .unwrap()
        .project
        .vj
        .layers[0]
        .effects[0]
        .id
        .clone();
    let params = crate::VjOp::EffectParams {
        effect,
        kind: crate::vj::EffectKind::Blur { radius: 0.7 },
    };
    session.preview_vj(params).unwrap();
    assert_eq!(
        session.grid_copy().unwrap().layers[0].effects[0].kind,
        crate::vj::EffectKind::Blur { radius: 0.7 }
    );
    let remove = crate::VjOp::RemoveEffect {
        effect: session.grid_copy().unwrap().layers[0].effects[0].id.clone(),
    };
    assert!(session.preview_vj(remove).is_err());
    assert_eq!(session.grid_copy().unwrap().layers.len(), grid.layers.len());
}

#[test]
fn preview_vj_should_move_a_speed_and_a_region_but_not_a_direction_or_a_cue() {
    let mut session = Session::default();
    let project = session.new_project_of("Show", FrameRate::Fps25, crate::ProjectMode::Vj);
    let layer = project.vj.layers[0].id.clone();
    let column = project.vj.decks[0].columns[0].id.clone();
    // A new project has no video: a generator is a clip too.
    let state = session
        .edit_vj(crate::VjOp::PlaceGenerator {
            generator: crate::generator::Generator::Bars,
            layer,
            column,
        })
        .unwrap();
    let clip = state.project.vj.layers[0].clips[0].id.clone();
    session
        .preview_vj(crate::VjOp::ClipSpeed {
            clip: clip.clone(),
            value: 2.0,
        })
        .unwrap();
    session
        .preview_vj(crate::VjOp::ClipRegion {
            clip: clip.clone(),
            start: 0.25,
            end: 0.5,
        })
        .unwrap();
    let held = session.grid_copy().unwrap().layers[0].clips[0].clone();
    assert_eq!(held.speed, 2.0);
    assert_eq!((held.region.start, held.region.end), (0.25, 0.5));
    let direction = crate::VjOp::ClipDirection {
        clip: clip.clone(),
        direction: crate::vj::Direction::Reverse,
    };
    assert!(session.preview_vj(direction).is_err());
    let cue = crate::VjOp::ClipCue {
        clip,
        index: 0,
        position: Some(0.5),
    };
    assert!(session.preview_vj(cue).is_err());
}
