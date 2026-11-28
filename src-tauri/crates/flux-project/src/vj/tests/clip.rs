use super::*;

#[test]
fn place_video_should_replace_the_clip_of_the_cell() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    place(&mut project, "g1", "g5", &mut new_id);
    let clips = &project.vj.layers[0].clips;
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].id, "g14");
}

#[test]
fn place_video_should_refuse_an_unknown_video() {
    let (mut project, mut new_id) = vj_project();
    let op = VjOp::PlaceVideo {
        asset: "nope".into(),
        layer: "g1".into(),
        column: "g5".into(),
    };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn a_generator_should_be_placed_edited_and_saved_without_a_video() {
    let (mut project, mut new_id) = vj_project();
    let strobe = Generator::Strobe {
        colour: "#ff0000".into(),
        rate: 4.0,
        width: 0.25,
    };
    let op = VjOp::PlaceGenerator {
        generator: Generator::Solid {
            colour: "#ffffff".into(),
        },
        layer: "g1".into(),
        column: "g5".into(),
    };
    run(&mut project, op, &mut new_id);
    let clip = project.vj.layers[0].clips[0].id.clone();
    let op = VjOp::ClipGenerator {
        clip: clip.clone(),
        generator: strobe.clone(),
    };
    run(&mut project, op, &mut new_id);
    let json = serde_json::to_value(&project.vj.layers[0].clips[0]).unwrap();
    assert!(json.get("asset").is_none(), "{json}");
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(back.generator, Some(strobe));
    project.vj.layers[0].clips[0].generator = Some(Generator::Noise {
        colour: "#ffffff".into(),
        rate: 0.0,
    });
    assert!(project.validate().is_err());
    project.vj.layers[0].clips[0].generator = None;
    assert!(project.validate().is_err(), "a clip without a video");
}

#[test]
fn clip_generator_should_refuse_a_video_clip() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    let op = VjOp::ClipGenerator {
        clip: project.vj.layers[0].clips[0].id.clone(),
        generator: Generator::Bars,
    };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn text_line_should_step_a_text_block_and_refuse_anything_else() {
    let (mut project, mut new_id) = vj_project();
    let text: Generator = serde_json::from_value(
        serde_json::json!({ "kind": "text", "text": "a\n\nb", "lines": "one" }),
    )
    .unwrap();
    let op = VjOp::PlaceGenerator {
        generator: text,
        layer: "g1".into(),
        column: "g5".into(),
    };
    run(&mut project, op, &mut new_id);
    let clip = project.vj.layers[0].clips[0].id.clone();
    let step = |project: &mut Project, new_id: &mut dyn FnMut() -> String, step: i32| {
        let op = VjOp::TextLine {
            clip: clip.clone(),
            step,
        };
        apply(project, op, new_id).unwrap();
        match &project.vj.layers[0].clips[0].generator {
            Some(Generator::Text { line, .. }) => *line,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(step(&mut project, &mut new_id, 1), 2);
    assert_eq!(step(&mut project, &mut new_id, 1), 0, "wraps");
    assert_eq!(step(&mut project, &mut new_id, -1), 2);
    project.vj.layers[0].clips[0].generator = Some(Generator::Bars);
    let op = VjOp::TextLine { clip, step: 1 };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn a_clip_should_scale_and_move_within_its_limits_and_keep_them_in_the_file() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let clip = |project: &Project| project.vj.layers[0].clips[0].clone();
    assert_eq!(
        (clip(&project).scale, clip(&project).position),
        (1.0, [0.0, 0.0])
    );
    let scale = |value| VjOp::ClipScale {
        clip: "g13".into(),
        value,
    };
    run(&mut project, scale(50.0), &mut new_id);
    assert_eq!(clip(&project).scale, MAX_SCALE);
    run(&mut project, scale(0.5), &mut new_id);
    let op = VjOp::ClipPosition {
        clip: "g13".into(),
        x: 0.25,
        y: -9.0,
    };
    run(&mut project, op, &mut new_id);
    assert_eq!(clip(&project).position, [0.25, -MAX_OFFSET]);
    assert!(apply(&mut project, scale(f32::NAN), &mut new_id).is_err());
    project.vj.layers[0].clips[0].position[1] = f32::NAN;
    assert!(project.validate().is_err(), "a file can carry anything");
    project.vj.layers[0].clips[0].position[1] = -MAX_OFFSET;

    let json = serde_json::to_value(clip(&project)).unwrap();
    assert_eq!(
        (json["scale"].as_f64(), json["position"][0].as_f64()),
        (Some(0.5), Some(0.25))
    );
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(back, clip(&project));
}

fn transport(project: &Project) -> &Clip {
    project.vj.find_clip("g13").unwrap().1
}

#[test]
fn a_clip_should_start_with_the_transport_of_a_plain_clip_and_store_none_of_it() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let clip = transport(&project);
    assert_eq!(clip.speed, 1.0);
    assert_eq!(clip.direction, Direction::Forward);
    assert_eq!(clip.tempo, Tempo::Free);
    assert_eq!(clip.region, Region::default());
    assert_eq!(clip.cues, [None; CUES]);
    let json = serde_json::to_value(clip).unwrap();
    assert_eq!(clip.duration, DEFAULT_DURATION);
    for field in ["speed", "duration", "direction", "tempo", "region", "cues"] {
        assert!(json.get(field).is_none(), "{field} is left out");
    }
    // A clip written before the transport has the same.
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(&back, clip);
}

#[test]
fn the_transport_ops_should_set_bring_into_range_and_round_trip() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let clip = || "g13".to_owned();
    run(
        &mut project,
        VjOp::ClipSpeed {
            clip: clip(),
            value: 50.0,
        },
        &mut new_id,
    );
    assert_eq!(transport(&project).speed, MAX_SPEED);
    run(
        &mut project,
        VjOp::ClipSpeed {
            clip: clip(),
            value: 0.0,
        },
        &mut new_id,
    );
    assert_eq!(transport(&project).speed, MIN_SPEED);
    assert!(
        apply(
            &mut project,
            VjOp::ClipSpeed {
                clip: clip(),
                value: f32::NAN
            },
            &mut new_id
        )
        .is_err()
    );
    run(
        &mut project,
        VjOp::ClipDirection {
            clip: clip(),
            direction: Direction::Reverse,
        },
        &mut new_id,
    );
    run(
        &mut project,
        VjOp::ClipTempo {
            clip: clip(),
            tempo: Tempo::Beats { beats: 5 },
        },
        &mut new_id,
    );
    assert_eq!(transport(&project).tempo, Tempo::Beats { beats: 4 });
    run(
        &mut project,
        VjOp::ClipRegion {
            clip: clip(),
            start: 0.3,
            end: 0.2,
        },
        &mut new_id,
    );
    let region = transport(&project).region;
    assert_eq!(region.start, 0.3);
    assert!(region.span() >= MIN_REGION * 0.999, "a minimum span");
    run(
        &mut project,
        VjOp::ClipRegion {
            clip: clip(),
            start: -2.0,
            end: 9.0,
        },
        &mut new_id,
    );
    assert_eq!(transport(&project).region, Region::default());
    for (index, position) in [(0, Some(0.25)), (7, Some(7.0)), (3, None)] {
        let op = VjOp::ClipCue {
            clip: clip(),
            index,
            position,
        };
        run(&mut project, op, &mut new_id);
    }
    let cues = transport(&project).cues;
    assert_eq!((cues[0], cues[7], cues[3]), (Some(0.25), Some(1.0), None));
    let outside = VjOp::ClipCue {
        clip: clip(),
        index: CUES,
        position: Some(0.5),
    };
    assert!(apply(&mut project, outside, &mut new_id).is_err());
    // What was set is stored, and read back the same.
    let json = serde_json::to_value(transport(&project)).unwrap();
    assert_eq!(json["direction"], "reverse");
    assert_eq!(json["tempo"]["kind"], "beats");
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(&back, transport(&project));
}

#[test]
fn the_duration_op_should_bring_into_range_round_trip_and_refuse_what_is_not_a_number() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let duration = |value| VjOp::ClipDuration {
        clip: "g13".into(),
        value,
    };
    run(&mut project, duration(3.0), &mut new_id);
    assert_eq!(transport(&project).duration, 3.0);
    run(&mut project, duration(0.0), &mut new_id);
    assert_eq!(transport(&project).duration, MIN_DURATION);
    run(&mut project, duration(1e9), &mut new_id);
    assert_eq!(transport(&project).duration, MAX_DURATION);
    assert!(apply(&mut project, duration(f32::NAN), &mut new_id).is_err());
    run(&mut project, duration(2.5), &mut new_id);
    let json = serde_json::to_value(transport(&project)).unwrap();
    assert_eq!(json["duration"].as_f64(), Some(2.5));
    let back: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(&back, transport(&project));
    let mut without = serde_json::to_value(transport(&project)).unwrap();
    without.as_object_mut().unwrap().remove("duration");
    let old: Clip = serde_json::from_value(without).unwrap();
    assert_eq!(
        old.duration, DEFAULT_DURATION,
        "an old file has the default"
    );
}

