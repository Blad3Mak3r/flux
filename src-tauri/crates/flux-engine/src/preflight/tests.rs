use super::*;

#[test]
fn check_video_should_warn_when_off_and_block_when_the_display_is_gone() {
    let codes = |uses, chosen, connected| {
        let mut report = Report::default();
        report.check_video(uses, chosen, connected);
        report
            .issues
            .iter()
            .map(|issue| (issue.code, issue.blocking))
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(false, None, false), vec![]);
    assert_eq!(codes(true, Some("HDMI-1"), true), vec![]);
    assert_eq!(
        codes(true, None, false),
        vec![(IssueCode::VideoOutputOff, false)]
    );
    assert_eq!(
        codes(true, Some("HDMI-1"), false),
        vec![(IssueCode::VideoDisplayMissing, true)]
    );
}
use flux_project::{
    Alignment, FrameRate, GeneratedVoice, LtcOutput, MidiEvent, MidiMessage, MidiPortRef,
    MtcOutput, OscEvent, Track, VoiceOrder,
};

fn project() -> Project {
    let mut p = Project::new("Show", FrameRate::Fps25);
    p.voice = Some("voice".into());
    p.assets.push(AudioAsset {
        id: "audio".into(),
        name: "Go".into(),
        file: "audio/go.mp3".into(),
        link: None,
        bpm: None,
    });
    p.timelines[0].tracks.push(Track {
        audio_output: Default::default(),
        id: "operator".into(),
        name: "Operator".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind: TrackKind::Operator {
            voice: None,
            orders: vec![VoiceOrder {
                mode: Default::default(),
                synthesis_text: None,
                trim_start: 0,
                trim_end: 0,
                id: "first".into(),
                at: 25,
                dialogue: "Go".into(),
                alignment: Alignment::Start,
                generated: Some(GeneratedVoice {
                    origin: flux_project::operator::RecordingOrigin::Generated,
                    synthesis_text: None,
                    asset: "audio".into(),
                    dialogue: "Go".into(),
                    voice: "voice".into(),
                    model: flux_project::operator::VOICE_MODEL.into(),
                    samples: 48_000,
                }),
            }],
        },
    });
    p
}

const READY: RouteCheck = RouteCheck {
    available: true,
    ltc_conflict: false,
};
fn orders(p: &mut Project) -> &mut Vec<VoiceOrder> {
    let TrackKind::Operator { orders, .. } = &mut p.timelines[0].tracks[0].kind else {
        panic!()
    };
    orders
}
fn codes(r: &Report) -> Vec<IssueCode> {
    r.issues.iter().map(|i| i.code).collect()
}

#[test]
fn valid_show_is_clear_and_muted_tracks_are_excluded() {
    let mut p = project();
    let report = check(&p, None, READY, |_| Some(48_000));
    assert_eq!(report.orders, 1);
    assert!(report.issues.is_empty());
    p.timelines[0].tracks[0].muted = true;
    let report = check(
        &p,
        None,
        RouteCheck {
            available: false,
            ltc_conflict: true,
        },
        |_| None,
    );
    assert_eq!(report.orders, 0);
    assert!(report.issues.is_empty());
}

#[test]
fn content_audio_position_and_route_problems_are_independent() {
    let mut p = project();
    let order = &mut orders(&mut p)[0];
    order.dialogue = "A new draft".into();
    order.alignment = Alignment::End;
    order.at = 1;
    let report = check(
        &p,
        None,
        RouteCheck {
            available: false,
            ltc_conflict: true,
        },
        |_| Some(47_999),
    );
    assert_eq!(
        codes(&report),
        [
            IssueCode::Stale,
            IssueCode::MissingAudio,
            IssueCode::BeforeOrigin,
            IssueCode::MissingRoute,
            IssueCode::LtcConflict
        ]
    );
    assert_eq!((report.errors, report.warnings), (4, 1));
    assert_eq!(report.issues[0].name, "Go");
    orders(&mut p)[0].generated = None;
    let report = check(&p, None, READY, |_| unreachable!());
    assert_eq!(codes(&report), [IssueCode::Ungenerated]);
}

#[test]
fn overlaps_mark_each_order_once_and_adjacent_audio_is_allowed() {
    let mut p = project();
    let mut second = orders(&mut p)[0].clone();
    second.id = "second".into();
    second.at = 50;
    orders(&mut p).push(second.clone());
    assert!(check(&p, None, READY, |_| Some(48_000)).issues.is_empty());
    second.id = "third".into();
    second.at = 49;
    orders(&mut p).push(second);
    let report = check(&p, None, READY, |_| Some(48_000));
    assert_eq!(codes(&report), [IssueCode::Overlap; 3]);
    assert_eq!((report.errors, report.warnings), (0, 3));
}

