//! Remote control: what a MIDI message or an OSC packet asks Flux to do.
//!
//! Only the pure part lives here: decoding the bytes, mapping them to a [`ControlAction`] and
//! finding the frame a cue action goes to. The ports, the socket and the execution belong to the
//! app, so this is testable without hardware.
//!
//! OSC has fixed addresses (see [`osc_action`]); MIDI has a list of [`Binding`]s the user makes,
//! usually by touching a pad. Flux stays the master clock: this only moves the transport, it never
//! follows a timecode.

use serde::{Deserialize, Serialize};

use flux_project::OscArg;

use crate::cues::Cue;

/// What the control surface can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ControlAction {
    Play,
    Pause,
    /// Play when stopped or paused, pause when playing.
    Toggle,
    /// Stops and goes back to the start of the timeline.
    Stop,
    NextCue,
    PrevCue,
    /// The cue with this number, from 1.
    GotoCue(u32),
    /// Moves by this many frames, back when negative.
    StepFrames(i32),
    /// Moves by this many seconds, back when negative.
    StepSeconds(i32),
    NextTimeline,
    PrevTimeline,
    /// The timeline in this place, from 1.
    SelectTimeline(u32),
    ViewEdit,
    ViewLive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerKind {
    Note,
    Cc,
    Program,
}

/// The message of a MIDI controller that can trigger an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiTrigger {
    pub kind: TriggerKind,
    /// From 1 to 16, as the cue tracks and the controllers' manuals count them.
    pub channel: u8,
    pub number: u8,
}

/// A trigger and the action it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub trigger: MidiTrigger,
    pub action: ControlAction,
}

/// The action of `trigger`, if the user bound one.
pub fn action_for(bindings: &[Binding], trigger: MidiTrigger) -> Option<ControlAction> {
    bindings
        .iter()
        .find(|binding| binding.trigger == trigger)
        .map(|binding| binding.action)
}

/// `bindings` with one action per trigger: the first binding of a trigger stays.
pub fn dedup(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut kept: Vec<Binding> = Vec::with_capacity(bindings.len());
    for binding in bindings {
        if kept.iter().all(|other| other.trigger != binding.trigger) {
            kept.push(binding);
        }
    }
    kept
}

/// The trigger a MIDI message presses, or `None` for anything else: releases (a note off, a note
/// on with velocity 0, a control at 0), clock, active sensing and the rest of the traffic a
/// controller sends all the time.
pub fn decode_midi(bytes: &[u8]) -> Option<MidiTrigger> {
    let (&status, data) = bytes.split_first()?;
    let channel = (status & 0x0F) + 1;
    let kind = match status & 0xF0 {
        0x90 => TriggerKind::Note,
        0xB0 => TriggerKind::Cc,
        0xC0 => TriggerKind::Program,
        _ => return None,
    };
    let number = *data.first().filter(|number| **number < 0x80)?;
    if kind != TriggerKind::Program {
        // The velocity or the value: zero means released.
        let value = *data.get(1).filter(|value| **value < 0x80)?;
        if value == 0 {
            return None;
        }
    }
    Some(MidiTrigger {
        kind,
        channel,
        number,
    })
}

/// One OSC message.
#[derive(Debug, Clone, PartialEq)]
pub struct OscMessage {
    pub address: String,
    pub args: Vec<OscArg>,
}

/// Bundles inside bundles stop being followed at this depth.
const MAX_BUNDLE_DEPTH: usize = 4;

/// The messages in an OSC packet: one message, or the messages of a bundle (their time tags are
/// ignored, a control surface wants them now). A malformed packet gives none.
pub fn decode_osc(packet: &[u8]) -> Vec<OscMessage> {
    let mut messages = Vec::new();
    collect(packet, 0, &mut messages);
    messages
}

