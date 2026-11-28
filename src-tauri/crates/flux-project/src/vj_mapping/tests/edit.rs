use super::*;

#[test]
fn learning_should_replace_by_source_and_by_target_and_keep_the_options() {
    let mut project = show();
    let layer_id = layer(&project, 0).id.clone();
    let opacity = Target::LayerOpacity {
        layer: layer_id.clone(),
    };
    let master = Target::LayerMaster { layer: layer_id };
    let add = |project: &mut Project, source, target| {
        apply(project, VjOp::AddMapping { source, target }).unwrap();
    };
    add(&mut project, cc(1), opacity.clone());
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.1,
            max: 0.9,
            invert: true,
            mode: ButtonMode::Trigger,
        },
    )
    .unwrap();

    add(&mut project, cc(2), opacity.clone());
    assert_eq!(project.vj.mappings.len(), 1, "a target has one source");
    assert_eq!(project.vj.mappings[0].source, cc(2));
    assert_eq!(
        (project.vj.mappings[0].min, project.vj.mappings[0].invert),
        (0.1, true),
        "learning again keeps what was set"
    );

    add(&mut project, cc(2), master.clone());
    assert_eq!(project.vj.mappings.len(), 1, "a source has one target");
    assert_eq!(project.vj.mappings[0].target, master);
}

#[test]
fn a_target_that_is_gone_should_be_refused_and_removing_it_should_prune_its_mappings() {
    let mut project = show();
    let gone = Target::LayerOpacity {
        layer: "nope".into(),
    };
    assert!(
        apply(
            &mut project,
            VjOp::AddMapping {
                source: cc(1),
                target: gone
            }
        )
        .is_err()
    );

    let clip = with_generator(&mut project);
    let layer_id = layer(&project, 1).id.clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::ClipLaunch { clip: clip.clone() },
        },
    )
    .unwrap();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(2),
            target: Target::LayerMaster {
                layer: layer_id.clone(),
            },
        },
    )
    .unwrap();
    // A layer mapping follows its slot unless told to follow the layer itself.
    let mapping = project.vj.mappings[1].id.clone();
    apply(
        &mut project,
        VjOp::SetMappingFollow {
            mapping,
            follow: Follow::Id,
        },
    )
    .unwrap();
    apply(&mut project, VjOp::RemoveClip { clip }).unwrap();
    assert_eq!(project.vj.mappings.len(), 1);
    apply(&mut project, VjOp::RemoveLayer { layer: layer_id }).unwrap();
    assert!(project.vj.mappings.is_empty());
    project.validate().unwrap();
}

#[test]
fn validation_should_refuse_a_dangling_mapping_and_a_range_out_of_the_target() {
    let mut project = show();
    let layer_id = layer(&project, 0).id.clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::LayerOpacity {
                layer: layer_id.clone(),
            },
        },
    )
    .unwrap();
    project.validate().unwrap();
    let mut dangling = project.clone();
    dangling.vj.layers[0].id = "renamed".into();
    assert!(dangling.validate().is_err());
    let mut wide = project.clone();
    wide.vj.mappings[0].max = 3.0;
    assert!(wide.validate().is_err());
    let mut toggled = project;
    toggled.vj.mappings[0].mode = ButtonMode::Toggle;
    assert!(toggled.validate().is_err(), "only a switch toggles");
}