#[test]
fn each_operator_track_uses_its_own_route_check_and_silent_is_explicit() {
    let mut p = project();
    let mut other = p.timelines[0].tracks[0].clone();
    other.id = "other".into();
    other.name = "Stage".into();
    if let TrackKind::Operator { orders, .. } = &mut other.kind {
        orders[0].id = "second".into();
        orders[0].at = 100;
    }
    p.timelines[0].tracks.push(other);
    let report = check_routes(
        &p,
        None,
        |_, track| {
            if track.id == "other" {
                RouteCheck {
                    available: false,
                    ltc_conflict: false,
                }
            } else {
                READY
            }
        },
        |_| Some(48_000),
    );
    assert_eq!((report.orders, report.ready, report.errors), (2, 1, 1));
    assert_eq!(report.issues[0].name, "Stage");
    p.timelines[0].tracks[1].audio_output = flux_project::routing::AudioAssignment::Silent;
    let report = check_routes(
        &p,
        None,
        |_, track| {
            if track.id == "other" {
                RouteCheck {
                    available: false,
                    ltc_conflict: false,
                }
            } else {
                READY
            }
        },
        |_| Some(48_000),
    );
    assert_eq!(codes(&report), [IssueCode::SilentOutput]);
    assert_eq!((report.errors, report.warnings), (0, 1));
}

fn fake_hardware() -> Hardware {
    let audio = |name: &str, output_channels, sample_rates: &[u32], is_default| AudioDevice {
        name: name.into(),
        output_channels,
        sample_rates: sample_rates.to_vec(),
        is_default,
    };
    Hardware {
        audio: vec![
            audio("Main", 8, &[44_100, 48_000], true),
            audio("Old card", 2, &[44_100], false),
        ],
        midi: ["Hub", "Hub", "Lights"]
            .map(|name| MidiPort { name: name.into() })
            .to_vec(),
    }
}

fn cue_project(kind: TrackKind) -> Project {
    let mut p = Project::new("Show", FrameRate::Fps25);
    p.timelines[0].tracks.push(Track {
        audio_output: Default::default(),
        id: "cues".into(),
        name: "Cues".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind,
    });
    p
}

fn midi_track(port: Option<(&str, usize)>, events: usize) -> TrackKind {
    TrackKind::Midi {
        port: port.map(|(name, index)| MidiPortRef {
            port_name: name.into(),
            port_index: index,
        }),
        events: (0..events)
            .map(|i| MidiEvent {
                first_turn_only: false,
                id: format!("m{i}"),
                at: 0,
                message: MidiMessage::ProgramChange {
                    channel: 1,
                    program: 1,
                },
            })
            .collect(),
    }
}

fn osc_track(destination: Option<&str>, events: usize) -> TrackKind {
    TrackKind::Osc {
        destination: destination.map(Into::into),
        events: (0..events)
            .map(|i| OscEvent {
                first_turn_only: false,
                id: format!("o{i}"),
                at: 0,
                address: "/go".into(),
                args: vec![],
            })
            .collect(),
    }
}

fn destination(host: &str, port: u16) -> OscDestination {
    OscDestination {
        id: "qlab".into(),
        name: "QLab".into(),
        host: host.into(),
        port,
    }
}

fn ltc(device: &str, channel: u16) -> LtcOutput {
    LtcOutput {
        device_name: device.into(),
        channel,
        level: None,
        offset_ms: None,
    }
}

fn mtc(port: &str, index: usize) -> MtcOutput {
    MtcOutput {
        port_name: port.into(),
        port_index: index,
        offset_ms: None,
    }
}

fn hardware_codes(p: &Project, output: &OutputConfig, hw: &Hardware) -> Vec<(IssueCode, bool)> {
    check_hardware(p, output, hw)
        .iter()
        .map(|i| (i.code, i.blocking))
        .collect()
}

#[test]
fn a_midi_track_needs_a_port_that_is_present() {
    let hw = fake_hardware();
    let out = OutputConfig::default();
    let check = |port| hardware_codes(&cue_project(midi_track(port, 1)), &out, &hw);
    assert!(check(Some(("Hub", 0))).is_empty());
    assert!(check(Some(("hub 24:1", 1))).is_empty());
    let missing = [(IssueCode::MidiPortMissing, true)];
    assert_eq!(check(Some(("Hub", 2))), missing);
    assert_eq!(check(Some(("Gone", 0))), missing);
    assert_eq!(check(None), missing);
    assert_eq!(
        hardware_codes(
            &cue_project(midi_track(Some(("Gone", 0)), 1)),
            &out,
            &Hardware::default()
        ),
        missing
    );
}

