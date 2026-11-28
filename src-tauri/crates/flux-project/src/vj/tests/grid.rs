use super::*;

#[test]
fn starting_grid_should_be_a_valid_vj_project() {
    let (project, _) = vj_project();
    assert_eq!(project.vj.layers.len(), DEFAULT_LAYERS);
    assert_eq!(project.vj.decks.len(), 1);
    assert_eq!(
        project.vj.shown_deck().unwrap().columns.len(),
        DEFAULT_COLUMNS
    );
    project.validate().unwrap();
}

#[test]
fn a_timeline_project_should_have_no_grid() {
    let mut project = video_project();
    assert!(apply(&mut project, VjOp::AddLayer { global: false }, ids()).is_err());
    project.vj = Vj::starting(ids());
    assert!(project.validate().is_err());
}

#[test]
fn a_vj_project_should_need_its_grid_and_a_shown_deck() {
    let (mut project, _) = vj_project();
    project.vj.deck = "nope".into();
    assert!(project.validate().is_err());
    project.vj = Vj::default();
    assert!(project.validate().is_err());
}

#[test]
fn a_global_layer_should_keep_its_clips_in_slots_across_decks() {
    let (mut project, mut new_id) = vj_project();
    run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
    place(&mut project, "g13", "g7", &mut new_id); // slot 2
    let clip = &project.vj.layers[3].clips[0];
    assert_eq!((clip.column.as_deref(), clip.slot), (None, Some(2)));

    run(&mut project, VjOp::AddDeck, &mut new_id); // g15, columns g16 to g23
    assert_eq!(project.vj.deck, "g15");
    place(&mut project, "g13", "g18", &mut new_id); // slot 2 again, from the other deck
    assert_eq!(project.vj.layers[3].clips.len(), 1);
    assert_eq!(project.vj.column_clips("g18").count(), DEFAULT_LAYERS);
}

#[test]
fn remove_deck_should_take_its_clips_and_show_another_deck() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id); // g13
    run(&mut project, VjOp::AddDeck, &mut new_id); // g14
    run(
        &mut project,
        VjOp::SelectDeck { deck: "g4".into() },
        &mut new_id,
    );
    run(
        &mut project,
        VjOp::RemoveDeck { deck: "g4".into() },
        &mut new_id,
    );
    assert_eq!(project.vj.deck, "g14");
    assert!(project.vj.layers[0].clips.is_empty());
    let op = VjOp::RemoveDeck { deck: "g14".into() };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn remove_column_should_take_its_clips_and_keep_the_last_column() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    run(
        &mut project,
        VjOp::RemoveColumn {
            column: "g5".into(),
        },
        &mut new_id,
    );
    assert!(project.vj.layers[0].clips.is_empty());
    project.vj.decks[0].columns.truncate(1);
    let last = project.vj.decks[0].columns[0].id.clone();
    let op = VjOp::RemoveColumn { column: last };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn move_layer_should_clamp_the_place() {
    let (mut project, mut new_id) = vj_project();
    let op = VjOp::MoveLayer {
        layer: "g1".into(),
        to: 99,
    };
    run(&mut project, op, &mut new_id);
    let order: Vec<_> = project.vj.layers.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(order, ["g2", "g3", "g1"]);
}

