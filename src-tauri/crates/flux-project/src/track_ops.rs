//! The edits of tracks and of the clips and notes on them that [`ItemOp`](super::ItemOp) asks for:
//! moving, adding, removing, the fader and colour of a track, the tempo of an audio file. The window
//! only says which and with what; the rounding, the clamping at the start of the timeline, the 0 dB
//! stored as no value and the clean-up of audio nobody plays live here.

use std::collections::HashSet;

use uuid::Uuid;

use super::error::{ProjectError, Result};
use super::item_ops::{MAX_TRACK_NAME, clip_mut};
use super::model::{AudioAsset, AudioClip, Note, Project, Tempo, Timeline, Track, TrackKind};

fn invalid(message: String) -> ProjectError {
    ProjectError::Invalid(message)
}

fn timeline_mut<'a>(project: &'a mut Project, active: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == active)
        .ok_or_else(|| invalid(format!("there is no timeline {active}")))
}

fn track_mut<'a>(project: &'a mut Project, active: &str, id: &str) -> Result<&'a mut Track> {
    timeline_mut(project, active)?
        .tracks
        .iter_mut()
        .find(|track| track.id == id)
        .ok_or_else(|| invalid(format!("there is no track {id}")))
}

fn note_mut<'a>(project: &'a mut Project, active: &str, id: &str) -> Result<&'a mut Note> {
    timeline_mut(project, active)?
        .tracks
        .iter_mut()
        .filter_map(|track| match &mut track.kind {
            TrackKind::Notes { notes } => Some(notes),
            _ => None,
        })
        .flatten()
        .find(|note| note.id == id)
        .ok_or_else(|| invalid(format!("there is no note {id}")))
}

/// A position in frames from the timeline origin, to a whole frame and never before it.
fn position(at: f64) -> Result<u64> {
    if !at.is_finite() {
        return Err(invalid("a position must be a number".into()));
    }
    Ok(at.round().max(0.0) as u64)
}

fn new_track(name: String, kind: TrackKind) -> Track {
    Track {
        audio_output: Default::default(),
        id: Uuid::new_v4().to_string(),
        name,
        muted: false,
        gain_db: None,
        color: None,
        kind,
    }
}

/// Moves a clip or a note to `at` frames from the timeline origin.
pub(crate) fn move_item(project: &mut Project, active: &str, id: &str, at: f64) -> Result<()> {
    let at = position(at)?;
    if let Ok(clip) = clip_mut(project, active, id) {
        clip.start = at;
        return Ok(());
    }
    note_mut(project, active, id)
        .map(|note| note.at = at)
        .map_err(|_| invalid(format!("there is no clip or note {id}")))
}

pub(crate) fn set_clip_gain(project: &mut Project, active: &str, id: &str, db: f32) -> Result<()> {
    if !db.is_finite() {
        return Err(invalid("a gain must be a number".into()));
    }
    clip_mut(project, active, id)?.gain_db = db;
    Ok(())
}

pub(crate) fn set_note_text(
    project: &mut Project,
    active: &str,
    id: &str,
    text: String,
) -> Result<()> {
    note_mut(project, active, id)?.text = text;
    Ok(())
}

/// How long a note lasts, in frames. None, or less than a frame, goes back to "until the next
/// note", stored as no value.
pub(crate) fn set_note_length(
    project: &mut Project,
    active: &str,
    id: &str,
    length: Option<f64>,
) -> Result<()> {
    note_mut(project, active, id)?.length = length
        .filter(|length| *length >= 1.0)
        .map(|length| length.round() as u64);
    Ok(())
}

