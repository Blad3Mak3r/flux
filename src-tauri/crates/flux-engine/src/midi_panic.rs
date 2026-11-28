//! MIDI panic: one deliberate action that silences whatever the show's MIDI gear is holding.
//!
//! The fire threads already release the notes Flux itself holds (see [`crate::fire`]). That does not
//! help when the receiver is stuck for another reason (a lost Note Off after a cable drop, a synth
//! latched by hand), so the operator can send, on every channel of every MIDI port the project
//! uses, All Sound Off, All Notes Off and Reset All Controllers.
//!
//! The ports are opened for the burst and closed after it, so the panic works with the output
//! stopped too. Nothing here logs and nothing runs in an audio callback: the caller reports the
//! [`PanicReport`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::Serialize;

use crate::midi_cue::MidiWrite;
use crate::output::connect_midi;
use flux_project::{MidiPortRef, MtcOutput, Project, TrackKind};

const CONTROL_CHANGE: u8 = 0xB0;
const ALL_SOUND_OFF: u8 = 120;
const RESET_ALL_CONTROLLERS: u8 = 121;
const ALL_NOTES_OFF: u8 = 123;

const CLIENT_NAME: &str = "FLUX panic";

/// How many panics were asked for. The fire threads read it in their loop (one atomic load, so the
/// output path stays free of locks and logs) and send the panic through the ports they own: some
/// systems let a port be open once only, so the output's own connection is the one that can reach it.
static REQUESTS: AtomicU64 = AtomicU64::new(0);

/// The ports a running output's fire thread holds open.
static SERVED: Mutex<Vec<MidiPortRef>> = Mutex::new(Vec::new());

/// Marks a port as held by a fire thread for as long as it lives; the thread owns it.
pub(crate) struct Served(MidiPortRef);

impl Served {
    pub(crate) fn new(port: MidiPortRef) -> Self {
        SERVED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(port.clone());
        Self(port)
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let mut served = SERVED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = served.iter().position(|port| *port == self.0) {
            served.remove(at);
        }
    }
}

fn is_served(port: &MidiPortRef) -> bool {
    SERVED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains(port)
}

/// The count of panics asked for so far.
pub(crate) fn requests() -> u64 {
    REQUESTS.load(Ordering::Acquire)
}

/// What one port is sent: on each of the 16 channels, All Sound Off (CC 120), Reset All Controllers
/// (CC 121) and All Notes Off (CC 123), in that order, so the notes go off last, with the sustain
/// pedal and the other controllers already reset.
pub fn messages() -> Vec<[u8; 3]> {
    (0..16)
        .flat_map(|channel| {
            [ALL_SOUND_OFF, RESET_ALL_CONTROLLERS, ALL_NOTES_OFF]
                .map(|controller| [CONTROL_CHANGE | channel, controller, 0])
        })
        .collect()
}

/// The MIDI ports `project` uses, each once, in the order found: the port of every MIDI track of
/// every timeline (muted ones too: a port a muted track sends to may still hold a note), then the
/// MTC ports `mtc` that this machine assigned, which are the ones with a name.
pub fn ports(project: &Project, mtc: &[MtcOutput]) -> Vec<MidiPortRef> {
    let tracks = project
        .timelines
        .iter()
        .flat_map(|timeline| &timeline.tracks)
        .filter_map(|track| match &track.kind {
            TrackKind::Midi { port, .. } => port.clone(),
            _ => None,
        });
    let mtc = mtc
        .iter()
        .filter(|out| !out.port_name.is_empty())
        .map(|out| MidiPortRef {
            port_name: out.port_name.clone(),
            port_index: out.port_index,
        });
    let mut found: Vec<MidiPortRef> = Vec::new();
    for port in tracks.chain(mtc) {
        if !found.contains(&port) {
            found.push(port);
        }
    }
    found
}

/// How the burst went.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanicReport {
    /// Ports that took every message, including those reached through the output.
    pub sent: usize,
    /// Of `sent`, the ports a running output holds: its own thread sent to them.
    pub via_output: usize,
    /// Names of the ports that could not be opened or refused a message.
    pub failed: Vec<String>,
}

/// Sends [`messages`] to each of `ports`, opening each with `open` for the burst. A port that does
/// not open or refuses a message is reported and the rest still get theirs.
pub fn send_with<W: MidiWrite>(
    ports: &[MidiPortRef],
    served: impl Fn(&MidiPortRef) -> bool,
    mut open: impl FnMut(&MidiPortRef) -> Result<W, String>,
) -> PanicReport {
    let messages = messages();
    let mut report = PanicReport::default();
    for port in ports {
        // A port the output holds is silenced by the output's own thread (see `requests`); opening
        // it a second time may be refused, and says nothing about whether it was reached.
        if served(port) {
            report.sent += 1;
            report.via_output += 1;
            continue;
        }
        let delivered = open(port).is_ok_and(|mut writer| {
            // Every message is tried, so one refusal does not leave the later channels sounding.
            let mut all = true;
            for message in &messages {
                all &= writer.write(message);
            }
            all
        });
        if delivered {
            report.sent += 1;
        } else {
            report.failed.push(port.port_name.clone());
        }
    }
    report
}