fn collect(packet: &[u8], depth: usize, out: &mut Vec<OscMessage>) {
    if let Some(elements) = packet.strip_prefix(b"#bundle\0") {
        if depth >= MAX_BUNDLE_DEPTH {
            return;
        }
        // 8 bytes of time tag, then elements each led by its size.
        let mut rest = elements.get(8..).unwrap_or_default();
        while let Some((size, tail)) = rest.split_first_chunk::<4>() {
            let size = i32::from_be_bytes(*size);
            let Ok(size) = usize::try_from(size) else {
                return;
            };
            let Some((element, tail)) = tail.split_at_checked(size) else {
                return;
            };
            collect(element, depth + 1, out);
            rest = tail;
        }
    } else if let Some(message) = decode_message(packet) {
        out.push(message);
    }
}

/// An OSC string at the start of `bytes` and what follows its padding.
fn take_string(bytes: &[u8]) -> Option<(&str, &[u8])> {
    let end = bytes.iter().position(|&byte| byte == 0)?;
    let text = std::str::from_utf8(&bytes[..end]).ok()?;
    let rest = bytes.get((end + 1).next_multiple_of(4)..)?;
    Some((text, rest))
}

fn decode_message(packet: &[u8]) -> Option<OscMessage> {
    let (address, rest) = take_string(packet)?;
    if !address.starts_with('/') {
        return None;
    }
    // OSC 1.0 senders may leave the type tags out, which means no arguments.
    let Some((tags, mut data)) = take_string(rest).filter(|(tags, _)| tags.starts_with(',')) else {
        return Some(OscMessage {
            address: address.to_owned(),
            args: Vec::new(),
        });
    };
    let mut args = Vec::new();
    for tag in tags[1..].chars() {
        match tag {
            'i' => {
                let (value, tail) = data.split_first_chunk::<4>()?;
                args.push(OscArg::Int(i32::from_be_bytes(*value)));
                data = tail;
            }
            'f' => {
                let (value, tail) = data.split_first_chunk::<4>()?;
                args.push(OscArg::Float(f32::from_be_bytes(*value)));
                data = tail;
            }
            's' => {
                let (text, tail) = take_string(data)?;
                args.push(OscArg::String(text.to_owned()));
                data = tail;
            }
            'T' => args.push(OscArg::Bool(true)),
            'F' => args.push(OscArg::Bool(false)),
            // Nil and impulse carry no data and mean nothing here.
            'N' | 'I' => {}
            // A type whose size is not known cannot be skipped.
            _ => return None,
        }
    }
    Some(OscMessage {
        address: address.to_owned(),
        args,
    })
}

/// The first argument as a whole number: an integer, a float (rounded) or a string with a
/// number, because the plugins of control surfaces send any of them.
fn number(args: &[OscArg]) -> Option<i32> {
    let value = match args.first()? {
        OscArg::Int(value) => f64::from(*value),
        OscArg::Float(value) => f64::from(*value),
        OscArg::String(text) => text.trim().parse().ok()?,
        OscArg::Bool(_) => return None,
    };
    // Rounding to an integer is the point, and out of range values are refused first.
    (value.is_finite() && value.abs() < f64::from(i32::MAX)).then(|| value.round() as i32)
}

