use super::*;
use crate::cues::CueList;
use flux_project::{AudioAsset, AudioClip, FrameRate, Note, Project, Track, TrackKind};

fn trigger(kind: TriggerKind, channel: u8, number: u8) -> MidiTrigger {
    MidiTrigger {
        kind,
        channel,
        number,
    }
}

#[test]
fn a_note_on_should_be_a_trigger_on_its_channel() {
    assert_eq!(
        decode_midi(&[0x90, 60, 100]),
        Some(trigger(TriggerKind::Note, 1, 60))
    );
    assert_eq!(
        decode_midi(&[0x9F, 36, 1]),
        Some(trigger(TriggerKind::Note, 16, 36))
    );
}

#[test]
fn a_release_should_not_be_a_trigger() {
    assert_eq!(decode_midi(&[0x80, 60, 64]), None, "note off");
    assert_eq!(decode_midi(&[0x90, 60, 0]), None, "note on at velocity 0");
    assert_eq!(decode_midi(&[0xB0, 7, 0]), None, "control at 0");
}

#[test]
fn a_control_change_and_a_program_change_should_be_triggers() {
    assert_eq!(
        decode_midi(&[0xB2, 64, 127]),
        Some(trigger(TriggerKind::Cc, 3, 64))
    );
    assert_eq!(
        decode_midi(&[0xC0, 5]),
        Some(trigger(TriggerKind::Program, 1, 5))
    );
}

#[test]
fn midi_traffic_that_is_not_control_should_be_ignored() {
    for message in [
        &[0xF8][..],
        &[0xFE],
        &[0xE0, 0, 64],
        &[0xF0, 1, 2, 0xF7],
        &[0x90],
        &[0x90, 60],
        &[],
    ] {
        assert_eq!(decode_midi(message), None, "{message:02X?}");
    }
}

#[test]
fn a_pitch_bend_should_decode_to_fourteen_bits_with_the_low_byte_first() {
    let bend = decode_midi_message(&[0xE2, 0x7F, 0x7F]).unwrap();
    assert_eq!(
        (bend.kind, bend.channel, bend.value),
        (WireKind::PitchBend, 3, 16383)
    );
    assert_eq!(
        decode_midi_message(&[0xE0, 0x00, 0x40]).unwrap().value,
        8192
    );
    assert_eq!(decode_midi_message(&[0xE0, 0x01, 0x00]).unwrap().value, 1);
    assert_eq!(bend.trigger(), None, "a bend presses nothing of the cues");
}

#[test]
fn a_message_should_be_heard_as_a_level_with_the_port_it_came_from() {
    let cc = decode_midi_message(&[0xB0, 7, 127])
        .unwrap()
        .heard("APC40")
        .unwrap();
    assert_eq!(cc.level, 1.0);
    assert!(cc.down);
    assert_eq!(
        cc.source,
        Source::Midi {
            port: Some("APC40".into()),
            channel: 1,
            kind: MidiKind::Cc,
            number: 7
        }
    );
    let off = decode_midi_message(&[0x80, 36, 64])
        .unwrap()
        .heard("APC40")
        .unwrap();
    assert_eq!(
        (off.level, off.down),
        (0.0, false),
        "a note off is released"
    );
    let bend = decode_midi_message(&[0xE0, 0x00, 0x40])
        .unwrap()
        .heard("p")
        .unwrap();
    assert!((bend.level - 0.5).abs() < 0.001);
    assert!(
        decode_midi_message(&[0xC0, 5])
            .unwrap()
            .heard("p")
            .is_none()
    );
}

#[test]
fn a_data_byte_with_the_high_bit_set_should_be_refused() {
    assert_eq!(decode_midi(&[0x90, 0x80, 100]), None);
    assert_eq!(decode_midi(&[0x90, 60, 0x90]), None);
}

