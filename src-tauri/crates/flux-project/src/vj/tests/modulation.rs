use super::*;

fn set(target: ModTarget, modulation: Option<Modulation>) -> VjOp {
    VjOp::SetModulation { target, modulation }
}

#[test]
fn set_modulation_should_store_and_remove_it_on_effects_layers_and_clips() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    add_effect(
        &mut project,
        EffectTarget::Clip("g13".into()),
        "blur",
        &mut new_id,
    ); // g14
    let effect = || ModTarget::Effect {
        effect: "g14".into(),
        param: "radius".into(),
    };
    let layer = |field: &str| ModTarget::Layer {
        layer: "g1".into(),
        field: field.into(),
    };
    let clip = |field: &str| ModTarget::Clip {
        clip: "g13".into(),
        field: field.into(),
    };
    run(
        &mut project,
        set(effect(), Some(sine(0.0, 1.0))),
        &mut new_id,
    );
    run(
        &mut project,
        set(layer("opacity"), Some(sine(1.0, 0.2))),
        &mut new_id,
    );
    run(
        &mut project,
        set(clip("positionX"), Some(sine(-1.0, 1.0))),
        &mut new_id,
    );
    let layer0 = &project.vj.layers[0];
    assert_eq!(
        layer0.clips[0].effects[0].modulation["radius"],
        sine(0.0, 1.0)
    );
    assert_eq!(
        layer0.modulation["opacity"].min, 1.0,
        "min may be above max"
    );
    assert!(layer0.clips[0].modulation.contains_key("positionX"));
    // The stored values stay as they were.
    assert_eq!(layer0.opacity, 1.0);

    // Out of range is brought into it; a number that is not one is refused.
    run(
        &mut project,
        set(layer("master"), Some(sine(-3.0, 9.0))),
        &mut new_id,
    );
    let master = project.vj.layers[0].modulation["master"];
    assert_eq!((master.min, master.max), (0.0, 1.0));
    let mut nan = sine(0.0, 1.0);
    nan.max = f32::NAN;
    assert!(apply(&mut project, set(layer("master"), Some(nan)), &mut new_id).is_err());

    // Null removes.
    run(&mut project, set(layer("master"), None), &mut new_id);
    assert!(!project.vj.layers[0].modulation.contains_key("master"));
}

#[test]
fn a_generator_number_should_be_modulated_and_go_when_the_generator_changes_kind() {
    let (mut project, mut new_id) = vj_project();
    let op = VjOp::PlaceGenerator {
        generator: Generator::Text {
            text: "FLUX".into(),
            font: "Inter".into(),
            style: "Bold".into(),
            size: 0.12,
            colour: "#ffffff".into(),
            opacity: 1.0,
            align: Default::default(),
            position_x: 0.5,
            position_y: 0.5,
            rotation: 0.0,
            tracking: 0.0,
            leading: 0.0,
            lines: Default::default(),
            line: 0,
            advance: 0,
        },
        layer: "g1".into(),
        column: "g5".into(),
    };
    run(&mut project, op, &mut new_id);
    let id = project.vj.layers[0].clips[0].id.clone();
    let target = |field: &str| ModTarget::Clip {
        clip: id.clone(),
        field: field.into(),
    };
    run(
        &mut project,
        set(target("generator.rotation"), Some(sine(-9999.0, 90.0))),
        &mut new_id,
    );
    run(
        &mut project,
        set(target("generator.size"), Some(sine(0.1, 0.3))),
        &mut new_id,
    );
    let stored = &project.vj.layers[0].clips[0].modulation;
    assert_eq!(
        stored["generator.rotation"].min, -360.0,
        "brought into range"
    );
    // A strobe's rate is not a text block's number.
    assert!(
        apply(
            &mut project,
            set(target("generator.rate"), Some(sine(1.0, 2.0))),
            &mut new_id
        )
        .is_err()
    );
    assert!(
        apply(
            &mut project,
            set(target("generator.colour"), Some(sine(0.0, 1.0))),
            &mut new_id
        )
        .is_err()
    );
    project.validate().unwrap();
    // Another kind of generator keeps what it has too, and drops what it does not.
    let strobe = VjOp::ClipGenerator {
        clip: id.clone(),
        generator: Generator::Strobe {
            colour: "#ffffff".into(),
            rate: 8.0,
            width: 0.5,
        },
    };
    run(&mut project, strobe, &mut new_id);
    assert!(project.vj.layers[0].clips[0].modulation.is_empty());
    project.validate().unwrap();
}

