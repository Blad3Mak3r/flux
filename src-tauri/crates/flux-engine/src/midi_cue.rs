//! The MIDI cue tracks: the bytes of every message a track can send, the schedule a timeline
//! makes of them, and the sink that writes them to a port.
//!
//! Encoding and planning are pure and tested without a port; the only thing that needs one is
//! [`MidiWrite`] for `midir`.

use std::collections::BTreeSet;

use flux_core::Clock;
use flux_project::{MidiMessage, MidiPortRef, MscCommand, Timeline, TrackKind};

use super::fire::{Planned, Schedule, Sink};

const NOTE_OFF: u8 = 0x80;
const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;
const PROGRAM_CHANGE: u8 = 0xC0;
const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;
/// Universal real-time SysEx, MIDI Show Control sub-id.
const REAL_TIME: u8 = 0x7F;
const MSC: u8 = 0x02;

/// The bytes that go on the wire for `message`. Channels are 1 to 16 in the project and 0 to 15
/// on the wire.
pub fn encode(message: &MidiMessage) -> Vec<u8> {
    let status = |base: u8, channel: u8| base | (channel.saturating_sub(1) & 0x0F);
    match message {
        MidiMessage::NoteOn {
            channel,
            note,
            velocity,
            ..
        } => vec![status(NOTE_ON, *channel), *note, *velocity],
        MidiMessage::NoteOff { channel, note } => vec![status(NOTE_OFF, *channel), *note, 0],
        MidiMessage::ControlChange {
            channel,
            controller,
            value,
        } => vec![status(CONTROL_CHANGE, *channel), *controller, *value],
        MidiMessage::ProgramChange { channel, program } => {
            vec![status(PROGRAM_CHANGE, *channel), *program]
        }
        MidiMessage::Msc {
            device_id,
            format,
            command,
            cue,
            list,
        } => {
            let command = match command {
                MscCommand::Go => 0x01,
                MscCommand::Stop => 0x02,
                MscCommand::Resume => 0x03,
            };
            let mut bytes = vec![SYSEX_START, REAL_TIME, *device_id, MSC, *format, command];
            if let Some(cue) = cue {
                bytes.extend(cue.bytes());
                if let Some(list) = list {
                    bytes.push(0x00);
                    bytes.extend(list.bytes());
                }
            }
            bytes.push(SYSEX_END);
            bytes
        }
        MidiMessage::SysEx { data } => {
            let mut bytes = Vec::with_capacity(data.len() + 2);
            bytes.push(SYSEX_START);
            bytes.extend(data);
            bytes.push(SYSEX_END);
            bytes
        }
    }
}

/// What one MIDI port is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiPlan {
    pub port: MidiPortRef,
    pub schedule: Schedule<Vec<u8>>,
}

/// The schedule of every port the unmuted MIDI tracks of `timeline` send to. Tracks on the same
/// port share one schedule; a track with no port sends nothing. A note with a length gets its Note
/// Off planned that many frames after it.
pub fn plan(timeline: &Timeline, clock: &Clock) -> Vec<MidiPlan> {
    let mut plans: Vec<(MidiPortRef, Vec<Planned<Vec<u8>>>)> = Vec::new();
    for track in timeline.tracks.iter().filter(|track| !track.muted) {
        let TrackKind::Midi {
            port: Some(port),
            events,
        } = &track.kind
        else {
            continue;
        };
        let at = match plans.iter().position(|(other, _)| other == port) {
            Some(at) => at,
            None => {
                plans.push((port.clone(), Vec::new()));
                plans.len() - 1
            }
        };
        let items = &mut plans[at].1;
        for event in events {
            items.push(Planned {
                at: clock.frames_to_samples(event.at),
                payload: encode(&event.message),
            });
            if let MidiMessage::NoteOn {
                channel,
                note,
                length: Some(length),
                ..
            } = event.message
            {
                items.push(Planned {
                    at: clock.frames_to_samples(event.at + length),
                    payload: encode(&MidiMessage::NoteOff { channel, note }),
                });
            }
        }
    }
    plans
        .into_iter()
        .map(|(port, items)| MidiPlan {
            port,
            schedule: Schedule::new(items),
        })
        .collect()
}