#[test]
fn the_first_binding_of_a_trigger_should_stay() {
    let pad = trigger(TriggerKind::Note, 1, 36);
    let other = trigger(TriggerKind::Note, 1, 37);
    let bindings = dedup(vec![
        Binding {
            trigger: pad,
            action: ControlAction::Play,
            feedback: None,
        },
        Binding {
            trigger: pad,
            action: ControlAction::Stop,
            feedback: None,
        },
        Binding {
            trigger: other,
            action: ControlAction::Stop,
            feedback: None,
        },
    ]);
    assert_eq!(bindings.len(), 2);
    assert_eq!(action_for(&bindings, pad), Some(ControlAction::Play));
    assert_eq!(action_for(&bindings, other), Some(ControlAction::Stop));
    assert_eq!(action_for(&bindings, trigger(TriggerKind::Cc, 1, 36)), None);
}

#[test]
fn a_binding_should_round_trip_through_json() {
    let binding = Binding {
        trigger: trigger(TriggerKind::Note, 2, 40),
        action: ControlAction::GotoCue(3),
        feedback: None,
    };
    let json = serde_json::to_string(&binding).unwrap();
    assert_eq!(
        json,
        r#"{"trigger":{"kind":"note","channel":2,"number":40},"action":{"type":"gotoCue","value":3}}"#
    );
    assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), binding);
    let unit = serde_json::to_string(&ControlAction::NextCue).unwrap();
    assert_eq!(unit, r#"{"type":"nextCue"}"#);
}