/// The action an OSC message asks for. The addresses are fixed:
///
/// | Address | Argument | Action |
/// | --- | --- | --- |
/// | `/flux/play`, `/flux/pause`, `/flux/toggle`, `/flux/stop` | none | transport |
/// | `/flux/cue/next`, `/flux/cue/prev` | none | cue before or after the position |
/// | `/flux/cue/goto` | number, from 1 | the cue with that number |
/// | `/flux/seek/frames`, `/flux/seek/seconds` | number, negative goes back | move the position |
/// | `/flux/timeline/next`, `/flux/timeline/prev` | none | neighbour timeline |
/// | `/flux/timeline/select` | number, from 1 | the timeline in that place |
/// | `/flux/view/edit`, `/flux/view/live` | none | switch view |
///
/// A button of a control surface sends an argument (usually `1`) with the actions that need
/// none; it is ignored.
pub fn osc_action(message: &OscMessage) -> Option<ControlAction> {
    let args = &message.args;
    let position = |make: fn(u32) -> ControlAction| {
        number(args)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 1)
            .map(make)
    };
    match message.address.strip_prefix("/flux/")? {
        "play" => Some(ControlAction::Play),
        "pause" => Some(ControlAction::Pause),
        "toggle" => Some(ControlAction::Toggle),
        "stop" => Some(ControlAction::Stop),
        "cue/next" => Some(ControlAction::NextCue),
        "cue/prev" => Some(ControlAction::PrevCue),
        "cue/goto" => position(ControlAction::GotoCue),
        "seek/frames" => number(args).map(ControlAction::StepFrames),
        "seek/seconds" => number(args).map(ControlAction::StepSeconds),
        "timeline/next" => Some(ControlAction::NextTimeline),
        "timeline/prev" => Some(ControlAction::PrevTimeline),
        "timeline/select" => position(ControlAction::SelectTimeline),
        "view/edit" => Some(ControlAction::ViewEdit),
        "view/live" => Some(ControlAction::ViewLive),
        _ => None,
    }
}

/// The frame of the first unmuted cue after `position`.
pub fn next_cue_frame(cues: &[Cue], position: u64) -> Option<u64> {
    cues.iter()
        .filter(|cue| !cue.muted && cue.frame > position)
        .map(|cue| cue.frame)
        .min()
}

/// The frame of the last unmuted cue before `position`. A cue that started less than `grace`
/// frames ago does not count, so that pressing "previous" right after a cue goes to the one
/// before, and later restarts the one that is playing.
pub fn prev_cue_frame(cues: &[Cue], position: u64, grace: u64) -> Option<u64> {
    cues.iter()
        .filter(|cue| !cue.muted && cue.frame.saturating_add(grace) < position)
        .map(|cue| cue.frame)
        .max()
}

/// The frame of the cue with this number, muted or not: it is a place in the show.
pub fn goto_cue_frame(cues: &[Cue], number: u32) -> Option<u64> {
    cues.iter()
        .find(|cue| cue.number == number)
        .map(|cue| cue.frame)
}

/// `position` moved by `delta` frames, held inside `first..=last`.
pub fn step(position: u64, delta: i64, first: u64, last: u64) -> u64 {
    let moved = i128::from(position) + i128::from(delta);
    let clamped = moved.clamp(i128::from(first), i128::from(last.max(first)));
    // Inside two `u64`, so it fits.
    u64::try_from(clamped).unwrap_or(first)
}

/// The frame an action that moves the position goes to, or `None` when it moves nothing (the
/// transport and view actions, or a cue that does not exist). `position` and the result count the
/// timeline's start offset, `start` is where the timeline begins and `fps` is its nominal frame
/// rate, which is also how long "previous" forgives a cue that just started.
pub fn seek_target(
    action: ControlAction,
    position: u64,
    cues: &[Cue],
    start: u64,
    fps: u8,
) -> Option<u64> {
    let fps = u64::from(fps);
    match action {
        ControlAction::NextCue => next_cue_frame(cues, position),
        ControlAction::PrevCue => prev_cue_frame(cues, position, fps),
        ControlAction::GotoCue(number) => goto_cue_frame(cues, number),
        ControlAction::StepFrames(frames) => {
            Some(step(position, i64::from(frames), start, u64::MAX))
        }
        ControlAction::StepSeconds(seconds) => {
            let frames = i64::from(seconds).saturating_mul(i64::try_from(fps).unwrap_or(0));
            Some(step(position, frames, start, u64::MAX))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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
            },
            Binding {
                trigger: pad,
                action: ControlAction::Stop,
            },
            Binding {
                trigger: other,
                action: ControlAction::Stop,
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
            bpm: None,
        });
        let track = |id: &str, muted, kind| Track {
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
}