#[test]
fn a_mapping_should_survive_a_round_trip_and_an_old_file_should_have_none() {
    let mut project = show();
    let layer_id = layer(&project, 0).id.clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(7),
            target: Target::LayerOpacity { layer: layer_id },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&project.vj).unwrap();
    let back: Vj = serde_json::from_str(&json).unwrap();
    assert_eq!(back, project.vj);
    assert!(json.contains(r#""type":"midi""#));
    assert!(json.contains(r#""kind":"layerOpacity""#));

    let mut old = serde_json::to_value(&project.vj).unwrap();
    let object = old.as_object_mut().unwrap();
    object.remove("mappings");
    object.remove("dashboard");
    let old: Vj = serde_json::from_value(old).unwrap();
    assert!(old.mappings.is_empty() && old.dashboard.is_empty());
    let plain = serde_json::to_value(Vj::starting(String::new)).unwrap();
    assert!(
        plain.get("mappings").is_none(),
        "an empty list is not written"
    );
}

#[test]
fn effect_and_generator_numbers_should_become_edits_in_their_own_range() {
    let mut project = show();
    let clip = with_generator(&mut project);
    let layer_id = layer(&project, 0).id.clone();
    apply(
        &mut project,
        VjOp::AddEffect {
            target: crate::vj::EffectTarget::Layer(layer_id),
            kind: "blur".into(),
        },
    )
    .unwrap();
    let effect = project.vj.layers[0].effects[0].id.clone();
    let param = Target::EffectParam {
        effect: effect.clone(),
        param: "radius".into(),
    };
    let (lo, hi) = param.range(&project.vj).unwrap();
    let Some(Plan::Edit(op)) = plan(&project.vj, &param, Command::Value(hi)) else {
        panic!("an edit")
    };
    apply(&mut project, op).unwrap();
    assert!(lo < hi);
    let number = Target::GeneratorNumber {
        clip,
        name: "rate".into(),
    };
    let Some(Plan::Edit(op)) = plan(&project.vj, &number, Command::Value(20.0)) else {
        panic!("an edit")
    };
    apply(&mut project, op).unwrap();
    match &project.vj.layers[0].clips[0].generator {
        Some(crate::generator::Generator::Strobe { rate, .. }) => assert_eq!(*rate, 20.0),
        other => panic!("{other:?}"),
    }
    project.validate().unwrap();
}

#[test]
fn a_knob_should_move_every_link_into_its_own_range_in_one_edit() {
    let mut project = show();
    let a = layer(&project, 0).id.clone();
    let b = layer(&project, 1).id.clone();
    let link = |project: &mut Project, layer: &str, min, max| {
        apply(
            project,
            VjOp::DashboardLink {
                scope: Default::default(),
                knob: 2,
                target: Target::LayerOpacity {
                    layer: layer.into(),
                },
                min: Some(min),
                max: Some(max),
            },
        )
        .unwrap();
    };
    link(&mut project, &a, 0.0, 1.0);
    link(&mut project, &b, 1.0, 0.5);
    assert_eq!(
        project.vj.dashboard.len(),
        3,
        "the dashboard grows to the knob"
    );

    apply(
        &mut project,
        VjOp::DashboardValue {
            scope: Default::default(),
            knob: 2,
            value: 0.5,
        },
    )
    .unwrap();

    assert_eq!(layer(&project, 0).opacity, 0.5);
    assert_eq!(
        layer(&project, 1).opacity,
        0.75,
        "a larger min inverts the link"
    );
    assert_eq!(project.vj.dashboard[2].value, 0.5);
    project.validate().unwrap();
}

#[test]
fn a_knob_should_refuse_what_it_cannot_drive_and_a_ninth_knob() {
    let mut project = show();
    let id = layer(&project, 0).id.clone();
    let link = |target| VjOp::DashboardLink {
        scope: Default::default(),
        knob: 0,
        target,
        min: None,
        max: None,
    };
    assert!(apply(&mut project, link(Target::Crossfader)).is_err());
    assert!(apply(&mut project, link(Target::Dashboard { knob: 1 })).is_err());
    assert!(apply(&mut project, link(Target::StopAll)).is_err());
    assert!(
        apply(
            &mut project,
            VjOp::DashboardValue {
                scope: Default::default(),
                knob: KNOBS,
                value: 0.5
            }
        )
        .is_err()
    );
    apply(
        &mut project,
        link(Target::LayerMaster { layer: id.clone() }),
    )
    .unwrap();
    apply(&mut project, VjOp::RemoveLayer { layer: id }).unwrap();
    assert!(
        project.vj.dashboard[0].links.is_empty(),
        "removing the layer unlinks it"
    );
}

#[test]
fn a_fine_channel_should_be_set_for_a_dmx_mapping_only() {
    let mut project = show();
    let layer_id = layer(&project, 0).id.clone();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: artnet(1, 37, None),
            target: Target::LayerOpacity {
                layer: layer_id.clone(),
            },
        },
    )
    .unwrap();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::LayerMaster { layer: layer_id },
        },
    )
    .unwrap();
    let (dmx, midi) = (
        project.vj.mappings[0].id.clone(),
        project.vj.mappings[1].id.clone(),
    );

    apply(
        &mut project,
        VjOp::SetMappingFine {
            mapping: dmx.clone(),
            fine: Some(38),
        },
    )
    .unwrap();
    assert_eq!(project.vj.mappings[0].source, artnet(1, 37, Some(38)));
    assert!(
        apply(
            &mut project,
            VjOp::SetMappingFine {
                mapping: dmx.clone(),
                fine: Some(37)
            }
        )
        .is_err()
    );
    assert!(
        apply(
            &mut project,
            VjOp::SetMappingFine {
                mapping: midi,
                fine: Some(2)
            }
        )
        .is_err()
    );
    apply(
        &mut project,
        VjOp::SetMappingFine {
            mapping: dmx,
            fine: None,
        },
    )
    .unwrap();
    assert_eq!(project.vj.mappings[0].source, artnet(1, 37, None));
}
