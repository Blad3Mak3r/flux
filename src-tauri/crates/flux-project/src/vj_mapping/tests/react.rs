use super::*;

#[test]
fn a_cc_should_scale_into_the_range_of_its_mapping() {
    let mut project = show();
    let layer = layer(&project, 0).id.clone();
    let target = Target::LayerOpacity {
        layer: layer.clone(),
    };
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(7),
            target,
        },
    )
    .unwrap();
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.2,
            max: 0.8,
            invert: false,
            mode: ButtonMode::Trigger,
        },
    )
    .unwrap();
    let mut held = HashSet::new();

    let low = react(&project.vj, &heard(cc(7), 0.0, false), &mut held);
    let high = react(&project.vj, &heard(cc(7), 1.0, true), &mut held);

    assert_eq!(low[0].1, Command::Value(0.2));
    assert_eq!(high[0].1, Command::Value(0.8));
}

#[test]
fn invert_should_turn_the_source_round_inside_the_range() {
    let mapping = Mapping {
        id: "m".into(),
        source: cc(1),
        target: Target::Crossfader,
        min: 0.0,
        max: 1.0,
        invert: true,
        mode: ButtonMode::Trigger,
        follow: Follow::Id,
        place: None,
        parked: false,
        lights: None,
    };
    assert_eq!(mapping.scale(0.25), 0.75);
    let reversed = Mapping {
        min: 1.0,
        max: 0.0,
        invert: false,
        ..mapping
    };
    assert_eq!(
        reversed.scale(0.25),
        0.75,
        "a larger min is an inversion too"
    );
}

#[test]
fn a_pitch_bend_should_use_all_its_fourteen_bits() {
    let bend = Source::Midi {
        port: None,
        channel: 1,
        kind: MidiKind::PitchBend,
        number: 0,
    };
    let mapping = Mapping {
        id: "m".into(),
        source: bend,
        target: Target::Bpm,
        min: 100.0,
        max: 200.0,
        invert: false,
        mode: ButtonMode::Trigger,
        follow: Follow::Id,
        place: None,
        parked: false,
        lights: None,
    };
    // The decoder gives value / 16383 as the level.
    let centre = command(&mapping, 8192.0 / 16383.0, Edge::Same).unwrap();
    let top = command(&mapping, 16383.0 / 16383.0, Edge::Same).unwrap();
    let Command::Value(centre) = centre else {
        panic!("a value")
    };
    assert!((centre - 150.0).abs() < 0.01);
    assert_eq!(top, Command::Value(200.0));
}

#[test]
fn a_note_should_toggle_a_switch_and_a_held_one_should_follow_the_pad() {
    let mut project = show();
    let id = layer(&project, 1).id.clone();
    let target = Target::LayerBypass { layer: id };
    apply(
        &mut project,
        VjOp::AddMapping {
            source: note(36),
            target: target.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        project.vj.mappings[0].mode,
        ButtonMode::Toggle,
        "a switch starts as a toggle"
    );
    let mut held = HashSet::new();
    let on = react(&project.vj, &heard(note(36), 1.0, true), &mut held);
    let off = react(&project.vj, &heard(note(36), 0.0, false), &mut held);
    assert_eq!(on, vec![(target.clone(), Command::Toggle)]);
    assert!(off.is_empty(), "a toggle ignores the release");

    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.0,
            max: 1.0,
            invert: false,
            mode: ButtonMode::Hold,
        },
    )
    .unwrap();
    let down = react(&project.vj, &heard(note(36), 1.0, true), &mut held);
    let up = react(&project.vj, &heard(note(36), 0.0, false), &mut held);
    assert_eq!(down[0].1, Command::Switch(true));
    assert_eq!(up[0].1, Command::Switch(false));
}

#[test]
fn a_knob_sending_many_values_should_press_a_button_once() {
    let mut project = show();
    let target = Target::StopAll;
    apply(
        &mut project,
        VjOp::AddMapping {
            source: cc(20),
            target,
        },
    )
    .unwrap();
    let mut held = HashSet::new();
    let first = react(&project.vj, &heard(cc(20), 0.5, true), &mut held);
    let second = react(&project.vj, &heard(cc(20), 0.6, true), &mut held);
    let release = react(&project.vj, &heard(cc(20), 0.0, false), &mut held);
    assert_eq!(first[0].1, Command::Press);
    assert!(second.is_empty());
    assert_eq!(release[0].1, Command::Release);
}