/// Whatever a MIDI port is written to; `midir` in the app, a list of messages in the tests.
pub trait MidiWrite {
    fn write(&mut self, bytes: &[u8]) -> bool;
}

impl MidiWrite for midir::MidiOutputConnection {
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.send(bytes).is_ok()
    }
}

/// Writes messages to a port and remembers which notes are on, so it can release them.
#[derive(Debug)]
pub struct MidiSink<W> {
    port: W,
    /// `(status byte of the channel, note)` of every note that is on.
    held: BTreeSet<(u8, u8)>,
}

impl<W: MidiWrite> MidiSink<W> {
    pub fn new(port: W) -> Self {
        Self {
            port,
            held: BTreeSet::new(),
        }
    }
}

impl<W: MidiWrite> Sink<Vec<u8>> for MidiSink<W> {
    fn send(&mut self, payload: &Vec<u8>) -> bool {
        if let [status, note, velocity] = payload[..] {
            let channel = status & 0x0F;
            match status & 0xF0 {
                NOTE_ON if velocity > 0 => {
                    self.held.insert((channel, note));
                }
                NOTE_ON | NOTE_OFF => {
                    self.held.remove(&(channel, note));
                }
                _ => {}
            }
        }
        self.port.write(payload)
    }

    fn release(&mut self) {
        for (channel, note) in std::mem::take(&mut self.held) {
            self.port.write(&[NOTE_OFF | channel, note, 0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::FrameRate;
    use flux_project::{MidiEvent, Track};

    fn note_on(channel: u8, note: u8, velocity: u8, length: Option<u64>) -> MidiMessage {
        MidiMessage::NoteOn {
            channel,
            note,
            velocity,
            length,
        }
    }

    #[test]
    fn note_on_should_use_the_status_of_its_channel() {
        assert_eq!(encode(&note_on(1, 60, 100, None)), [0x90, 60, 100]);
        assert_eq!(encode(&note_on(16, 61, 1, None)), [0x9F, 61, 1]);
    }

    #[test]
    fn note_off_cc_and_program_should_use_their_statuses() {
        assert_eq!(
            encode(&MidiMessage::NoteOff {
                channel: 2,
                note: 60
            }),
            [0x81, 60, 0]
        );
        let cc = MidiMessage::ControlChange {
            channel: 3,
            controller: 7,
            value: 100,
        };
        assert_eq!(encode(&cc), [0xB2, 7, 100]);
        let pc = MidiMessage::ProgramChange {
            channel: 4,
            program: 12,
        };
        assert_eq!(encode(&pc), [0xC3, 12]);
    }

    #[test]
    fn msc_go_should_carry_the_cue_and_the_list_separated_by_a_zero() {
        let msc = MidiMessage::Msc {
            device_id: 0x7F,
            format: 0x01,
            command: MscCommand::Go,
            cue: Some("12.5".into()),
            list: Some("1".into()),
        };
        assert_eq!(
            encode(&msc),
            [
                0xF0, 0x7F, 0x7F, 0x02, 0x01, 0x01, b'1', b'2', b'.', b'5', 0x00, b'1', 0xF7
            ]
        );
    }

    #[test]
    fn msc_without_a_cue_should_be_the_bare_command() {
        let msc = MidiMessage::Msc {
            device_id: 5,
            format: 0x7F,
            command: MscCommand::Resume,
            cue: None,
            list: None,
        };
        assert_eq!(encode(&msc), [0xF0, 0x7F, 5, 0x02, 0x7F, 0x03, 0xF7]);
    }

    #[test]
    fn sysex_should_be_framed() {
        let sysex = MidiMessage::SysEx {
            data: vec![0x43, 0x10],
        };
        assert_eq!(encode(&sysex), [0xF0, 0x43, 0x10, 0xF7]);
    }

    fn clock() -> Clock {
        // 25 fps at 48 kHz: a frame is 1920 samples.
        Clock::new(48_000, FrameRate::Fps25)
    }

    fn track(id: &str, muted: bool, port: Option<&str>, events: Vec<MidiEvent>) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Midi {
                port: port.map(|name| MidiPortRef {
                    port_name: name.into(),
                    port_index: 0,
                }),
                events,
            },
        }
    }

    fn event(id: &str, at: u64, message: MidiMessage) -> MidiEvent {
        MidiEvent {
            id: id.into(),
            at,
            message,
        }
    }

    fn timeline(tracks: Vec<Track>) -> Timeline {
        let mut timeline = Timeline::new("T", FrameRate::Fps25);
        timeline.tracks = tracks;
        timeline
    }

    #[test]
    fn plan_should_place_events_at_their_frame_in_samples() {
        let t = timeline(vec![track(
            "a",
            false,
            Some("P"),
            vec![event("e", 2, note_on(1, 60, 100, None))],
        )]);
        let plans = plan(&t, &clock());
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].schedule.items()[0].at, 3_840);
    }

