//! The cues of the Live view and where the transport stands among them.
//!
//! A cue is not stored in the project: every clip of an audio track and every note of a notes track
//! is one, in the order the show reaches it. [`CueList::build`] derives the list and
//! [`CueList::state_at`] says, for a position, which cue is next, which are sounding and which are
//! done. The UI only draws what comes out, so every time is already written as timecode here. Nothing
//! in this module touches a device or waits, which keeps it testable and cheap enough to run about
//! thirty times a second.

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::audio::ENGINE_RATE;
use crate::engine::mixer::span;
use crate::engine::timecode::Timecode;
use crate::engine::transport::Clock;
use crate::project::{AudioAsset, FrameRate, Project, Timeline, TrackKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CueKind {
    Audio,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    /// Id of the clip or note.
    pub id: String,
    /// Place in the show, from 1.
    pub number: u32,
    pub kind: CueKind,
    /// The audio file's name or the first line of a note; a note can have none.
    pub name: String,
    /// The whole text of a note.
    pub text: Option<String>,
    pub track_id: String,
    pub track_name: String,
    pub color: Option<String>,
    pub muted: bool,
    /// Timecode frame where the cue starts, counting the project's start offset: what to seek to.
    pub frame: u64,
    pub timecode: String,
    /// Frames the cue lasts, for a clip whose audio could be read.
    pub length: Option<u64>,
    pub duration: Option<String>,
    /// Where a note stops being shown: its length, or else the next note of its track, or never.
    #[serde(skip)]
    shown_until: Option<u64>,
}

impl Cue {
    /// The frame after the last one the cue occupies; a cue without length takes no time.
    fn end(&self) -> u64 {
        self.frame + self.length.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CueStatus {
    /// Still to come.
    Pending,
    /// The first cue to come: the one on standby.
    Next,
    /// A clip that is sounding.
    Running,
    Done,
    /// Its track is muted, so it never plays.
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Countdown {
    pub cue_id: String,
    pub remaining: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningCue {
    pub cue_id: String,
    pub elapsed: String,
    pub remaining: String,
    /// How much of the clip has played, from 0 to 1.
    pub progress: f32,
}

/// A note that is on show now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteNow {
    pub cue_id: String,
    pub text: String,
    /// Time left of a note with a length.
    pub remaining: Option<String>,
}

/// Where the transport stands among the cues.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    /// The status of every cue, in the order of the list.
    pub statuses: Vec<CueStatus>,
    /// The cue the show is at: the clip that started last and still sounds, or else the last one done.
    pub current: Option<String>,
    pub next: Option<Countdown>,
    pub running: Vec<RunningCue>,
    /// The notes on show now, in the order of the list.
    pub notes: Vec<NoteNow>,
}

/// The cues of a project, in the order the show reaches them.
#[derive(Debug, Clone, PartialEq)]
pub struct CueList {
    rate: FrameRate,
    cues: Vec<Cue>,
    /// Samples (at the engine rate) since the timeline origin where the last cue ends.
    end: u64,
}

impl CueList {
    /// Derives the cues of `timeline`, a timeline of `project`. `length_of` gives the samples (at the engine rate) of an
    /// asset's audio; one it cannot give leaves the clip without length rather than failing,
    /// because a file that went missing must not stop the show.
    pub fn build(
        project: &Project,
        timeline: &Timeline,
        mut length_of: impl FnMut(&AudioAsset) -> Option<u64>,
    ) -> Self {
        let rate = timeline.timecode.frame_rate;
        let start = timeline.timecode.start;
        let clock = Clock::new(ENGINE_RATE, rate);
        let text = |frames| Timecode::from_frames(frames, rate).display(rate);
        let mut cues = Vec::new();
        let mut end = 0;

        for track in &timeline.tracks {
            let cue = |id: &str, kind, name: &str, at: u64, length: Option<u64>| Cue {
                id: id.to_owned(),
                number: 0,
                kind,
                name: name.to_owned(),
                text: None,
                track_id: track.id.clone(),
                track_name: track.name.clone(),
                color: track.color.clone(),
                muted: track.muted,
                frame: start + at,
                timecode: text(start + at),
                length,
                duration: length.map(text),
                shown_until: None,
            };
            match &track.kind {
                TrackKind::Audio { clips } => {
                    for clip in clips {
                        let Some(asset) = project.assets.iter().find(|a| a.id == clip.asset) else {
                            continue;
                        };
                        let played = length_of(asset).map(|samples| span(clip, &clock, samples));
                        end = end.max(played.map_or(clock.frames_to_samples(clip.start), |s| s.to));
                        let length = played.map(|s| clock.samples_to_frames(s.to - s.from));
                        cues.push(cue(
                            &clip.id,
                            CueKind::Audio,
                            &asset.name,
                            clip.start,
                            length,
                        ));
                    }
                }
                TrackKind::Notes { notes } => {
                    for note in notes {
                        end = end.max(clock.frames_to_samples(note.at + note.length.unwrap_or(0)));
                        let first_line = note.text.lines().next().unwrap_or_default().trim();
                        let mut item =
                            cue(&note.id, CueKind::Note, first_line, note.at, note.length);
                        item.text = Some(note.text.clone());
                        cues.push(item);
                    }
                }
            }
        }

        // Stable, so cues at the same frame keep the order of their tracks.
        cues.sort_by_key(|cue| cue.frame);
        for (cue, number) in cues.iter_mut().zip(1..) {
            cue.number = number;
        }
        let ends: Vec<Option<u64>> = (0..cues.len()).map(|i| shown_until(&cues, i)).collect();
        for (cue, end) in cues.iter_mut().zip(ends) {
            cue.shown_until = end;
        }
        Self { rate, cues, end }
    }

    /// Where the show ends: the latest end among all cues, muted ones too, in samples since the
    /// timeline origin. A clip's end uses its exact decoded length, not the length in whole frames,
    /// so the last sound is never cut. A cue without length (a note, or a file that cannot be read)
    /// ends where it starts. Zero when there are no cues.
    pub fn end_samples(&self) -> u64 {
        self.end
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// The state of the show when the transport is at `frame` (absolute, as the position reports it).
    pub fn state_at(&self, frame: u64) -> LiveState {
        let text = |frames| Timecode::from_frames(frames, self.rate).display(self.rate);
        let mut statuses = Vec::with_capacity(self.cues.len());
        let mut next = None;
        let mut running = Vec::new();
        let mut notes = Vec::new();
        let (mut last_running, mut last_done) = (None, None);

        for cue in &self.cues {
            let status = if cue.muted {
                CueStatus::Muted
            } else if cue.frame > frame {
                if next.is_none() {
                    next = Some(Countdown {
                        cue_id: cue.id.clone(),
                        remaining: cue.frame - frame,
                        text: text(cue.frame - frame),
                    });
                    CueStatus::Next
                } else {
                    CueStatus::Pending
                }
            } else if frame < cue.end() {
                let length = cue.end() - cue.frame;
                let elapsed = frame - cue.frame;
                running.push(RunningCue {
                    cue_id: cue.id.clone(),
                    elapsed: text(elapsed),
                    remaining: text(length - elapsed),
                    progress: elapsed as f32 / length as f32,
                });
                CueStatus::Running
            } else {
                CueStatus::Done
            };
            if cue.kind == CueKind::Note
                && !cue.muted
                && cue.frame <= frame
                && cue.shown_until.is_none_or(|end| frame < end)
            {
                notes.push(NoteNow {
                    cue_id: cue.id.clone(),
                    text: cue.text.clone().unwrap_or_default(),
                    remaining: cue.length.map(|length| text(cue.frame + length - frame)),
                });
            }
            // Cues come in order, so the last of each kind is the one that started latest.
            match status {
                CueStatus::Running => last_running = Some(&cue.id),
                CueStatus::Done => last_done = Some(&cue.id),
                _ => {}
            }
            statuses.push(status);
        }

        LiveState {
            statuses,
            current: last_running.or(last_done).cloned(),
            next,
            running,
            notes,
        }
    }
}

/// Where cue `index` stops being shown. Only notes are shown over time: one with a length for that
/// long, one without until the next note of its track that starts later, or for the rest of the show.
fn shown_until(cues: &[Cue], index: usize) -> Option<u64> {
    let cue = &cues[index];
    if cue.kind != CueKind::Note {
        return None;
    }
    cue.length.map(|length| cue.frame + length).or_else(|| {
        cues[index + 1..]
            .iter()
            .find(|other| {
                other.kind == CueKind::Note
                    && other.track_id == cue.track_id
                    && other.frame > cue.frame
            })
            .map(|other| other.frame)
    })
}

/// The cue list the live lane reads, replaced whole when the project's tracks change, so the lane
/// never waits for a rebuild (the same pattern as `SharedLevels`).
#[derive(Debug, Clone)]
pub struct SharedCues(Arc<Mutex<Arc<CueList>>>);

impl SharedCues {
    pub fn new(list: CueList) -> Self {
        Self(Arc::new(Mutex::new(Arc::new(list))))
    }

    pub fn replace(&self, list: CueList) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(list);
    }

    pub fn current(&self) -> Arc<CueList> {
        Arc::clone(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AudioClip, Note, Track};

    /// One second of 25 fps video at the engine rate.
    const SECOND: u64 = ENGINE_RATE as u64;

    fn track(id: &str, muted: bool, kind: TrackKind) -> Track {
        Track {
            id: id.into(),
            name: format!("Track {id}"),
            muted,
            gain_db: None,
            color: None,
            kind,
        }
    }

    fn clip(id: &str, start: u64) -> AudioClip {
        AudioClip::new(id, "a1", start)
    }

    fn note(id: &str, at: u64, length: Option<u64>, text: &str) -> Note {
        Note {
            id: id.into(),
            at,
            length,
            text: text.into(),
        }
    }

    /// A 25 fps project starting at frame 100 with a two-second asset `a1`, a muted-able audio
    /// track `t1` (clips `c1` at 0 and `c2` at 100) and a notes track `t2` (note `m1` at 50, with no length).
    fn project(muted: bool) -> Project {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].timecode.start = 100;
        project.assets.push(AudioAsset {
            id: "a1".into(),
            name: "Intro".into(),
            file: "audio/intro.wav".into(),
            bpm: None,
        });
        project.timelines[0].tracks.push(track(
            "t1",
            muted,
            TrackKind::Audio {
                clips: vec![clip("c2", 100), clip("c1", 0)],
            },
        ));
        project.timelines[0].tracks.push(track(
            "t2",
            false,
            TrackKind::Notes {
                notes: vec![note("m1", 50, None, "M1")],
            },
        ));
        project
    }

    fn list(project: &Project) -> CueList {
        CueList::build(project, &project.timelines[0], |_| Some(2 * SECOND))
    }

    fn cue_named(list: &CueList, id: &str) -> Cue {
        list.cues()
            .iter()
            .find(|cue| cue.id == id)
            .cloned()
            .unwrap()
    }

    fn ids(list: &CueList) -> Vec<&str> {
        list.cues().iter().map(|cue| cue.id.as_str()).collect()
    }

    #[test]
    fn build_should_order_cues_by_frame_and_number_them() {
        let list = list(&project(false));
        assert_eq!(ids(&list), ["c1", "m1", "c2"]);
        let numbers: Vec<u32> = list.cues().iter().map(|cue| cue.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    #[test]
    fn end_samples_should_be_the_latest_end_of_the_clips_muted_ones_too() {
        // c2 starts at frame 100 (4 s) and lasts 2 s.
        assert_eq!(list(&project(false)).end_samples(), 6 * SECOND);
        assert_eq!(list(&project(true)).end_samples(), 6 * SECOND);
    }

    #[test]
    fn end_samples_should_use_the_exact_length_of_a_clip_and_count_notes() {
        let mut project = project(false);
        // 1.5 frames of audio: the end is not rounded down to whole frames.
        let short = CueList::build(&project, &project.timelines[0], |_| Some(2_880));
        assert_eq!(short.end_samples(), 4 * SECOND + 2_880);

        project.timelines[0].tracks[1].kind = TrackKind::Notes {
            notes: vec![note("m1", 250, Some(25), "late")],
        };
        assert_eq!(list(&project).end_samples(), 11 * SECOND);
    }

    #[test]
    fn end_samples_should_be_zero_without_cues() {
        let project = Project::new("Empty", FrameRate::Fps25);
        assert_eq!(list(&project).end_samples(), 0);
    }

    #[test]
    fn build_should_keep_the_track_order_for_cues_at_the_same_frame() {
        let mut project = project(false);
        project.timelines[0].tracks[1].kind = TrackKind::Notes {
            notes: vec![note("m1", 0, None, "M1")],
        };
        assert_eq!(ids(&list(&project))[..2], ["c1", "m1"]);
    }

    #[test]
    fn build_should_count_the_start_offset_in_the_frame_and_the_timecode() {
        let cue = list(&project(false)).cues()[0].clone();
        assert_eq!(cue.frame, 100);
        assert_eq!(cue.timecode, "00:00:04:00");
    }

    #[test]
    fn build_should_take_the_length_of_a_clip_from_its_audio() {
        let cue = list(&project(false)).cues()[0].clone();
        assert_eq!(cue.length, Some(50));
        assert_eq!(cue.duration.as_deref(), Some("00:00:02:00"));
    }

    #[test]
    fn build_should_take_the_trimmed_length_and_end_of_a_clip() {
        let mut project = project(false);
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio")
        };
        // The 2 s asset is 50 frames; 10 are skipped and 20 of the 40 left are played.
        let c1 = clips.iter_mut().find(|clip| clip.id == "c1").unwrap();
        c1.offset = 10;
        c1.length = Some(20);
        let list = list(&project);
        assert_eq!(cue_named(&list, "c1").length, Some(20));
        assert_eq!(
            cue_named(&list, "c1").duration.as_deref(),
            Some("00:00:00:20")
        );
        // The second clip still ends the show: it is later than the first.
        assert_eq!(list.end_samples(), 100 * 1920 + 50 * 1920);
    }

    #[test]
    fn build_should_leave_a_clip_without_length_when_its_audio_is_unknown() {
        let project = project(false);
        let list = CueList::build(&project, &project.timelines[0], |_| None);
        assert_eq!(list.cues()[0].length, None);
    }

    #[test]
    fn build_should_name_a_note_after_its_first_line_and_keep_all_of_its_text() {
        let mut project = project(false);
        project.timelines[0].tracks[1].kind = TrackKind::Notes {
            notes: vec![note("n1", 0, None, "  Singer on\nstage left ")],
        };
        let cue = cue_named(&list(&project), "n1");
        assert_eq!(cue.kind, CueKind::Note);
        assert_eq!(cue.name, "Singer on");
        assert_eq!(cue.text.as_deref(), Some("  Singer on\nstage left "));
    }

    #[test]
    fn build_should_give_a_note_its_own_length() {
        let mut project = project(false);
        project.timelines[0].tracks[1].kind = TrackKind::Notes {
            notes: vec![note("n1", 0, Some(30), "Hold")],
        };
        assert_eq!(cue_named(&list(&project), "n1").length, Some(30));
    }

    #[test]
    fn build_should_skip_a_clip_whose_asset_is_not_in_the_project() {
        let mut project = project(false);
        project.assets.clear();
        assert_eq!(ids(&list(&project)), ["m1"]);
    }

    #[test]
    fn state_at_should_put_only_the_first_cue_on_standby_before_the_show() {
        let state = list(&project(false)).state_at(50);
        assert_eq!(
            state.statuses,
            [CueStatus::Next, CueStatus::Pending, CueStatus::Pending]
        );
        assert_eq!(state.next.unwrap().cue_id, "c1");
    }

    #[test]
    fn state_at_should_count_down_to_the_next_cue() {
        let next = list(&project(false)).state_at(110).next.unwrap();
        assert_eq!(next.cue_id, "m1");
        assert_eq!(next.remaining, 40);
        assert_eq!(next.text, "00:00:01:15");
    }

    #[test]
    fn state_at_should_run_a_clip_from_its_first_frame_to_the_one_before_its_end() {
        let list = list(&project(false));
        assert_eq!(list.state_at(100).statuses[0], CueStatus::Running);
        assert_eq!(list.state_at(149).statuses[0], CueStatus::Running);
        assert_eq!(list.state_at(150).statuses[0], CueStatus::Done);
    }

    #[test]
    fn state_at_should_report_the_progress_of_a_running_clip() {
        let running = list(&project(false)).state_at(125).running;
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].cue_id, "c1");
        assert_eq!(running[0].elapsed, "00:00:01:00");
        assert_eq!(running[0].remaining, "00:00:01:00");
        assert_eq!(running[0].progress, 0.5);
    }

    #[test]
    fn state_at_should_finish_a_note_without_length_once_its_frame_is_reached() {
        let list = list(&project(false));
        assert_eq!(list.state_at(149).statuses[1], CueStatus::Next);
        assert_eq!(list.state_at(150).statuses[1], CueStatus::Done);
    }

    #[test]
    fn state_at_should_have_no_next_cue_after_the_last_one() {
        let state = list(&project(false)).state_at(1_000);
        assert_eq!(state.next, None);
        assert_eq!(state.current.as_deref(), Some("c2"));
    }

    #[test]
    fn state_at_should_prefer_a_sounding_clip_as_the_current_cue() {
        // The note at 150 is done, but the clip that started at 200 sounds.
        let state = list(&project(false)).state_at(210);
        assert_eq!(state.current.as_deref(), Some("c2"));
        let state = list(&project(false)).state_at(160);
        assert_eq!(state.current.as_deref(), Some("m1"));
    }

    #[test]
    fn state_at_should_never_run_or_queue_a_muted_cue() {
        let state = list(&project(true)).state_at(120);
        assert_eq!(state.statuses[0], CueStatus::Muted);
        assert_eq!(state.statuses[2], CueStatus::Muted);
        assert!(state.running.is_empty());
        assert_eq!(state.next.unwrap().cue_id, "m1");
    }

    #[test]
    fn state_at_should_not_run_a_clip_without_length() {
        let project = project(false);
        let list = CueList::build(&project, &project.timelines[0], |_| None);
        assert_eq!(list.state_at(100).statuses[0], CueStatus::Done);
    }

    #[test]
    fn state_at_should_write_times_with_a_semicolon_in_drop_frame() {
        let mut project = project(false);
        project.timelines[0].timecode.frame_rate = FrameRate::Fps2997Drop;
        assert!(list(&project).cues()[0].timecode.contains(';'));
    }

    /// A project with only the notes track `t2` holding `notes`.
    fn with_notes(notes: Vec<Note>) -> Project {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0]
            .tracks
            .push(track("t2", false, TrackKind::Notes { notes }));
        project
    }

    fn shown(list: &CueList, frame: u64) -> Vec<String> {
        list.state_at(frame)
            .notes
            .into_iter()
            .map(|note| note.cue_id)
            .collect()
    }

    #[test]
    fn state_at_should_show_a_note_until_the_next_note_of_its_track_when_it_has_no_length() {
        let list = list(&with_notes(vec![
            note("a", 10, None, "A"),
            note("b", 20, None, "B"),
        ]));
        assert_eq!(shown(&list, 9), Vec::<String>::new());
        assert_eq!(shown(&list, 10), ["a"]);
        assert_eq!(shown(&list, 19), ["a"]);
        assert_eq!(shown(&list, 20), ["b"]);
    }

    #[test]
    fn state_at_should_keep_showing_the_last_note_until_the_end_of_the_show() {
        let list = list(&with_notes(vec![note("a", 10, None, "A")]));
        assert_eq!(shown(&list, 100_000), ["a"]);
    }

    #[test]
    fn state_at_should_show_a_note_with_a_length_for_that_long_only() {
        let list = list(&with_notes(vec![
            note("a", 10, Some(5), "A"),
            note("b", 100, None, "B"),
        ]));
        assert_eq!(shown(&list, 14), ["a"]);
        assert_eq!(shown(&list, 15), Vec::<String>::new());
    }

    #[test]
    fn state_at_should_say_how_long_a_note_with_a_length_has_left() {
        let list = list(&with_notes(vec![note("a", 0, Some(50), "A")]));
        let now = list.state_at(25).notes;
        assert_eq!(now[0].remaining.as_deref(), Some("00:00:01:00"));
        assert_eq!(now[0].text, "A");
    }

    #[test]
    fn state_at_should_show_notes_of_different_tracks_together() {
        let mut project = with_notes(vec![note("a", 0, None, "A")]);
        project.timelines[0].tracks.push(track(
            "t3",
            false,
            TrackKind::Notes {
                notes: vec![note("b", 5, None, "B")],
            },
        ));
        assert_eq!(shown(&list(&project), 10), ["a", "b"]);
    }

    #[test]
    fn state_at_should_not_show_the_notes_of_a_muted_track() {
        let mut project = with_notes(vec![note("a", 0, None, "A")]);
        project.timelines[0].tracks[0].muted = true;
        assert_eq!(shown(&list(&project), 10), Vec::<String>::new());
    }

    #[test]
    fn state_at_should_run_a_note_with_a_length_like_a_clip() {
        let list = list(&with_notes(vec![note("a", 10, Some(5), "A")]));
        assert_eq!(list.state_at(12).statuses[0], CueStatus::Running);
        assert_eq!(list.state_at(15).statuses[0], CueStatus::Done);
    }

    #[test]
    fn shared_cues_should_hand_out_the_list_it_was_last_given() {
        let shared = SharedCues::new(list(&project(false)));
        shared.replace(list(&Project::new("Empty", FrameRate::Fps25)));
        assert!(shared.current().cues().is_empty());
    }

    #[test]
    fn state_at_should_serialise_to_the_shape_the_ui_reads() {
        let state = serde_json::to_value(list(&project(false)).state_at(110)).unwrap();
        assert_eq!(state["statuses"][0], "running");
        assert_eq!(state["next"]["cueId"], "m1");
        assert_eq!(state["current"], "c1");
    }
}
