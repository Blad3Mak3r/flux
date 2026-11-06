//! What the user can do to the MIDI and OSC tracks and to the OSC destinations. The window only says
//! which operation and with what; the defaults, the lookups, the ordering and the clean-up of
//! references live here, and [`Session::cue_edit`](super::Session::cue_edit) refuses anything that
//! would leave the project invalid.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ProjectError, Result};
use super::events::{MidiEvent, MidiMessage, MidiPortRef, OscArg, OscDestination, OscEvent};
use super::model::{Project, Timeline, Track, TrackKind};
use super::session::ProjectState;

/// Which kind of cue track an operation is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CueTrackKind {
    Midi,
    Osc,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CueOp {
    /// An empty track on the active timeline. `name` is the translated default name.
    AddTrack {
        kind: CueTrackKind,
        name: String,
    },
    /// A new event with sensible values at `at` (frames from the timeline origin). It goes on
    /// `track_id` when that is a track of `kind`, else on the first one of the active timeline,
    /// creating one named `track_name` when there is none.
    AddEvent {
        track_id: Option<String>,
        kind: CueTrackKind,
        at: u64,
        track_name: String,
    },
    SetMidi {
        id: String,
        message: MidiMessage,
    },
    SetOsc {
        id: String,
        address: String,
        args: Vec<OscArg>,
    },
    MoveEvent {
        id: String,
        at: u64,
    },
    RemoveEvent {
        id: String,
    },
    SetMidiPort {
        track_id: String,
        port: Option<MidiPortRef>,
    },
    SetOscDestination {
        track_id: String,
        destination: Option<String>,
    },
    AddDestination {
        name: String,
        host: String,
        port: u16,
    },
    UpdateDestination {
        id: String,
        name: String,
        host: String,
        port: u16,
    },
    /// Tracks that sent to it are left without a destination.
    RemoveDestination {
        id: String,
    },
}

/// The project after an operation, and the id of what it created, so the window can select it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CueEdited {
    #[serde(flatten)]
    pub state: ProjectState,
    pub id: Option<String>,
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn missing(what: &str, id: &str) -> ProjectError {
    ProjectError::Invalid(format!("there is no {what} {id}"))
}

fn tracks_mut(project: &mut Project) -> impl Iterator<Item = &mut Track> {
    project
        .timelines
        .iter_mut()
        .flat_map(|timeline| timeline.tracks.iter_mut())
}

fn active_mut<'a>(project: &'a mut Project, active: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == active)
        .ok_or_else(|| missing("timeline", active))
}

fn is_kind(track: &Track, kind: CueTrackKind) -> bool {
    matches!(
        (&track.kind, kind),
        (TrackKind::Midi { .. }, CueTrackKind::Midi) | (TrackKind::Osc { .. }, CueTrackKind::Osc)
    )
}

fn empty_track(project: &Project, kind: CueTrackKind, name: &str) -> Track {
    Track {
        id: new_id(),
        name: name.to_owned(),
        muted: false,
        gain_db: None,
        color: None,
        kind: match kind {
            CueTrackKind::Midi => TrackKind::Midi {
                port: None,
                events: Vec::new(),
            },
            // A project with a single receiver needs no choosing.
            CueTrackKind::Osc => TrackKind::Osc {
                destination: project.output.osc.first().map(|d| d.id.clone()),
                events: Vec::new(),
            },
        },
    }
}

/// A note on middle C, or a call to `/go`: something valid to start editing from.
fn default_midi(id: String, at: u64) -> MidiEvent {
    MidiEvent {
        id,
        at,
        message: MidiMessage::NoteOn {
            channel: 1,
            note: 60,
            velocity: 100,
            length: None,
        },
    }
}

fn default_osc(id: String, at: u64) -> OscEvent {
    OscEvent {
        id,
        at,
        address: "/go".into(),
        args: Vec::new(),
    }
}