#[test]
fn set_modulation_should_refuse_what_cannot_be_modulated() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    add_effect(
        &mut project,
        EffectTarget::Clip("g13".into()),
        "kaleidoscope",
        &mut new_id,
    ); // g14
    add_effect(
        &mut project,
        EffectTarget::Clip("g13".into()),
        "mirror",
        &mut new_id,
    ); // g15
    let effect = |id: &str, param: &str| ModTarget::Effect {
        effect: id.into(),
        param: param.into(),
    };
    let mut refused = |target: ModTarget| {
        assert!(apply(&mut project, set(target, Some(sine(0.0, 1.0))), &mut new_id).is_err());
    };
    // A choice, a rate, a length of beats, a setting the kind lacks, an unknown effect.
    refused(effect("g15", "axis"));
    refused(effect("g14", "rate"));
    refused(effect("g14", "beats"));
    refused(effect("g14", "nope"));
    refused(effect("nope", "x"));
    refused(ModTarget::Layer {
        layer: "g1".into(),
        field: "transition".into(),
    });
    refused(ModTarget::Clip {
        clip: "g13".into(),
        field: "opacity".into(),
    });
    refused(ModTarget::Layer {
        layer: "nope".into(),
        field: "master".into(),
    });
}

#[test]
fn modulation_should_follow_duplicate_and_clear_on_reset_and_survive_a_round_trip() {
    let (mut project, mut new_id) = vj_project();
    add_effect(
        &mut project,
        EffectTarget::Layer("g1".into()),
        "zoom",
        &mut new_id,
    ); // g13
    let target = ModTarget::Effect {
        effect: "g13".into(),
        param: "amount".into(),
    };
    run(&mut project, set(target, Some(sine(0.5, 2.0))), &mut new_id);
    let duplicate = VjOp::DuplicateEffect {
        effect: "g13".into(),
    };
    run(&mut project, duplicate, &mut new_id);
    let stack = &project.vj.layers[0].effects;
    assert_eq!(stack[1].modulation, stack[0].modulation);
    assert_eq!(stack[1].modulation.len(), 1);

    // Changing the setting keeps the modulation; the stored value is the base.
    let params = VjOp::EffectParams {
        effect: "g13".into(),
        kind: EffectKind::Zoom { amount: 1.5 },
    };
    run(&mut project, params, &mut new_id);
    assert_eq!(project.vj.layers[0].effects[0].modulation.len(), 1);

    let json = serde_json::to_string(&project).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vj, project.vj);
    back.validate().unwrap();
    // A plain effect writes no modulation key.
    let plain = serde_json::to_value(&project.vj.layers[1]).unwrap();
    assert!(plain.get("modulation").is_none());

    let reset = VjOp::ResetEffect {
        effect: "g13".into(),
    };
    run(&mut project, reset, &mut new_id);
    let stack = &project.vj.layers[0].effects;
    assert!(stack[0].modulation.is_empty());
    assert_eq!(
        stack[1].modulation.len(),
        1,
        "only the reset effect is cleared"
    );
}

#[test]
fn validate_should_refuse_a_modulation_out_of_range_or_on_a_wrong_field() {
    let (mut project, _) = vj_project();
    project.vj.layers[0]
        .modulation
        .insert("opacity".into(), sine(0.0, 2.0));
    assert!(project.validate().is_err());
    project.vj.layers[0].modulation.clear();
    project.vj.layers[0]
        .modulation
        .insert("blend".into(), sine(0.0, 1.0));
    assert!(project.validate().is_err());
    project.vj.layers[0].modulation.clear();
    project.vj.layers[0]
        .modulation
        .insert("volume".into(), sine(0.0, 1.0));
    project.validate().unwrap();
}

#[test]
fn a_project_saved_before_modulation_should_load_without_any() {
    let (project, _) = vj_project();
    let json = serde_json::to_string(&project).unwrap();
    assert!(!json.contains("modulation"));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert!(!back.vj.is_modulated());
}

#[test]
fn set_modulation_should_set_and_clear_a_knob_and_refuse_a_ninth() {
    let (mut project, new_id) = vj_project();
    let m = sine(0.1, 0.9);
    let knob = |n| ModTarget::Knob { knob: n };
    run(&mut project, set(knob(7), Some(m)), new_id);
    assert_eq!(project.vj.dashboard[7].modulation, Some(m));
    assert!(apply(&mut project, set(knob(8), Some(m)), String::new).is_err());
    run(&mut project, set(knob(7), None), String::new);
    assert_eq!(project.vj.dashboard[7].modulation, None);
}
