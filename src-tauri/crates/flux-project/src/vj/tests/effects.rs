use super::*;

#[test]
fn effects_should_be_added_moved_bypassed_and_removed_on_clips_and_layers() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let clip = || EffectTarget::Clip("g13".into());
    add_effect(&mut project, clip(), "brightness", &mut new_id); // g14
    add_effect(&mut project, clip(), "mirror", &mut new_id); // g15
    add_effect(
        &mut project,
        EffectTarget::Layer("g1".into()),
        "blur",
        &mut new_id,
    ); // g16
    let names = |project: &Project| -> Vec<String> {
        project.vj.layers[0].clips[0]
            .effects
            .iter()
            .map(|e| e.kind.name())
            .collect()
    };
    assert_eq!(names(&project), ["brightness", "mirror"]);
    assert_eq!(project.vj.layers[0].effects[0].id, "g16");

    let move_to = |effect: &str, index| VjOp::MoveEffect {
        effect: effect.into(),
        index,
    };
    run(&mut project, move_to("g15", 0), &mut new_id);
    assert_eq!(names(&project), ["mirror", "brightness"]);
    run(&mut project, move_to("g15", 99), &mut new_id);
    assert_eq!(names(&project), ["brightness", "mirror"]);

    let bypass = VjOp::EffectBypass {
        effect: "g14".into(),
        bypass: true,
    };
    run(&mut project, bypass, &mut new_id);
    assert!(project.vj.layers[0].clips[0].effects[0].bypass);

    let remove = |effect: &str| VjOp::RemoveEffect {
        effect: effect.into(),
    };
    run(&mut project, remove("g14"), &mut new_id);
    run(&mut project, remove("g16"), &mut new_id);
    assert_eq!(names(&project), ["mirror"]);
    assert!(project.vj.layers[0].effects.is_empty());
    assert!(apply(&mut project, remove("g14"), &mut new_id).is_err());
}

#[test]
fn an_effect_should_take_its_settings_in_range_and_keep_its_kind() {
    let (mut project, mut new_id) = vj_project();
    add_effect(
        &mut project,
        EffectTarget::Layer("g1".into()),
        "zoom",
        &mut new_id,
    ); // g13
    let params = |kind| VjOp::EffectParams {
        effect: "g13".into(),
        kind,
    };
    run(
        &mut project,
        params(EffectKind::Zoom { amount: 50.0 }),
        &mut new_id,
    );
    assert_eq!(
        project.vj.layers[0].effects[0].kind,
        EffectKind::Zoom {
            amount: crate::vj_effects::MAX_ZOOM
        }
    );
    let other = params(EffectKind::Hue { degrees: 10.0 });
    assert!(apply(&mut project, other, &mut new_id).is_err());
    let nan = params(EffectKind::Zoom { amount: f32::NAN });
    assert!(apply(&mut project, nan, &mut new_id).is_err());
    add_effect(
        &mut project,
        EffectTarget::Layer("g1".into()),
        "tint",
        &mut new_id,
    ); // g14
    let bad = VjOp::EffectParams {
        effect: "g14".into(),
        kind: EffectKind::Tint {
            colour: "orange".into(),
            amount: 0.5,
        },
    };
    assert!(apply(&mut project, bad, &mut new_id).is_err());
    let unknown = VjOp::AddEffect {
        target: EffectTarget::Layer("g1".into()),
        kind: "sparkle".into(),
    };
    assert!(apply(&mut project, unknown, &mut new_id).is_err());
}

#[test]
fn a_stack_should_hold_at_most_sixteen_effects() {
    let (mut project, mut new_id) = vj_project();
    for _ in 0..MAX_EFFECTS {
        add_effect(
            &mut project,
            EffectTarget::Layer("g1".into()),
            "invert",
            &mut new_id,
        );
    }
    let seventeenth = VjOp::AddEffect {
        target: EffectTarget::Layer("g1".into()),
        kind: "invert".into(),
    };
    assert!(apply(&mut project, seventeenth, &mut new_id).is_err());
    // Another stack is its own.
    add_effect(
        &mut project,
        EffectTarget::Layer("g2".into()),
        "invert",
        &mut new_id,
    );
    let repeated = project.vj.layers[0].effects[0].clone();
    project.vj.layers[0].effects.push(repeated);
    assert!(project.validate().is_err(), "seventeen, and a repeated id");
}