#[test]
fn an_osc_track_needs_a_defined_destination_with_a_valid_address() {
    let hw = fake_hardware();
    let check = |chosen: Option<&str>, host: &str, port| {
        let out = OutputConfig {
            osc: vec![destination(host, port)],
            ..Default::default()
        };
        hardware_codes(&cue_project(osc_track(chosen, 1)), &out, &hw)
    };
    assert!(check(Some("qlab"), "192.168.1.20", 53000).is_empty());
    assert!(check(Some("qlab"), "qlab.local", 53000).is_empty());
    assert!(check(Some("qlab"), "::1", 53000).is_empty());
    let missing = [(IssueCode::OscDestinationMissing, true)];
    assert_eq!(check(Some("other"), "192.168.1.20", 53000), missing);
    assert_eq!(check(None, "192.168.1.20", 53000), missing);
    let invalid = [(IssueCode::OscDestinationInvalid, true)];
    assert_eq!(check(Some("qlab"), "300.1.1.1", 53000), invalid);
    assert_eq!(check(Some("qlab"), "bad host!", 53000), invalid);
    assert_eq!(check(Some("qlab"), "-x.local", 53000), invalid);
    assert_eq!(check(Some("qlab"), "192.168.1.20", 0), invalid);
}

#[test]
fn a_track_with_no_events_is_a_warning_and_a_muted_track_is_skipped() {
    let hw = fake_hardware();
    let out = OutputConfig::default();
    let warned = [(IssueCode::EmptyTrack, false)];
    let mut p = cue_project(midi_track(Some(("Gone", 0)), 0));
    assert_eq!(hardware_codes(&p, &out, &hw), warned);
    p = cue_project(osc_track(None, 0));
    assert_eq!(hardware_codes(&p, &out, &hw), warned);
    p.timelines[0].tracks[0].muted = true;
    assert!(hardware_codes(&p, &out, &hw).is_empty());
}

#[test]
fn ltc_needs_a_present_device_channel_and_engine_rate() {
    let hw = fake_hardware();
    let p = Project::new("Show", FrameRate::Fps25);
    let check = |ltc: LtcOutput| {
        let out = OutputConfig {
            ltc: vec![ltc],
            ..Default::default()
        };
        hardware_codes(&p, &out, &hw)
    };
    assert!(check(ltc("Main", 8)).is_empty());
    assert!(check(ltc("", 1)).is_empty());
    let down = [(IssueCode::LtcUnavailable, true)];
    assert_eq!(check(ltc("Gone", 1)), down);
    assert_eq!(check(ltc("Main", 9)), down);
    assert_eq!(check(ltc("Main", 0)), down);
    assert_eq!(check(ltc("Old card", 1)), down);
    assert_eq!(check(ltc(UNASSIGNED_DEVICE, 1)), down);
}

#[test]
fn mtc_needs_a_present_port() {
    let hw = fake_hardware();
    let p = Project::new("Show", FrameRate::Fps25);
    let check = |mtc: MtcOutput| {
        let out = OutputConfig {
            mtc: vec![mtc],
            ..Default::default()
        };
        hardware_codes(&p, &out, &hw)
    };
    assert!(check(mtc("Lights", 0)).is_empty());
    assert!(check(mtc("Hub", 1)).is_empty());
    let down = [(IssueCode::MtcUnavailable, true)];
    assert_eq!(check(mtc("Hub", 2)), down);
    assert_eq!(check(mtc("", 0)), down);
    assert_eq!(check(mtc("Gone", 0)), down);
}

#[test]
fn outputs_on_the_same_channel_or_port_collide() {
    let hw = fake_hardware();
    let p = Project::new("Show", FrameRate::Fps25);
    let out = OutputConfig {
        ltc: vec![ltc("Main", 3), ltc("", 3), ltc("Main", 4)],
        mtc: vec![mtc("Hub", 0), mtc("hub", 0), mtc("Hub", 1)],
        ..Default::default()
    };
    assert_eq!(
        hardware_codes(&p, &out, &hw),
        [
            (IssueCode::LtcCollision, true),
            (IssueCode::MtcCollision, true)
        ]
    );
}

#[test]
fn with_no_devices_attached_every_configured_output_is_reported_and_nothing_fails() {
    let p = cue_project(midi_track(Some(("Hub", 0)), 1));
    let out = OutputConfig {
        ltc: vec![ltc("Main", 1)],
        mtc: vec![mtc("Hub", 0)],
        ..Default::default()
    };
    let mut report = check(&p, None, READY, |_| None);
    report.extend(check_hardware(&p, &out, &Hardware::default()));
    assert_eq!(
        codes(&report),
        [
            IssueCode::MidiPortMissing,
            IssueCode::LtcUnavailable,
            IssueCode::MtcUnavailable
        ]
    );
    assert_eq!((report.errors, report.warnings), (3, 0));
    report.extend(check_hardware(
        &cue_project(midi_track(None, 0)),
        &OutputConfig::default(),
        &Hardware::default(),
    ));
    assert_eq!((report.errors, report.warnings), (3, 1));
    assert_eq!(report.issues[3].area, Area::Midi);
}
