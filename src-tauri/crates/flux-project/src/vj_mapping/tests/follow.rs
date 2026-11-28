use super::*;

fn move_clip(project: &mut Project, clip: &str, layer: usize, column: usize) {
    let (layer, column) = (
        project.vj.layers[layer].id.clone(),
        project.vj.decks[0].columns[column].id.clone(),
    );
    apply(
        project,
        VjOp::MoveClip {
            clip: clip.into(),
            layer,
            column,
        },
    )
    .unwrap();
}

#[test]
fn a_new_layer_mapping_should_follow_its_slot_and_a_clip_mapping_itself() {
    let mut project = show();
    let clip = with_generator(&mut project);
    let layer_id = layer(&project, 1).id.clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::LayerOpacity { layer: layer_id },
        },
    )
    .unwrap();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(2),
            target: Target::ClipLaunch { clip },
        },
    )
    .unwrap();

    assert_eq!(project.vj.mappings[0].follow, Follow::Position);
    assert_eq!(project.vj.mappings[1].follow, Follow::Id);
}

#[test]
fn a_slot_mapping_should_follow_a_layer_reorder_and_an_id_mapping_the_layer() {
    let mut project = show();
    let (bottom, middle) = (layer(&project, 0).id.clone(), layer(&project, 1).id.clone());
    for (cc_number, id) in [(1, &bottom), (2, &middle)] {
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(cc_number),
                target: Target::LayerOpacity { layer: id.clone() },
            },
        )
        .unwrap();
    }
    // The second mapping follows the layer, the first the slot.
    let by_layer = project.vj.mappings[1].id.clone();
    apply(
        &mut project,
        VjOp::SetMappingFollow {
            mapping: by_layer,
            follow: Follow::Id,
        },
    )
    .unwrap();

    apply(
        &mut project,
        VjOp::MoveLayer {
            layer: bottom.clone(),
            to: 2,
        },
    )
    .unwrap();

    let now = |n: usize| project.vj.layers[n].id.clone();
    assert_ne!(now(0), bottom, "the layers did move");
    assert_eq!(
        project.vj.mappings[0].target,
        Target::LayerOpacity { layer: now(0) },
        "slot 0 is whatever is there now"
    );
    assert_eq!(
        project.vj.mappings[1].target,
        Target::LayerOpacity { layer: middle },
        "this layer, wherever it went"
    );
    project.validate().unwrap();
}

#[test]
fn a_clip_mapping_by_slot_should_hit_what_a_swap_leaves_there_and_by_id_the_clip_itself() {
    let mut project = show();
    let first = with_generator(&mut project);
    // A second clip in the next column of the same layer.
    let (layer_id, column) = (
        layer(&project, 0).id.clone(),
        project.vj.decks[0].columns[1].id.clone(),
    );
    apply(
        &mut project,
        VjOp::PlaceGenerator {
            generator: crate::generator::Generator::Strobe {
                colour: "#ffffff".into(),
                rate: 8.0,
                width: 0.5,
            },
            layer: layer_id,
            column,
        },
    )
    .unwrap();
    let second = project.vj.layers[0]
        .clips
        .iter()
        .find(|c| c.id != first)
        .unwrap()
        .id
        .clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::ClipLaunch {
                clip: first.clone(),
            },
        },
    )
    .unwrap();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(2),
            target: Target::ClipSpeed {
                clip: first.clone(),
            },
        },
    )
    .unwrap();
    let slot = project.vj.mappings[1].id.clone();
    apply(
        &mut project,
        VjOp::SetMappingFollow {
            mapping: slot,
            follow: Follow::Position,
        },
    )
    .unwrap();

    // Move the first clip onto the second's cell: the two swap.
    move_clip(&mut project, &first, 0, 1);

    assert_eq!(
        project.vj.mappings[0].target,
        Target::ClipLaunch {
            clip: first.clone()
        },
        "this clip"
    );
    assert_eq!(
        project.vj.mappings[1].target,
        Target::ClipSpeed { clip: second },
        "the clip that is now in that cell"
    );
    project.validate().unwrap();
}

#[test]
fn a_slot_that_empties_should_park_its_mapping_and_a_clip_that_fills_it_should_wake_it() {
    let mut project = show();
    let clip = with_generator(&mut project);
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(2),
            target: Target::ClipSpeed { clip: clip.clone() },
        },
    )
    .unwrap();
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMappingFollow {
            mapping: id,
            follow: Follow::Position,
        },
    )
    .unwrap();

    move_clip(&mut project, &clip, 1, 3);
    assert!(project.vj.mappings[0].parked, "the cell is empty now");
    assert!(react(&project.vj, &heard(cc(2), 0.5, true), &mut HashSet::new()).is_empty());
    project.validate().unwrap();

    move_clip(&mut project, &clip, 0, 0);
    assert!(!project.vj.mappings[0].parked);
    assert_eq!(project.vj.mappings[0].target, Target::ClipSpeed { clip });
}

#[test]
fn a_follow_mode_should_be_refused_for_a_target_with_no_slot() {
    let mut project = show();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::Crossfader,
        },
    )
    .unwrap();
    let id = project.vj.mappings[0].id.clone();

    assert!(
        apply(
            &mut project,
            VjOp::SetMappingFollow {
                mapping: id,
                follow: Follow::Position
            }
        )
        .is_err()
    );
}