/// Adds an empty note at `at`. It goes on `track_id` when that is a notes track, else on the first
/// notes track, creating one named `track_name` when there is none. Returns the id of the note.
pub(crate) fn add_note(
    project: &mut Project,
    active: &str,
    track_id: Option<&str>,
    at: f64,
    track_name: &str,
) -> Result<String> {
    let note = Note {
        id: Uuid::new_v4().to_string(),
        at: position(at)?,
        length: None,
        text: String::new(),
    };
    let id = note.id.clone();
    let timeline = timeline_mut(project, active)?;
    let is_notes = |track: &Track| matches!(track.kind, TrackKind::Notes { .. });
    let index = timeline
        .tracks
        .iter()
        .position(|track| Some(track.id.as_str()) == track_id && is_notes(track))
        .or_else(|| timeline.tracks.iter().position(is_notes));
    match index.map(|index| &mut timeline.tracks[index].kind) {
        Some(TrackKind::Notes { notes }) => notes.push(note),
        _ => timeline.tracks.push(new_track(
            track_name.trim().chars().take(MAX_TRACK_NAME).collect(),
            TrackKind::Notes { notes: vec![note] },
        )),
    }
    Ok(id)
}

/// Adds imported audio files to the project, each on a new track of its own named after it with a
/// clip at `at`. Returns the id of the last clip.
pub(crate) fn add_audio_clips(
    project: &mut Project,
    active: &str,
    assets: Vec<AudioAsset>,
    at: f64,
) -> Result<String> {
    let at = position(at)?;
    let mut last = None;
    for asset in assets {
        let clip = AudioClip::new(Uuid::new_v4().to_string(), asset.id.clone(), at);
        last = Some(clip.id.clone());
        let name = asset.name.chars().take(MAX_TRACK_NAME).collect();
        let track = new_track(name, TrackKind::Audio { clips: vec![clip] });
        timeline_mut(project, active)?.tracks.push(track);
        project.assets.push(asset);
    }
    last.ok_or_else(|| invalid("there is no audio to add".into()))
}

/// Drops the audio tracks of the timeline left with no clip, and the audio files nothing plays.
fn drop_unused(project: &mut Project, active: &str) -> Result<()> {
    timeline_mut(project, active)?
        .tracks
        .retain(|track| !matches!(&track.kind, TrackKind::Audio { clips } if clips.is_empty()));
    let used: HashSet<String> = project
        .used_assets()
        .into_iter()
        .map(String::from)
        .collect();
    project.assets.retain(|asset| used.contains(&asset.id));
    Ok(())
}

/// Removes a whole track with its clips or notes, and the audio files nothing else plays.
pub(crate) fn remove_track(project: &mut Project, active: &str, id: &str) -> Result<()> {
    let timeline = timeline_mut(project, active)?;
    let before = timeline.tracks.len();
    timeline.tracks.retain(|track| track.id != id);
    if timeline.tracks.len() == before {
        return Err(invalid(format!("there is no track {id}")));
    }
    drop_unused(project, active)
}

/// Moves a track to index `to` of the track order (clamped); that order is the one on screen and in
/// the mix.
pub(crate) fn move_track(project: &mut Project, active: &str, id: &str, to: i64) -> Result<()> {
    let tracks = &mut timeline_mut(project, active)?.tracks;
    let from = tracks
        .iter()
        .position(|track| track.id == id)
        .ok_or_else(|| invalid(format!("there is no track {id}")))?;
    let to = to.clamp(0, tracks.len() as i64 - 1) as usize;
    let track = tracks.remove(from);
    tracks.insert(to, track);
    Ok(())
}

pub(crate) fn toggle_mute(project: &mut Project, active: &str, id: &str) -> Result<()> {
    let track = track_mut(project, active, id)?;
    track.muted = !track.muted;
    Ok(())
}

/// Sets the fader of an audio or video track. 0 dB is stored as no value.
pub(crate) fn set_track_gain(project: &mut Project, active: &str, id: &str, db: f32) -> Result<()> {
    if !db.is_finite() {
        return Err(invalid("a gain must be a number".into()));
    }
    let track = track_mut(project, active, id)?;
    if !matches!(
        track.kind,
        TrackKind::Audio { .. } | TrackKind::Video { .. }
    ) {
        return Err(invalid(format!("track {id} has no fader")));
    }
    track.gain_db = (db != 0.0).then_some(db);
    Ok(())
}

/// Sets the colour a track is drawn in (`#rrggbb`, which the project checks); None goes back to
/// the default, stored as no value.
pub(crate) fn set_track_color(
    project: &mut Project,
    active: &str,
    id: &str,
    color: Option<String>,
) -> Result<()> {
    track_mut(project, active, id)?.color = color;
    Ok(())
}