#[test]
fn the_composition_should_take_every_effect_and_the_history_ones_only_there() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let json = serde_json::to_string(&project).unwrap();
    assert!(!json.contains("compositionEffects") && !json.contains("compositionBypass"));
    // The kinds that need the frames before them are refused on a clip and on a layer.
    for kind in ["feedback", "trails", "bloom"] {
        for target in [
            EffectTarget::Clip("g13".into()),
            EffectTarget::Layer("g1".into()),
        ] {
            let op = VjOp::AddEffect {
                target,
                kind: kind.into(),
            };
            assert!(apply(&mut project, op, &mut new_id).is_err(), "{kind}");
        }
    }
    // Every kind goes on the composition, up to the limit of a stack.
    let names: Vec<&str> = EffectKind::names().collect();
    for kind in names.iter().take(MAX_EFFECTS) {
        add_effect(&mut project, EffectTarget::Composition, kind, &mut new_id);
    }
    let seventeenth = VjOp::AddEffect {
        target: EffectTarget::Composition,
        kind: "bloom".into(),
    };
    assert!(apply(&mut project, seventeenth, &mut new_id).is_err());
    // A stack of the composition is its own: the layers' are not counted in it.
    assert_eq!(project.vj.composition_effects.len(), MAX_EFFECTS);
    let hand_made = Effect {
        id: "z1".into(),
        bypass: false,
        modulation: Default::default(),
        kind: EffectKind::default_of("feedback").unwrap(),
    };
    project.vj.layers[0].effects.push(hand_made);
    assert!(project.validate().is_err(), "feedback on a layer");
}