#[test]
fn a_pad_mapped_to_a_fader_should_use_its_velocity_and_hold_should_drop_it() {
    let mut project = show();
    let target = Target::LayerMaster {
        layer: layer(&project, 0).id.clone(),
    };
    apply(
        &mut project,
        VjOp::AddMapping {
            source: note(40),
            target,
        },
    )
    .unwrap();
    let mut held = HashSet::new();
    let press = react(&project.vj, &heard(note(40), 0.5, true), &mut held);
    let release = react(&project.vj, &heard(note(40), 0.0, false), &mut held);
    assert_eq!(press[0].1, Command::Value(0.5));
    assert!(release.is_empty());
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.0,
            max: 1.0,
            invert: false,
            mode: ButtonMode::Hold,
        },
    )
    .unwrap();
    react(&project.vj, &heard(note(40), 1.0, true), &mut held);
    let release = react(&project.vj, &heard(note(40), 0.0, false), &mut held);
    assert_eq!(release[0].1, Command::Value(0.0));
}

#[test]
fn a_clip_launch_should_press_and_release_and_the_plan_should_launch_and_release() {
    let mut project = show();
    let clip = with_generator(&mut project);
    let target = Target::ClipLaunch { clip: clip.clone() };
    apply(
        &mut project,
        VjOp::AddMapping {
            source: note(48),
            target,
        },
    )
    .unwrap();
    let mut held = HashSet::new();
    let press = react(&project.vj, &heard(note(48), 1.0, true), &mut held);
    let release = react(&project.vj, &heard(note(48), 0.0, false), &mut held);
    let plan_of = |(target, command): &(Target, Command)| plan(&project.vj, target, *command);
    assert_eq!(
        plan_of(&press[0]),
        Some(Plan::Play(PlayAction::Launch(clip.clone())))
    );
    assert_eq!(
        plan_of(&release[0]),
        Some(Plan::Play(PlayAction::Release(clip)))
    );
}

#[test]
fn the_values_of_a_burst_should_coalesce_to_the_last_but_presses_stay() {
    let a = Target::Crossfader;
    let b = Target::Bpm;
    let out = coalesce(vec![
        (a.clone(), Command::Value(0.1)),
        (a.clone(), Command::Value(0.2)),
        (b.clone(), Command::Value(100.0)),
        (a.clone(), Command::Value(0.3)),
        (Target::StopAll, Command::Press),
        (Target::StopAll, Command::Press),
    ]);
    assert_eq!(
        out,
        vec![
            (a.clone(), Command::Value(0.2)),
            (b, Command::Value(100.0)),
            (a, Command::Value(0.3)),
            (Target::StopAll, Command::Press),
            (Target::StopAll, Command::Press),
        ]
    );
}

#[test]
fn a_source_should_match_the_port_by_name_only_when_both_have_one() {
    let heard = cc(7);
    let other_port = Source::Midi {
        port: Some("Launchpad".into()),
        channel: 1,
        kind: MidiKind::Cc,
        number: 7,
    };
    let any_port = Source::Midi {
        port: None,
        channel: 1,
        kind: MidiKind::Cc,
        number: 7,
    };
    assert!(heard.matches(&cc(7)));
    assert!(!other_port.matches(&heard));
    assert!(any_port.matches(&heard));
    assert!(!cc(8).matches(&heard));
    assert!(
        !Source::Osc {
            address: "/a".into()
        }
        .matches(&heard)
    );
}

#[test]
fn a_plan_should_flip_a_switch_from_its_state_and_step_decks_round() {
    let mut project = show();
    let id = layer(&project, 0).id.clone();
    let bypass = Target::LayerBypass { layer: id.clone() };
    let Some(Plan::Edit(op)) = plan(&project.vj, &bypass, Command::Toggle) else {
        panic!("an edit")
    };
    assert_eq!(
        op,
        VjOp::LayerBypass {
            layer: id.clone(),
            on: true
        }
    );
    apply(&mut project, op).unwrap();
    let Some(Plan::Edit(op)) = plan(&project.vj, &bypass, Command::Toggle) else {
        panic!("an edit")
    };
    assert_eq!(
        op,
        VjOp::LayerBypass {
            layer: id,
            on: false
        }
    );

    apply(&mut project, VjOp::AddDeck).unwrap();
    let first = project.vj.decks[0].id.clone();
    let Some(Plan::Edit(VjOp::SelectDeck { deck })) =
        plan(&project.vj, &Target::DeckNext, Command::Press)
    else {
        panic!("a deck")
    };
    assert_eq!(
        deck, first,
        "the second deck is shown, next wraps to the first"
    );
    assert_eq!(
        plan(&project.vj, &Target::Crossfader, Command::Value(2.0)),
        Some(Plan::Play(PlayAction::Crossfader(1.0)))
    );
    assert_eq!(plan(&project.vj, &Target::StopAll, Command::Release), None);
}

#[test]
fn the_autopilot_button_should_cycle_through_every_mode() {
    let mut mode = Autopilot::Off;
    let seen: Vec<Autopilot> = (0..4)
        .map(|_| {
            mode = mode.cycled();
            mode
        })
        .collect();
    assert_eq!(
        seen,
        [
            Autopilot::Next,
            Autopilot::Previous,
            Autopilot::Random,
            Autopilot::Off
        ]
    );
}
