//! The cues of the Live view and where the transport stands among them.
//!
//! A cue is not stored in the project: every clip of an audio track and every trigger of an events
//! track is one, in the order the show reaches it. [`CueList::build`] derives the list and
//! [`CueList::state_at`] says, for a position, which cue is next, which are sounding and which are
//! done. The UI only draws what comes out, so every time is already written as timecode here. Nothing
//! in this module touches a device or waits, which keeps it testable and cheap enough to run about
//! thirty times a second.

use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use crate::audio::ENGINE_RATE;
use crate::engine::timecode::Timecode;
use crate::engine::transport::Clock;
use crate::project::{AudioAsset, FrameRate, Project, TrackKind, TriggerAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CueKind {
    Audio,
    Marker,
    Midi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    /// Id of the clip or trigger.
    pub id: String,
    /// Place in the show, from 1.
    pub number: u32,
    pub kind: CueKind,
    /// The audio file's name or the trigger's label; a trigger can have none.
    pub name: String,
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
}

/// The cues of a project, in the order the show reaches them.
#[derive(Debug, Clone, PartialEq)]
pub struct CueList {
    rate: FrameRate,
    cues: Vec<Cue>,
}

impl CueList {
    /// Derives the cues of `project`. `length_of` gives the samples (at the engine rate) of an
    /// asset's audio; one it cannot give leaves the clip without length rather than failing,
    /// because a file that went missing must not stop the show.
    pub fn build(project: &Project, mut length_of: impl FnMut(&AudioAsset) -> Option<u64>) -> Self {
        let rate = project.timecode.frame_rate;
        let start = project.timecode.start;
        let clock = Clock::new(ENGINE_RATE, rate);
        let text = |frames| Timecode::from_frames(frames, rate).display(rate);
        let mut cues = Vec::new();

        for track in &project.tracks {
            let cue = |id: &str, kind, name: &str, at: u64, length: Option<u64>| Cue {
                id: id.to_owned(),
                number: 0,
                kind,
                name: name.to_owned(),
                track_id: track.id.clone(),
                track_name: track.name.clone(),
                color: track.color.clone(),
                muted: track.muted,
                frame: start + at,
                timecode: text(start + at),
                length,
                duration: length.map(text),
            };
            match &track.kind {
                TrackKind::Audio { clips } => {
                    for clip in clips {
                        let Some(asset) = project.assets.iter().find(|a| a.id == clip.asset) else {
                            continue;
                        };
                        let length =
                            length_of(asset).map(|samples| clock.samples_to_frames(samples));
                        cues.push(cue(
                            &clip.id,
                            CueKind::Audio,
                            &asset.name,
                            clip.start,
                            length,
                        ));
                    }
                }
                TrackKind::Events { triggers } => {
                    for trigger in triggers {
                        let kind = match trigger.action {
                            TriggerAction::Marker => CueKind::Marker,
                            TriggerAction::Midi { .. } => CueKind::Midi,
                        };
                        cues.push(cue(&trigger.id, kind, &trigger.label, trigger.at, None));
                    }
                }
            }
        }

        // Stable, so cues at the same frame keep the order of their tracks.
        cues.sort_by_key(|cue| cue.frame);
        for (cue, number) in cues.iter_mut().zip(1..) {
            cue.number = number;
        }
        Self { rate, cues }
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
        }
    }
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
    use crate::project::{AudioClip, Track, Trigger};

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
        AudioClip {
            id: id.into(),
            asset: "a1".into(),
            start,
            gain_db: 0.0,
        }
    }

    fn trigger(id: &str, at: u64, action: TriggerAction) -> Trigger {
        Trigger {
            id: id.into(),
            at,
            label: id.to_uppercase(),
            action,
        }
    }

    /// A 25 fps project starting at frame 100 with a two-second asset `a1`, a muted-able audio
    /// track `t1` (clips `c1` at 0 and `c2` at 100) and an events track `t2` (marker `m1` at 50).
    fn project(muted: bool) -> Project {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timecode.start = 100;
        project.assets.push(AudioAsset {
            id: "a1".into(),
            name: "Intro".into(),
            file: "audio/intro.wav".into(),
        });
        project.tracks.push(track(
            "t1",
            muted,
            TrackKind::Audio {
                clips: vec![clip("c2", 100), clip("c1", 0)],
            },
        ));
        project.tracks.push(track(
            "t2",
            false,
            TrackKind::Events {
                triggers: vec![trigger("m1", 50, TriggerAction::Marker)],
            },
        ));
        project
    }

    fn list(project: &Project) -> CueList {
        CueList::build(project, |_| Some(2 * SECOND))
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
    fn build_should_keep_the_track_order_for_cues_at_the_same_frame() {
        let mut project = project(false);
        project.tracks[1].kind = TrackKind::Events {
            triggers: vec![trigger("m1", 0, TriggerAction::Marker)],
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
    fn build_should_leave_a_clip_without_length_when_its_audio_is_unknown() {
        let list = CueList::build(&project(false), |_| None);
        assert_eq!(list.cues()[0].length, None);
    }

    #[test]
    fn build_should_tell_markers_from_midi_triggers() {
        let mut project = project(false);
        project.tracks[1].kind = TrackKind::Events {
            triggers: vec![trigger(
                "x1",
                0,
                TriggerAction::Midi {
                    bytes: vec![0xC0, 1],
                },
            )],
        };
        let kinds: Vec<CueKind> = list(&project).cues().iter().map(|cue| cue.kind).collect();
        assert!(kinds.contains(&CueKind::Midi));
        assert!(!kinds.contains(&CueKind::Marker));
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
    fn state_at_should_finish_a_trigger_once_its_frame_is_reached() {
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
        // The marker at 150 is done, but the clip that started at 200 sounds.
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
        let list = CueList::build(&project(false), |_| None);
        assert_eq!(list.state_at(100).statuses[0], CueStatus::Done);
    }

    #[test]
    fn state_at_should_write_times_with_a_semicolon_in_drop_frame() {
        let mut project = project(false);
        project.timecode.frame_rate = FrameRate::Fps2997Drop;
        assert!(list(&project).cues()[0].timecode.contains(';'));
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