#[test]
fn validate_should_refuse_a_transport_out_of_range() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    let broken = |change: &dyn Fn(&mut Clip)| {
        let mut project = project.clone();
        change(project.vj.clip_mut("g13").unwrap());
        project.validate().is_err()
    };
    assert!(broken(&|c| c.speed = 9.0));
    assert!(broken(&|c| c.speed = 0.0));
    assert!(broken(&|c| c.duration = 0.0));
    assert!(broken(&|c| c.duration = MAX_DURATION + 1.0));
    assert!(broken(&|c| c.duration = f32::NAN));
    assert!(broken(&|c| c.tempo = Tempo::Beats { beats: 3 }));
    assert!(broken(&|c| c.region = Region {
        start: 0.5,
        end: 0.5
    }));
    assert!(broken(&|c| c.region = Region {
        start: 0.1,
        end: 1.5
    }));
    assert!(broken(&|c| c.cues[2] = Some(1.5)));
    assert!(!broken(&|c| c.tempo = Tempo::Beats { beats: 128 }));
}

#[test]
fn move_clip_should_swap_with_the_clip_already_in_the_cell() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    place(&mut project, "g2", "g6", &mut new_id); // g14
    let op = VjOp::MoveClip {
        clip: "g13".into(),
        layer: "g2".into(),
        column: "g6".into(),
    };
    run(&mut project, op, &mut new_id);
    let (layer, clip) = project.vj.find_clip("g13").unwrap();
    assert_eq!(
        (layer.id.as_str(), clip.column.as_deref()),
        ("g2", Some("g6"))
    );
    let (layer, clip) = project.vj.find_clip("g14").unwrap();
    assert_eq!(
        (layer.id.as_str(), clip.column.as_deref()),
        ("g1", Some("g5"))
    );
}

#[test]
fn move_clip_should_turn_a_column_into_a_slot_on_a_global_layer() {
    let (mut project, mut new_id) = vj_project();
    run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
    place(&mut project, "g1", "g8", &mut new_id); // g14
    let op = VjOp::MoveClip {
        clip: "g14".into(),
        layer: "g13".into(),
        column: "g9".into(),
    };
    run(&mut project, op, &mut new_id);
    let (layer, clip) = project.vj.find_clip("g14").unwrap();
    assert_eq!((layer.id.as_str(), clip.slot), ("g13", Some(4)));
}