fn add_event(
    project: &mut Project,
    active: &str,
    track_id: Option<&str>,
    kind: CueTrackKind,
    at: u64,
    track_name: &str,
) -> Result<String> {
    let created = empty_track(project, kind, track_name);
    let timeline = active_mut(project, active)?;
    let position = track_id
        .and_then(|wanted| {
            timeline
                .tracks
                .iter()
                .position(|track| track.id == wanted && is_kind(track, kind))
        })
        .or_else(|| {
            timeline
                .tracks
                .iter()
                .position(|track| is_kind(track, kind))
        });
    let index = position.unwrap_or_else(|| {
        timeline.tracks.push(created);
        timeline.tracks.len() - 1
    });
    let id = new_id();
    match &mut timeline.tracks[index].kind {
        TrackKind::Midi { events, .. } => events.push(default_midi(id.clone(), at)),
        TrackKind::Osc { events, .. } => events.push(default_osc(id.clone(), at)),
        _ => unreachable!("the track was picked by its kind"),
    }
    Ok(id)
}

fn midi_event<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut MidiEvent> {
    tracks_mut(project)
        .find_map(|track| match &mut track.kind {
            TrackKind::Midi { events, .. } => events.iter_mut().find(|event| event.id == id),
            _ => None,
        })
        .ok_or_else(|| missing("MIDI event", id))
}

fn osc_event<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut OscEvent> {
    tracks_mut(project)
        .find_map(|track| match &mut track.kind {
            TrackKind::Osc { events, .. } => events.iter_mut().find(|event| event.id == id),
            _ => None,
        })
        .ok_or_else(|| missing("OSC event", id))
}