#[test]
fn composition_effects_should_be_edited_like_any_other() {
    let (mut project, mut new_id) = vj_project();
    let composition = || EffectTarget::Composition;
    add_effect(&mut project, composition(), "feedback", &mut new_id); // g13
    add_effect(&mut project, composition(), "bloom", &mut new_id); // g14
    add_effect(&mut project, composition(), "invert", &mut new_id); // g15
    let ids = |project: &Project| -> Vec<String> {
        project
            .vj
            .composition_effects
            .iter()
            .map(|e| e.id.clone())
            .collect()
    };
    assert_eq!(ids(&project), ["g13", "g14", "g15"]);
    let move_to = VjOp::MoveEffect {
        effect: "g15".into(),
        index: 0,
    };
    run(&mut project, move_to, &mut new_id);
    assert_eq!(ids(&project), ["g15", "g13", "g14"]);
    // Settings are brought into range and keep their kind.
    let params = VjOp::EffectParams {
        effect: "g13".into(),
        kind: EffectKind::Feedback {
            amount: 3.0,
            zoom: 9.0,
            rotate: 10.0,
            x: 0.0,
            y: 0.0,
            hue: -500.0,
            blend: crate::vj_effects::FeedbackBlend::Add,
        },
    };
    run(&mut project, params, &mut new_id);
    let EffectKind::Feedback {
        amount, zoom, hue, ..
    } = project.vj.composition_effects[1].kind
    else {
        unreachable!()
    };
    assert_eq!((amount, zoom, hue), (1.0, 2.0, -180.0));
    let wrong = VjOp::EffectParams {
        effect: "g13".into(),
        kind: EffectKind::default_of("trails").unwrap(),
    };
    assert!(apply(&mut project, wrong, &mut new_id).is_err());
    // Bypass, duplicate, reset, modulation and removal find the effect in the composition.
    let bypass = VjOp::EffectBypass {
        effect: "g14".into(),
        bypass: true,
    };
    run(&mut project, bypass, &mut new_id);
    let duplicate = VjOp::DuplicateEffect {
        effect: "g14".into(),
    };
    run(&mut project, duplicate, &mut new_id); // g16
    assert_eq!(ids(&project), ["g15", "g13", "g14", "g16"]);
    let modulate = VjOp::SetModulation {
        target: ModTarget::Effect {
            effect: "g13".into(),
            param: "amount".into(),
        },
        modulation: Some(sine(0.2, 0.8)),
    };
    run(&mut project, modulate, &mut new_id);
    assert!(project.vj.is_modulated());
    let reset = VjOp::ResetEffect {
        effect: "g13".into(),
    };
    run(&mut project, reset, &mut new_id);
    assert!(project.vj.composition_effects[1].modulation.is_empty());
    let remove = VjOp::RemoveEffect {
        effect: "g15".into(),
    };
    run(&mut project, remove, &mut new_id);
    assert_eq!(ids(&project), ["g13", "g14", "g16"]);
    // The stack, its bypass and its effects are in the file, and only when there are any.
    let bypass_all = VjOp::CompositionBypass { bypass: true };
    run(&mut project, bypass_all, &mut new_id);
    let json = serde_json::to_string(&project).unwrap();
    assert!(json.contains(r#""compositionBypass":true"#), "{json}");
    assert!(json.contains(r#""compositionEffects":[{"id":"g13","effect":"feedback""#));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vj, project.vj);
    back.validate().unwrap();
    let off = VjOp::CompositionBypass { bypass: false };
    run(&mut project, off, &mut new_id);
    assert!(
        !serde_json::to_string(&project)
            .unwrap()
            .contains("compositionBypass")
    );
}

#[test]
fn validate_should_refuse_a_repeated_effect_id_and_a_setting_out_of_range() {
    let (mut project, mut new_id) = vj_project();
    add_effect(
        &mut project,
        EffectTarget::Layer("g1".into()),
        "blur",
        &mut new_id,
    ); // g13
    add_effect(
        &mut project,
        EffectTarget::Layer("g2".into()),
        "blur",
        &mut new_id,
    ); // g14
    project.vj.layers[1].effects[0].id = "g13".into();
    assert!(project.validate().is_err());
    project.vj.layers[1].effects[0].id = "g14".into();
    project.validate().unwrap();
    project.vj.layers[1].effects[0].kind = EffectKind::Blur { radius: 2.0 };
    assert!(project.validate().is_err());
    project.vj.layers[1].effects[0].kind = EffectKind::Blur { radius: f32::NAN };
    assert!(project.validate().is_err());
}

#[test]
fn effects_should_round_trip_in_the_file_and_be_absent_when_there_are_none() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let json = serde_json::to_string(&project).unwrap();
    assert!(!json.contains("effects"), "{json}");
    let clip = || EffectTarget::Clip("g13".into());
    // Every kind goes on the clip, its layer or another layer, 16 to each.
    let names: Vec<&str> = EffectKind::names()
        .filter(|name| !crate::vj_effects::spec_of(name).unwrap().composition_only)
        .collect();
    for (at, kind) in names.iter().enumerate() {
        let target = match at / MAX_EFFECTS {
            0 => clip(),
            1 => EffectTarget::Layer("g1".into()),
            _ => EffectTarget::Layer("g2".into()),
        };
        add_effect(&mut project, target, kind, &mut new_id);
    }
    // The tint is the fifth of the clip: g14 is the first effect.
    let tint = VjOp::EffectParams {
        effect: "g18".into(),
        kind: EffectKind::Tint {
            colour: "#102030".into(),
            amount: 0.25,
        },
    };
    run(&mut project, tint, &mut new_id);
    let bypass = VjOp::EffectBypass {
        effect: "g14".into(),
        bypass: true,
    };
    run(&mut project, bypass, &mut new_id);
    let json = serde_json::to_string(&project).unwrap();
    assert!(
        json.contains(r##"{"id":"g18","effect":"tint","colour":"#102030","amount":0.25}"##),
        "{json}"
    );
    assert!(
        json.contains(r#"{"id":"g14","bypass":true,"effect":"brightness","amount":0.0}"#),
        "{json}"
    );
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.vj, project.vj);
    back.validate().unwrap();
    let on_clip = back.vj.layers[0].clips[0].effects.len();
    let on_layers: usize = back.vj.layers.iter().map(|l| l.effects.len()).sum();
    assert_eq!(on_clip + on_layers, names.len());
}

#[test]
fn an_effect_should_be_duplicated_after_itself_and_reset_to_its_defaults() {
    let (mut project, mut new_id) = vj_project();
    let layer = || EffectTarget::Layer("g1".into());
    add_effect(&mut project, layer(), "twirl", &mut new_id); // g13
    add_effect(&mut project, layer(), "hue", &mut new_id); // g14
    let twirl = VjOp::EffectParams {
        effect: "g13".into(),
        kind: EffectKind::Twirl {
            angle: 40.0,
            radius: 0.2,
            x: 0.1,
            y: 0.9,
        },
    };
    run(&mut project, twirl.clone(), &mut new_id);
    let duplicate = VjOp::DuplicateEffect {
        effect: "g13".into(),
    };
    run(&mut project, duplicate, &mut new_id);
    let stack = &project.vj.layers[0].effects;
    assert_eq!(stack.len(), 3);
    assert_eq!(stack[1].kind, stack[0].kind);
    assert_ne!(stack[1].id, stack[0].id);
    assert_eq!(stack[2].id, "g14");
    let reset = VjOp::ResetEffect {
        effect: "g13".into(),
    };
    run(&mut project, reset, &mut new_id);
    let stack = &project.vj.layers[0].effects;
    assert_eq!(stack[0].kind, EffectKind::default_of("twirl").unwrap());
    assert_ne!(stack[1].kind, stack[0].kind);
    // A full stack takes no copy.
    while project.vj.layers[0].effects.len() < MAX_EFFECTS {
        add_effect(&mut project, layer(), "invert", &mut new_id);
    }
    let again = VjOp::DuplicateEffect {
        effect: "g13".into(),
    };
    assert!(apply(&mut project, again, &mut new_id).is_err());
    let missing = VjOp::ResetEffect {
        effect: "nope".into(),
    };
    assert!(apply(&mut project, missing, &mut new_id).is_err());
}