#[test]
fn a_binding_without_feedback_should_load_and_one_with_it_should_keep_it() {
    let old = r#"{"trigger":{"kind":"note","channel":1,"number":36},"action":{"type":"play"}}"#;
    let binding = serde_json::from_str::<Binding>(old).unwrap();
    assert_eq!(binding.feedback, None);
    assert_eq!(serde_json::to_string(&binding).unwrap(), old);

    let with = Binding {
        feedback: Some(Feedback { on: 3, off: 0 }),
        ..binding
    };
    let json = serde_json::to_string(&with).unwrap();
    assert!(json.ends_with(r#""feedback":{"on":3,"off":0}}"#), "{json}");
    assert_eq!(serde_json::from_str::<Binding>(&json).unwrap(), with);
}

fn message(address: &str, args: &[OscArg]) -> Vec<u8> {
    crate::osc::encode(address, args)
}

#[test]
fn an_encoded_message_should_decode_back() {
    let args = [
        OscArg::Int(-3),
        OscArg::Float(1.5),
        OscArg::String("go".into()),
        OscArg::Bool(true),
        OscArg::Bool(false),
    ];
    assert_eq!(
        decode_osc(&message("/flux/cue/goto", &args)),
        vec![OscMessage {
            address: "/flux/cue/goto".into(),
            args: args.to_vec()
        }]
    );
}

#[test]
fn a_message_without_type_tags_should_have_no_arguments() {
    let mut packet = b"/flux/play\0\0".to_vec();
    packet.truncate(12);
    assert_eq!(
        decode_osc(&packet),
        vec![OscMessage {
            address: "/flux/play".into(),
            args: vec![]
        }]
    );
}

#[test]
fn a_bundle_should_give_every_message_in_it() {
    let first = message("/flux/play", &[]);
    let second = message("/flux/cue/goto", &[OscArg::Int(2)]);
    let mut bundle = b"#bundle\0".to_vec();
    bundle.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    for element in [&first, &second] {
        bundle.extend_from_slice(&i32::try_from(element.len()).unwrap().to_be_bytes());
        bundle.extend_from_slice(element);
    }
    let addresses: Vec<_> = decode_osc(&bundle)
        .into_iter()
        .map(|message| message.address)
        .collect();
    assert_eq!(addresses, ["/flux/play", "/flux/cue/goto"]);
}

#[test]
fn a_malformed_packet_should_give_nothing() {
    assert!(decode_osc(&[]).is_empty());
    assert!(decode_osc(b"flux\0\0\0\0").is_empty(), "no leading slash");
    assert!(decode_osc(b"/flux/play").is_empty(), "no terminator");
    let mut truncated = message("/flux/seek/frames", &[OscArg::Int(5)]);
    truncated.truncate(truncated.len() - 2);
    assert!(decode_osc(&truncated).is_empty(), "argument cut short");
    let mut huge = b"#bundle\0".to_vec();
    huge.extend_from_slice(&[0; 8]);
    huge.extend_from_slice(&i32::MAX.to_be_bytes());
    assert!(
        decode_osc(&huge).is_empty(),
        "element larger than the packet"
    );
    let mut unknown = b"/flux/play\0\0,h\0\0".to_vec();
    unknown.extend_from_slice(&[0; 8]);
    assert!(
        decode_osc(&unknown).is_empty(),
        "an argument of unknown size"
    );
}

fn action(address: &str, args: &[OscArg]) -> Option<ControlAction> {
    osc_action(&OscMessage {
        address: address.into(),
        args: args.to_vec(),
    })
}

#[test]
fn the_fixed_addresses_should_map_to_their_actions() {
    for (address, expected) in [
        ("/flux/play", ControlAction::Play),
        ("/flux/pause", ControlAction::Pause),
        ("/flux/toggle", ControlAction::Toggle),
        ("/flux/stop", ControlAction::Stop),
        ("/flux/cue/next", ControlAction::NextCue),
        ("/flux/cue/prev", ControlAction::PrevCue),
        ("/flux/timeline/next", ControlAction::NextTimeline),
        ("/flux/timeline/prev", ControlAction::PrevTimeline),
        ("/flux/view/edit", ControlAction::ViewEdit),
        ("/flux/view/live", ControlAction::ViewLive),
    ] {
        assert_eq!(action(address, &[]), Some(expected), "{address}");
    }
}

#[test]
fn a_button_argument_should_be_ignored_by_actions_without_parameters() {
    assert_eq!(
        action("/flux/play", &[OscArg::Int(1)]),
        Some(ControlAction::Play)
    );
    assert_eq!(
        action("/flux/stop", &[OscArg::Float(1.0)]),
        Some(ControlAction::Stop)
    );
}

#[test]
fn actions_with_a_number_should_take_an_integer_a_float_or_a_string() {
    assert_eq!(
        action("/flux/cue/goto", &[OscArg::Int(4)]),
        Some(ControlAction::GotoCue(4))
    );
    assert_eq!(
        action("/flux/cue/goto", &[OscArg::Float(3.0)]),
        Some(ControlAction::GotoCue(3))
    );
    assert_eq!(
        action("/flux/timeline/select", &[OscArg::String(" 2 ".into())]),
        Some(ControlAction::SelectTimeline(2))
    );
    assert_eq!(
        action("/flux/seek/seconds", &[OscArg::Int(-10)]),
        Some(ControlAction::StepSeconds(-10))
    );
    assert_eq!(
        action("/flux/seek/frames", &[OscArg::Float(1.6)]),
        Some(ControlAction::StepFrames(2))
    );
}

#[test]
fn a_missing_or_invalid_number_should_ask_for_nothing() {
    assert_eq!(action("/flux/cue/goto", &[]), None);
    assert_eq!(action("/flux/cue/goto", &[OscArg::Int(0)]), None, "from 1");
    assert_eq!(action("/flux/cue/goto", &[OscArg::Int(-1)]), None);
    assert_eq!(
        action("/flux/cue/goto", &[OscArg::String("one".into())]),
        None
    );
    assert_eq!(action("/flux/seek/frames", &[OscArg::Bool(true)]), None);
    assert_eq!(
        action("/flux/seek/frames", &[OscArg::Float(f32::NAN)]),
        None
    );
    assert_eq!(action("/flux/seek/frames", &[OscArg::Float(1.0e12)]), None);
}

#[test]
fn other_addresses_should_ask_for_nothing() {
    assert_eq!(action("/flux/dance", &[]), None);
    assert_eq!(action("/flux/", &[]), None);
    assert_eq!(action("/play", &[]), None);
    assert_eq!(action("/flux/Play", &[]), None, "case matters");
}

/// Clips `c1` at 0, `c2` at 100 on a muted-able track and a note `m1` at 50 on another, in a
/// 25 fps timeline that starts at frame 1000. Their numbers are 1, 2 and 3 by place.
fn cues(muted: bool) -> CueList {
    let mut project = Project::new("Show", FrameRate::Fps25);
    project.timelines[0].timecode.start = 1000;
    project.assets.push(AudioAsset {
        id: "a1".into(),
        name: "Intro".into(),
        file: "audio/intro.wav".into(),
        link: None,
        bpm: None,
    });
    let track = |id: &str, muted, kind| Track {
        audio_output: Default::default(),
        id: id.into(),
        name: id.into(),
        muted,
        gain_db: None,
        color: None,
        kind,
    };
    project.timelines[0].tracks.push(track(
        "t1",
        muted,
        TrackKind::Audio {
            clips: vec![
                AudioClip::new("c2", "a1", 100),
                AudioClip::new("c1", "a1", 0),
            ],
        },
    ));
    project.timelines[0].tracks.push(track(
        "t2",
        false,
        TrackKind::Notes {
            notes: vec![Note {
                id: "m1".into(),
                at: 50,
                length: None,
                text: "M1".into(),
            }],
        },
    ));
    CueList::build(&project, &project.timelines[0], |_| Some(1))
}

#[test]
fn next_should_be_the_first_cue_after_the_position() {
    let list = cues(false);
    assert_eq!(next_cue_frame(list.cues(), 0), Some(1000));
    assert_eq!(
        next_cue_frame(list.cues(), 1000),
        Some(1050),
        "not the same cue"
    );
    assert_eq!(next_cue_frame(list.cues(), 1060), Some(1100));
    assert_eq!(next_cue_frame(list.cues(), 1100), None, "after the last");
}

#[test]
fn next_and_previous_should_skip_muted_cues() {
    let list = cues(true);
    assert_eq!(next_cue_frame(list.cues(), 0), Some(1050));
    assert_eq!(next_cue_frame(list.cues(), 1050), None);
    assert_eq!(prev_cue_frame(list.cues(), 1200, 0), Some(1050));
}

#[test]
fn previous_should_go_to_the_cue_before_when_one_just_started() {
    let list = cues(false);
    assert_eq!(
        prev_cue_frame(list.cues(), 1100, 25),
        Some(1050),
        "just started"
    );
    assert_eq!(
        prev_cue_frame(list.cues(), 1130, 25),
        Some(1100),
        "restarts it"
    );
    assert_eq!(
        prev_cue_frame(list.cues(), 1000, 25),
        None,
        "before the first"
    );
}

#[test]
fn go_to_should_find_a_cue_by_its_number() {
    let list = cues(true);
    assert_eq!(
        goto_cue_frame(list.cues(), 1),
        Some(1000),
        "muted still a place"
    );
    assert_eq!(goto_cue_frame(list.cues(), 2), Some(1050));
    assert_eq!(goto_cue_frame(list.cues(), 3), Some(1100));
    assert_eq!(goto_cue_frame(list.cues(), 4), None);
    assert_eq!(goto_cue_frame(list.cues(), 0), None);
}

#[test]
fn the_actions_that_move_should_give_their_frame_and_the_rest_none() {
    let list = cues(false);
    let at = |action, position| seek_target(action, position, list.cues(), 1000, 25);
    assert_eq!(at(ControlAction::NextCue, 1000), Some(1050));
    assert_eq!(at(ControlAction::PrevCue, 1130), Some(1100));
    assert_eq!(at(ControlAction::GotoCue(2), 1000), Some(1050));
    assert_eq!(at(ControlAction::GotoCue(9), 1000), None);
    assert_eq!(at(ControlAction::StepFrames(-5), 1003), Some(1000));
    assert_eq!(at(ControlAction::StepSeconds(2), 1000), Some(1050));
    assert_eq!(at(ControlAction::StepSeconds(-2), 1030), Some(1000));
    assert_eq!(at(ControlAction::Play, 1000), None);
    assert_eq!(at(ControlAction::NextTimeline, 1000), None);
    assert_eq!(at(ControlAction::ViewLive, 1000), None);
}

#[test]
fn a_step_should_stay_inside_the_timeline() {
    assert_eq!(step(1500, 25, 1000, 2000), 1525);
    assert_eq!(step(1500, -25, 1000, 2000), 1475);
    assert_eq!(step(1010, -25, 1000, 2000), 1000, "not before the start");
    assert_eq!(step(1990, 25, 1000, 2000), 2000, "not past the end");
    assert_eq!(step(u64::MAX, i64::MAX, 0, 10), 10);
    assert_eq!(step(5, 1, 10, 3), 10, "an empty range stays at the first");
}