/// Records the tempo of an audio file, to two decimals and within the range of a tempo; None
/// forgets it.
pub(crate) fn set_asset_bpm(project: &mut Project, id: &str, bpm: Option<f64>) -> Result<()> {
    let bpm = match bpm {
        Some(bpm) => {
            let bpm = (bpm * 100.0).round() / 100.0;
            Tempo {
                bpm,
                ..Tempo::default()
            }
            .validate()?;
            Some(bpm)
        }
        None => None,
    };
    project
        .assets
        .iter_mut()
        .find(|asset| asset.id == id)
        .ok_or_else(|| invalid(format!("there is no audio file {id}")))?
        .bpm = bpm;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CueEdited;
    use crate::item_ops::ItemOp;
    use crate::model::tests::sample_project;
    use crate::session::Session;

    fn session() -> Session {
        let mut session = Session::default();
        session.update(sample_project());
        session
    }

    fn edit(session: &mut Session, op: ItemOp) -> Result<Project> {
        session.item_edit(op).map(|edited| edited.state.project)
    }

    fn ids(project: &Project) -> Vec<&str> {
        let tracks = &project.timelines[0].tracks;
        tracks.iter().map(|track| track.id.as_str()).collect()
    }

    fn track<'a>(project: &'a Project, id: &str) -> &'a Track {
        let tracks = &project.timelines[0].tracks;
        tracks.iter().find(|track| track.id == id).unwrap()
    }

    fn clip_start(project: &Project) -> u64 {
        let TrackKind::Audio { clips } = &track(project, "t1").kind else {
            panic!("t1 is audio");
        };
        clips[0].start
    }

    fn note<'a>(project: &'a Project, id: &str) -> &'a Note {
        let TrackKind::Notes { notes } = &track(project, "t2").kind else {
            panic!("t2 is notes");
        };
        notes.iter().find(|note| note.id == id).unwrap()
    }

    #[test]
    fn move_item_should_round_and_stop_at_the_start_for_clips_and_notes() {
        let mut session = session();
        let mv = |id: &str, at| ItemOp::MoveItem { id: id.into(), at };
        assert_eq!(clip_start(&edit(&mut session, mv("c1", 40.4)).unwrap()), 40);
        assert_eq!(clip_start(&edit(&mut session, mv("c1", -9.0)).unwrap()), 0);
        let project = edit(&mut session, mv("n1", 9.6)).unwrap();
        assert_eq!(note(&project, "n1").at, 10);
        assert!(edit(&mut session, mv("nope", 1.0)).is_err());
        assert!(edit(&mut session, mv("c1", f64::NAN)).is_err());
    }

    #[test]
    fn notes_should_be_added_written_and_given_a_length() {
        let mut session = session();
        let CueEdited { state, id, .. } = session
            .item_edit(ItemOp::AddNote {
                track_id: Some("t3".into()),
                at: -30.0,
                track_name: "Notes".into(),
            })
            .unwrap();
        let id = id.expect("the id of the note");
        assert_eq!(
            ids(&state.project).len(),
            4,
            "a MIDI track is no place for a note"
        );
        let added = note(&state.project, &id);
        assert_eq!((added.at, added.text.as_str(), added.length), (0, "", None));

        let text = ItemOp::SetNoteText {
            id: id.clone(),
            text: "Lights\nup".into(),
        };
        let project = edit(&mut session, text).unwrap();
        assert_eq!(note(&project, &id).text, "Lights\nup");
        let length = |length| ItemOp::SetNoteLength {
            id: id.clone(),
            length,
        };
        let project = edit(&mut session, length(Some(50.4))).unwrap();
        assert_eq!(note(&project, &id).length, Some(50));
        let project = edit(&mut session, length(Some(0.0))).unwrap();
        assert_eq!(note(&project, &id).length, None);
        assert!(
            edit(
                &mut session,
                ItemOp::SetNoteLength {
                    id: "c1".into(),
                    length: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn add_note_should_create_a_notes_track_when_there_is_none_and_prefer_the_one_it_is_given() {
        let mut project = sample_project();
        project.timelines[0]
            .tracks
            .retain(|track| !matches!(track.kind, TrackKind::Notes { .. }));
        let before = project.timelines[0].tracks.len();
        let id = add_note(&mut project, "l1", None, 120.0, " Notes ").unwrap();
        let created = &project.timelines[0].tracks[before];
        assert_eq!(created.name, "Notes");
        let TrackKind::Notes { notes } = &created.kind else {
            panic!("notes");
        };
        assert_eq!((notes[0].id.as_str(), notes[0].at), (id.as_str(), 120));

        let first = created.id.clone();
        let more = new_track("More".into(), TrackKind::Notes { notes: Vec::new() });
        let more_id = more.id.clone();
        project.timelines[0].tracks.push(more);
        add_note(&mut project, "l1", Some(&more_id), 5.0, "x").unwrap();
        add_note(&mut project, "l1", Some("t1"), 6.0, "x").unwrap();
        let counts: Vec<usize> = [&first, &more_id]
            .iter()
            .map(|id| match &track(&project, id).kind {
                TrackKind::Notes { notes } => notes.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(
            counts,
            [2, 1],
            "t1 is not notes, so that note took the first notes track"
        );
    }

    #[test]
    fn clip_gain_should_be_set_and_refuse_what_is_not_a_number() {
        let mut session = session();
        let gain = |gain_db| ItemOp::SetClipGain {
            id: "c1".into(),
            gain_db,
        };
        let project = edit(&mut session, gain(-12.0)).unwrap();
        let TrackKind::Audio { clips } = &track(&project, "t1").kind else {
            panic!("audio");
        };
        assert_eq!(clips[0].gain_db, -12.0);
        assert!(edit(&mut session, gain(f32::NAN)).is_err());
    }

    #[test]
    fn audio_clips_should_each_get_a_track_of_their_own_and_the_file_should_join_the_project() {
        let mut session = session();
        let asset = |id: &str| AudioAsset {
            id: id.into(),
            name: format!("Song {id}"),
            file: format!("audio/{id}.wav"),
            link: None,
            bpm: None,
        };
        let added = session
            .item_edit(ItemOp::AddAudioClips {
                assets: vec![asset("z1"), asset("z2")],
                at: 12.6,
            })
            .unwrap();
        let project = added.state.project;
        assert_eq!(project.assets.len(), 3);
        assert_eq!(ids(&project).len(), 6);
        let last = &project.timelines[0].tracks[5];
        assert_eq!(last.name, "Song z2");
        let TrackKind::Audio { clips } = &last.kind else {
            panic!("audio");
        };
        assert_eq!((Some(clips[0].id.clone()), clips[0].start), (added.id, 13));
        let none = ItemOp::AddAudioClips {
            assets: vec![],
            at: 0.0,
        };
        assert!(edit(&mut session, none).is_err());
    }

    #[test]
    fn remove_track_should_take_its_audio_with_it_and_keep_what_another_track_plays() {
        let mut session = session();
        let extra = AudioAsset {
            id: "a2".into(),
            name: "Other".into(),
            file: "audio/other.wav".into(),
            link: None,
            bpm: None,
        };
        let added = ItemOp::AddAudioClips {
            assets: vec![extra],
            at: 0.0,
        };
        edit(&mut session, added).unwrap();
        let project = edit(&mut session, ItemOp::RemoveTrack { id: "t1".into() }).unwrap();
        assert!(!ids(&project).contains(&"t1"));
        let assets: Vec<&str> = project.assets.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(assets, ["a2"]);
        assert!(edit(&mut session, ItemOp::RemoveTrack { id: "nope".into() }).is_err());
    }

    #[test]
    fn move_track_should_clamp_the_index() {
        let mut session = session();
        let to = |to| ItemOp::MoveTrack {
            id: "t1".into(),
            to,
        };
        let project = edit(&mut session, to(99)).unwrap();
        assert_eq!(ids(&project), ["t2", "t3", "t4", "t1"]);
        let project = edit(&mut session, to(-4)).unwrap();
        assert_eq!(ids(&project), ["t1", "t2", "t3", "t4"]);
        let project = edit(&mut session, to(1)).unwrap();
        assert_eq!(ids(&project), ["t2", "t1", "t3", "t4"]);
        let nope = ItemOp::MoveTrack {
            id: "nope".into(),
            to: 0,
        };
        assert!(edit(&mut session, nope).is_err());
    }

    #[test]
    fn mute_gain_and_colour_should_be_stored_as_the_track_has_them() {
        let mut session = session();
        let muted = edit(&mut session, ItemOp::ToggleMute { id: "t1".into() }).unwrap();
        assert!(track(&muted, "t1").muted);
        let unmuted = edit(&mut session, ItemOp::ToggleMute { id: "t1".into() }).unwrap();
        assert!(!track(&unmuted, "t1").muted);

        let gain = |gain_db| ItemOp::SetTrackGain {
            id: "t1".into(),
            gain_db,
        };
        let down = edit(&mut session, gain(-9.0)).unwrap();
        assert_eq!(track(&down, "t1").gain_db, Some(-9.0));
        let reset = edit(&mut session, gain(0.0)).unwrap();
        assert_eq!(track(&reset, "t1").gain_db, None);
        assert!(edit(&mut session, gain(f32::NAN)).is_err());
        let notes = ItemOp::SetTrackGain {
            id: "t2".into(),
            gain_db: -6.0,
        };
        assert!(
            edit(&mut session, notes).is_err(),
            "a notes track has no fader"
        );

        let color = |color: Option<&str>| ItemOp::SetTrackColor {
            id: "t2".into(),
            color: color.map(String::from),
        };
        let coloured = edit(&mut session, color(Some("#66d5ba"))).unwrap();
        assert_eq!(track(&coloured, "t2").color.as_deref(), Some("#66d5ba"));
        let plain = edit(&mut session, color(None)).unwrap();
        assert_eq!(track(&plain, "t2").color, None);
        assert!(edit(&mut session, color(Some("red"))).is_err());
    }

    #[test]
    fn asset_bpm_should_round_refuse_what_is_out_of_range_and_forget_on_none() {
        let mut session = session();
        let set = |bpm| ItemOp::SetAssetBpm {
            id: "a1".into(),
            bpm,
        };
        let project = edit(&mut session, set(Some(127.456))).unwrap();
        assert_eq!(project.assets[0].bpm, Some(127.46));
        assert!(edit(&mut session, set(Some(5.0))).is_err());
        assert!(edit(&mut session, set(Some(9000.0))).is_err());
        assert!(edit(&mut session, set(Some(f64::NAN))).is_err());
        assert_eq!(
            session.document().unwrap().project.assets[0].bpm,
            Some(127.46)
        );
        assert_eq!(edit(&mut session, set(None)).unwrap().assets[0].bpm, None);
        let missing = ItemOp::SetAssetBpm {
            id: "nope".into(),
            bpm: Some(100.0),
        };
        assert!(edit(&mut session, missing).is_err());
    }

    #[test]
    fn an_edit_that_changes_nothing_should_say_so() {
        let mut session = session();
        let again = |gain_db| ItemOp::SetTrackGain {
            id: "t1".into(),
            gain_db,
        };
        assert!(session.item_edit(again(-9.0)).unwrap().changed);
        assert!(!session.item_edit(again(-9.0)).unwrap().changed);
        let mv = ItemOp::MoveTrack {
            id: "t1".into(),
            to: 0,
        };
        assert!(!session.item_edit(mv).unwrap().changed);
        let tempo = |bpm| crate::MarksOp::SetTempo {
            bpm: Some(bpm),
            beats_per_bar: None,
            click: None,
        };
        assert!(session.marks_edit(tempo(100.0)).unwrap().changed);
        assert!(!session.marks_edit(tempo(100.0)).unwrap().changed);
    }

    #[test]
    fn the_project_should_be_renamed() {
        let mut session = session();
        let op = ItemOp::RenameProject {
            name: "Tour".into(),
        };
        assert_eq!(edit(&mut session, op).unwrap().name, "Tour");
    }
}