/// Moves or removes the event `id` of either kind. `to` is the new position; `None` removes it.
fn move_or_remove(project: &mut Project, id: &str, to: Option<u64>) -> Result<()> {
    for track in tracks_mut(project) {
        match &mut track.kind {
            TrackKind::Midi { events, .. } => {
                if let Some(index) = events.iter().position(|event| event.id == id) {
                    match to {
                        Some(at) => events[index].at = at,
                        None => drop(events.remove(index)),
                    }
                    return Ok(());
                }
            }
            TrackKind::Osc { events, .. } => {
                if let Some(index) = events.iter().position(|event| event.id == id) {
                    match to {
                        Some(at) => events[index].at = at,
                        None => drop(events.remove(index)),
                    }
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    Err(missing("event", id))
}

fn track_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Track> {
    tracks_mut(project)
        .find(|track| track.id == id)
        .ok_or_else(|| missing("track", id))
}

fn destination_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut OscDestination> {
    project
        .output
        .osc
        .iter_mut()
        .find(|destination| destination.id == id)
        .ok_or_else(|| missing("OSC destination", id))
}

/// Events are kept in the order the show reaches them (stable, so equal positions keep theirs).
fn sort_events(project: &mut Project) {
    for track in tracks_mut(project) {
        match &mut track.kind {
            TrackKind::Midi { events, .. } => events.sort_by_key(|event| event.at),
            TrackKind::Osc { events, .. } => events.sort_by_key(|event| event.at),
            _ => {}
        }
    }
}

/// Applies `op` to `project`, whose active timeline is `active`. Returns the id of what it created.
pub(super) fn apply(project: &mut Project, active: &str, op: CueOp) -> Result<Option<String>> {
    let mut created = None;
    match op {
        CueOp::AddTrack { kind, name } => {
            let track = empty_track(project, kind, &name);
            created = Some(track.id.clone());
            active_mut(project, active)?.tracks.push(track);
        }
        CueOp::AddEvent {
            track_id,
            kind,
            at,
            track_name,
        } => {
            created = Some(add_event(
                project,
                active,
                track_id.as_deref(),
                kind,
                at,
                &track_name,
            )?);
        }
        CueOp::SetMidi { id, message } => midi_event(project, &id)?.message = message,
        CueOp::SetOsc { id, address, args } => {
            let event = osc_event(project, &id)?;
            event.address = address;
            event.args = args;
        }
        CueOp::MoveEvent { id, at } => move_or_remove(project, &id, Some(at))?,
        CueOp::RemoveEvent { id } => move_or_remove(project, &id, None)?,
        CueOp::SetMidiPort {
            track_id,
            port: chosen,
        } => match &mut track_mut(project, &track_id)?.kind {
            TrackKind::Midi { port, .. } => *port = chosen,
            _ => return Err(missing("MIDI track", &track_id)),
        },
        CueOp::SetOscDestination {
            track_id,
            destination: chosen,
        } => match &mut track_mut(project, &track_id)?.kind {
            TrackKind::Osc { destination, .. } => *destination = chosen,
            _ => return Err(missing("OSC track", &track_id)),
        },
        CueOp::AddDestination { name, host, port } => {
            let id = new_id();
            created = Some(id.clone());
            project.output.osc.push(OscDestination {
                id,
                name: name.trim().to_owned(),
                host: host.trim().to_owned(),
                port,
            });
        }
        CueOp::UpdateDestination {
            id,
            name,
            host,
            port,
        } => {
            let destination = destination_mut(project, &id)?;
            destination.name = name.trim().to_owned();
            destination.host = host.trim().to_owned();
            destination.port = port;
        }
        CueOp::RemoveDestination { id } => {
            destination_mut(project, &id)?;
            project
                .output
                .osc
                .retain(|destination| destination.id != id);
            for track in tracks_mut(project) {
                if let TrackKind::Osc { destination, .. } = &mut track.kind
                    && destination.as_deref() == Some(id.as_str())
                {
                    *destination = None;
                }
            }
        }
    }
    sort_events(project);
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::sample_project;
    use crate::session::Session;

    fn session() -> Session {
        let mut session = Session::default();
        session.update(sample_project());
        session
    }

    fn event_ids(project: &Project, track: &str) -> Vec<String> {
        let track = project
            .timelines
            .iter()
            .flat_map(|timeline| &timeline.tracks)
            .find(|candidate| candidate.id == track)
            .unwrap();
        match &track.kind {
            TrackKind::Midi { events, .. } => events.iter().map(|e| e.id.clone()).collect(),
            TrackKind::Osc { events, .. } => events.iter().map(|e| e.id.clone()).collect(),
            _ => panic!("not a cue track"),
        }
    }

    fn add(session: &mut Session, track: Option<&str>, kind: CueTrackKind, at: u64) -> CueEdited {
        session
            .cue_edit(CueOp::AddEvent {
                track_id: track.map(Into::into),
                kind,
                at,
                track_name: "New".into(),
            })
            .unwrap()
    }

    #[test]
    fn add_event_should_go_on_the_given_track_and_come_back_with_its_id() {
        let mut s = session();
        let edited = add(&mut s, Some("t3"), CueTrackKind::Midi, 100);
        let id = edited.id.unwrap();
        assert!(event_ids(&edited.state.project, "t3").contains(&id));
    }

    #[test]
    fn add_event_should_keep_the_events_in_the_order_of_the_show() {
        let mut s = session();
        let edited = add(&mut s, Some("t3"), CueTrackKind::Midi, 650);
        let ids = event_ids(&edited.state.project, "t3");
        assert_eq!(ids[0], "m1");
        assert_eq!(ids[2], "m2");
    }

    #[test]
    fn add_event_should_use_the_first_track_of_its_kind_when_none_is_named() {
        let mut s = session();
        let edited = add(&mut s, None, CueTrackKind::Osc, 10);
        assert_eq!(event_ids(&edited.state.project, "t4").len(), 2);
    }

    #[test]
    fn add_event_should_create_a_track_when_the_timeline_has_none_of_that_kind() {
        let mut s = session();
        let mut project = sample_project();
        project.timelines[0].tracks.retain(|t| t.id != "t4");
        project.output.osc.clear();
        s.update(project);
        let edited = add(&mut s, None, CueTrackKind::Osc, 10);
        let tracks = &edited.state.project.timelines[0].tracks;
        assert_eq!(tracks.last().unwrap().name, "New");
    }

    #[test]
    fn a_new_osc_track_should_send_to_the_only_destination() {
        let mut s = session();
        let edited = s
            .cue_edit(CueOp::AddTrack {
                kind: CueTrackKind::Osc,
                name: "More".into(),
            })
            .unwrap();
        let track = edited.state.project.timelines[0].tracks.last().unwrap();
        assert!(matches!(&track.kind, TrackKind::Osc { destination: Some(d), .. } if d == "d1"));
    }

    #[test]
    fn move_event_should_reorder_and_remove_event_should_drop_it() {
        let mut s = session();
        let moved = s
            .cue_edit(CueOp::MoveEvent {
                id: "m1".into(),
                at: 900,
            })
            .unwrap();
        assert_eq!(event_ids(&moved.state.project, "t3"), ["m2", "m1"]);
        let removed = s.cue_edit(CueOp::RemoveEvent { id: "m2".into() }).unwrap();
        assert_eq!(event_ids(&removed.state.project, "t3"), ["m1"]);
    }

    #[test]
    fn removing_a_destination_should_leave_its_tracks_without_one() {
        let mut s = session();
        let edited = s
            .cue_edit(CueOp::RemoveDestination { id: "d1".into() })
            .unwrap();
        let project = edited.state.project;
        assert!(project.output.osc.is_empty());
        let track = project.timelines[0].tracks.iter().find(|t| t.id == "t4");
        assert!(matches!(
            track.map(|t| &t.kind),
            Some(TrackKind::Osc {
                destination: None,
                ..
            })
        ));
    }

    #[test]
    fn a_refused_operation_should_leave_the_document_as_it_was() {
        let mut s = session();
        let refused = s.cue_edit(CueOp::SetOsc {
            id: "o1".into(),
            address: "no slash".into(),
            args: vec![],
        });
        assert!(refused.is_err());
        let kept = s.document().unwrap().project;
        assert_eq!(kept, sample_project());
    }

    #[test]
    fn an_unknown_event_should_be_refused() {
        let mut s = session();
        assert!(
            s.cue_edit(CueOp::RemoveEvent { id: "nope".into() })
                .is_err()
        );
    }

    #[test]
    fn a_destination_should_be_added_and_updated() {
        let mut s = session();
        let added = s
            .cue_edit(CueOp::AddDestination {
                name: " Companion ".into(),
                host: "10.0.0.5".into(),
                port: 12321,
            })
            .unwrap();
        let id = added.id.unwrap();
        let updated = s
            .cue_edit(CueOp::UpdateDestination {
                id: id.clone(),
                name: "Companion".into(),
                host: "10.0.0.6".into(),
                port: 12321,
            })
            .unwrap();
        let found = updated.state.project.output.osc.iter().find(|d| d.id == id);
        assert_eq!(found.map(|d| d.host.as_str()), Some("10.0.0.6"));
        assert_eq!(found.map(|d| d.name.as_str()), Some("Companion"));
    }

    #[test]
    fn a_track_should_not_point_at_a_destination_that_does_not_exist() {
        let mut s = session();
        let refused = s.cue_edit(CueOp::SetOscDestination {
            track_id: "t4".into(),
            destination: Some("ghost".into()),
        });
        assert!(refused.is_err());
    }

    #[test]
    fn set_midi_port_should_refuse_a_track_of_another_kind() {
        let mut s = session();
        let refused = s.cue_edit(CueOp::SetMidiPort {
            track_id: "t4".into(),
            port: None,
        });
        assert!(refused.is_err());
    }
}