    #[test]
    fn plan_should_add_the_note_off_of_a_note_with_a_length() {
        let t = timeline(vec![track(
            "a",
            false,
            Some("P"),
            vec![event("e", 2, note_on(1, 60, 100, Some(5)))],
        )]);
        let items = plan(&t, &clock())[0].schedule.items().to_vec();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].at, 7 * 1_920);
        assert_eq!(items[1].payload, [0x80, 60, 0]);
    }

    #[test]
    fn plan_should_skip_muted_tracks_and_tracks_without_a_port() {
        let t = timeline(vec![
            track(
                "m",
                true,
                Some("P"),
                vec![event("e", 1, note_on(1, 1, 1, None))],
            ),
            track(
                "n",
                false,
                None,
                vec![event("f", 1, note_on(1, 1, 1, None))],
            ),
        ]);
        assert!(plan(&t, &clock()).is_empty());
    }

    #[test]
    fn plan_should_merge_tracks_on_the_same_port_in_time_order() {
        let t = timeline(vec![
            track(
                "a",
                false,
                Some("P"),
                vec![event("e", 10, note_on(1, 1, 1, None))],
            ),
            track(
                "b",
                false,
                Some("P"),
                vec![event("f", 5, note_on(1, 2, 1, None))],
            ),
            track(
                "c",
                false,
                Some("Q"),
                vec![event("g", 5, note_on(1, 3, 1, None))],
            ),
        ]);
        let plans = plan(&t, &clock());
        assert_eq!(plans.len(), 2);
        let notes: Vec<u8> = plans[0]
            .schedule
            .items()
            .iter()
            .map(|item| item.payload[1])
            .collect();
        assert_eq!(notes, [2, 1]);
    }

    #[derive(Default)]
    struct Recorder(Vec<Vec<u8>>);

    impl MidiWrite for Recorder {
        fn write(&mut self, bytes: &[u8]) -> bool {
            self.0.push(bytes.to_vec());
            true
        }
    }

    #[test]
    fn release_should_turn_off_only_the_notes_that_are_on() {
        let mut sink = MidiSink::new(Recorder::default());
        sink.send(&vec![0x90, 60, 100]);
        sink.send(&vec![0x91, 62, 100]);
        sink.send(&vec![0x80, 60, 0]);
        sink.release();
        assert_eq!(sink.port.0.last().unwrap(), &vec![0x81, 62, 0]);
        assert_eq!(sink.port.0.len(), 4);
        sink.release();
        assert_eq!(sink.port.0.len(), 4);
    }

    #[test]
    fn a_note_on_with_zero_velocity_should_count_as_off() {
        let mut sink = MidiSink::new(Recorder::default());
        sink.send(&vec![0x90, 60, 100]);
        sink.send(&vec![0x90, 60, 0]);
        sink.release();
        assert_eq!(sink.port.0.len(), 2);
    }
}