#[test]
fn a_collapsed_layer_should_be_kept_in_the_file_and_unfold_again() {
    let (mut project, mut new_id) = vj_project();
    let collapse = |on| VjOp::LayerCollapse {
        layer: "g1".into(),
        on,
    };
    run(&mut project, collapse(true), &mut new_id);
    let json = serde_json::to_string(&project.vj.layers[0]).unwrap();
    assert!(json.contains(r#""collapsed":true"#), "{json}");
    run(&mut project, collapse(false), &mut new_id);
    let json = serde_json::to_string(&project.vj.layers[0]).unwrap();
    assert!(!json.contains("collapsed"), "absent when unfolded: {json}");
}

#[test]
fn the_tempo_should_be_set_nudged_scaled_and_kept_in_range() {
    let (mut project, mut new_id) = vj_project();
    let bpm = |p: &Project| p.timelines[0].tempo.bpm;
    run(&mut project, VjOp::SetBpm { bpm: 128.456 }, &mut new_id);
    assert_eq!(bpm(&project), 128.46);
    run(&mut project, VjOp::NudgeBpm { by: -1.0 }, &mut new_id);
    assert_eq!(bpm(&project), 127.46);
    run(&mut project, VjOp::ScaleBpm { factor: 0.5 }, &mut new_id);
    assert_eq!(bpm(&project), 63.73);
    run(&mut project, VjOp::ScaleBpm { factor: 100.0 }, &mut new_id);
    assert_eq!(bpm(&project), MAX_BPM);
    let nan = VjOp::SetBpm { bpm: f64::NAN };
    assert!(apply(&mut project, nan, &mut new_id).is_err());
}

#[test]
fn tapping_should_give_the_tempo_of_the_last_run_of_taps() {
    assert_eq!(tapped_bpm(&[]), None);
    assert_eq!(tapped_bpm(&[1000.0]), None);
    // Half a second apart is 120 BPM; the tap long before starts no run with them.
    assert_eq!(
        tapped_bpm(&[0.0, 5000.0, 5500.0, 6000.0, 6500.0]),
        Some(120.0)
    );
    let (mut project, mut new_id) = vj_project();
    let taps = VjOp::TapBpm {
        taps: vec![0.0, 400.0, 800.0],
    };
    run(&mut project, taps, &mut new_id);
    assert_eq!(project.timelines[0].tempo.bpm, 150.0);
}

#[test]
fn the_clock_source_should_be_kept_in_the_file() {
    let (mut project, mut new_id) = vj_project();
    let json = serde_json::to_string(&project.vj).unwrap();
    assert!(!json.contains("clock"), "internal is absent: {json}");
    let ltc = VjOp::SetClock {
        source: ClockSource::Ltc,
    };
    run(&mut project, ltc, &mut new_id);
    let json = serde_json::to_string(&project.vj).unwrap();
    assert!(json.contains(r#""clock":"ltc""#), "{json}");
}

#[test]
fn link_should_be_kept_in_the_file_only_while_on() {
    let (mut project, mut new_id) = vj_project();
    assert!(!serde_json::to_string(&project.vj).unwrap().contains("link"));
    run(&mut project, VjOp::SetLink { on: true }, &mut new_id);
    let json = serde_json::to_string(&project.vj).unwrap();
    assert!(json.contains(r#""link":true"#), "{json}");
}

#[test]
fn rename_should_take_an_empty_name_back_to_the_default() {
    let (mut project, mut new_id) = vj_project();
    for (name, expected) in [(" Intro ", Some("Intro")), ("  ", None)] {
        let op = VjOp::RenameLayer {
            layer: "g1".into(),
            name: name.into(),
        };
        run(&mut project, op, &mut new_id);
        assert_eq!(project.vj.layers[0].name.as_deref(), expected);
    }
}

#[test]
fn validate_should_refuse_a_slot_on_a_deck_layer() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    project.vj.layers[0].clips[0].put(Cell::Slot(0));
    assert!(project.validate().is_err());
}

#[test]
fn validate_should_refuse_cue_content_in_a_vj_show() {
    let (project, _) = vj_project();
    let mut with_track = project.clone();
    with_track.timelines[0].tracks = video_project().timelines[0].tracks.clone();
    assert!(with_track.validate().is_err());
    let mut two = project.clone();
    two.timelines
        .push(Timeline::new("Other", crate::FrameRate::Fps25));
    assert!(two.validate().is_err());
}

#[test]
fn json_should_leave_out_a_timeline_project_without_a_grid() {
    let value = serde_json::to_value(video_project()).unwrap();
    assert!(value.get("mode").is_none());
    assert!(value.get("vj").is_none());
}

#[test]
fn json_should_round_trip_a_grid() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    let layer = &mut project.vj.layers[0];
    layer.crossfade = Some(Side::B);
    layer.blend = Blend::Screen;
    layer.transition = 1.5;
    layer.clips[0].playback = Playback::PingPong;
    layer.clips[0].trigger = Trigger::Piano;
    let json = serde_json::to_string(&project).unwrap();
    assert!(json.contains(r#""mode":"vj""#));
    assert!(json.contains(r#""trigger":"piano""#));
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back, project);
}

#[test]
fn used_assets_should_count_the_clips_of_the_grid() {
    let (mut project, mut new_id) = vj_project();
    place(&mut project, "g1", "g5", &mut new_id);
    assert!(project.used_assets().contains("v1"));
}