/// Sends the panic to the real MIDI ports: through the ports a running output holds open (its fire
/// threads, counted in `via_output`), and by opening each of the others for the burst.
pub fn send(ports: &[MidiPortRef]) -> PanicReport {
    REQUESTS.fetch_add(1, Ordering::Release);
    send_with(ports, is_served, |port| {
        connect_midi(&port.port_name, port.port_index, CLIENT_NAME)
            .map(|(_, _, connection)| connection)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::{FrameRate, Track};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn port(name: &str, index: usize) -> MidiPortRef {
        MidiPortRef {
            port_name: name.into(),
            port_index: index,
        }
    }

    fn midi_track(id: &str, port: Option<MidiPortRef>, muted: bool) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Midi {
                port,
                events: Vec::new(),
            },
        }
    }

    fn mtc(name: &str) -> MtcOutput {
        MtcOutput {
            port_name: name.into(),
            port_index: 0,
            offset_ms: None,
        }
    }

    #[test]
    fn messages_should_silence_every_channel_with_three_controllers() {
        let messages = messages();
        assert_eq!(messages.len(), 48);
        for channel in 0..16u8 {
            for controller in [120, 121, 123] {
                assert!(messages.contains(&[0xB0 | channel, controller, 0]));
            }
        }
        // Per channel the sound goes off, the controllers reset, and the notes go off last.
        assert_eq!(
            messages[..3],
            [[0xB0, 120, 0], [0xB0, 121, 0], [0xB0, 123, 0]]
        );
    }

    #[test]
    fn ports_should_list_each_port_the_midi_tracks_and_mtc_use_once() {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].tracks = vec![
            midi_track("a", Some(port("Synth", 0)), false),
            midi_track("b", Some(port("Synth", 0)), false),
            midi_track("muted", Some(port("Lights", 0)), true),
            midi_track("none", None, false),
            midi_track("twin", Some(port("Synth", 1)), false),
        ];
        let mut second = flux_project::Timeline::new("Two", FrameRate::Fps25);
        second.tracks = vec![midi_track("c", Some(port("Sampler", 0)), false)];
        project.timelines.push(second);

        let found = ports(&project, &[mtc("Sync"), mtc(""), mtc("Synth")]);

        assert_eq!(
            found,
            [
                port("Synth", 0),
                port("Lights", 0),
                port("Synth", 1),
                port("Sampler", 0),
                port("Sync", 0),
            ]
        );
    }

    #[test]
    fn ports_should_be_empty_when_the_project_uses_no_midi() {
        let project = Project::new("Show", FrameRate::Fps25);
        assert!(ports(&project, &[]).is_empty());
    }

    /// What every port was sent, under its name.
    type Log = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

    /// A port that records what it was sent under its name, in a log all the ports share.
    struct Recorder {
        name: String,
        log: Log,
        accept: bool,
    }

    impl MidiWrite for Recorder {
        fn write(&mut self, bytes: &[u8]) -> bool {
            self.log
                .borrow_mut()
                .push((self.name.clone(), bytes.to_vec()));
            self.accept
        }
    }

    #[test]
    fn send_should_reach_every_channel_of_every_port_and_no_other() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let used = [port("Synth", 0), port("Lights", 0)];

        let report = send_with(
            &used,
            |_| false,
            |port| {
                Ok(Recorder {
                    name: port.port_name.clone(),
                    log: Rc::clone(&log),
                    accept: true,
                })
            },
        );

        assert_eq!(
            report,
            PanicReport {
                sent: 2,
                via_output: 0,
                failed: vec![]
            }
        );
        let log = log.borrow();
        for name in ["Synth", "Lights"] {
            let sent: Vec<Vec<u8>> = log
                .iter()
                .filter(|(to, _)| to == name)
                .map(|(_, bytes)| bytes.clone())
                .collect();
            let expected: Vec<Vec<u8>> = messages().iter().map(|m| m.to_vec()).collect();
            assert_eq!(sent, expected);
        }
        assert!(log.iter().all(|(to, _)| to == "Synth" || to == "Lights"));
    }

    #[test]
    fn a_port_the_output_holds_should_count_as_sent_through_it_and_not_be_opened() {
        let ports = [port("Held", 0), port("Free", 0)];

        let report = send_with(
            &ports,
            |port| port.port_name == "Held",
            |port| -> Result<Recorder, String> { Err(format!("must not open {}", port.port_name)) },
        );

        assert_eq!((report.sent, report.via_output), (1, 1));
        assert_eq!(report.failed, ["Free"]);
    }

    #[test]
    fn a_fire_thread_should_mark_its_port_while_it_lives() {
        let held = port("Registry test", 7);
        assert!(!is_served(&held));
        let guard = Served::new(held.clone());
        assert!(is_served(&held));
        drop(guard);
        assert!(!is_served(&held));
    }

    #[test]
    fn a_port_that_does_not_open_or_refuses_should_be_reported_and_the_rest_still_sent() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let ports = [port("Gone", 0), port("Stubborn", 0), port("Fine", 0)];

        let report = send_with(
            &ports,
            |_| false,
            |port| match port.port_name.as_str() {
                "Gone" => Err("not found".into()),
                name => Ok(Recorder {
                    name: name.into(),
                    log: Rc::clone(&log),
                    accept: name == "Fine",
                }),
            },
        );

        assert_eq!(report.sent, 1);
        assert_eq!(report.failed, ["Gone", "Stubborn"]);
        // A refusing port still got every message tried.
        let tried = log
            .borrow()
            .iter()
            .filter(|(to, _)| to == "Stubborn")
            .count();
        assert_eq!(tried, 48);
    }
}
