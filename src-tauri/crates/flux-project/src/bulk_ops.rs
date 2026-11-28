//! Edits of many items at once: audio and video clips, notes, MIDI and OSC events of the active
//! timeline, picked by id. The window only says which items and by how much; the clamping at the
//! start of the timeline, the ripple and the extent of what is picked live here.
//!
//! Ripple works like REAPER's: with it on, what a move or a delete opens or closes on a track (or on
//! every track) is made up by shifting what comes after it by the same amount.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::error::{ProjectError, Result};
use super::model::{Project, Timeline, TrackKind};

/// What ripple editing shifts when items are moved or deleted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ripple {
    #[default]
    Off,
    /// The later items of each track the edit touches.
    Track,
    /// The later items of every track.
    All,
}

/// An operation on several items of the active timeline.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ItemsOp {
    /// Moves every item by `delta` frames (negative is earlier). The group stops at the start of the
    /// timeline together, so the items keep their distances. With ripple, the later items that are
    /// not picked move by the same delta.
    Move {
        ids: Vec<String>,
        delta: i64,
        #[serde(default)]
        ripple: Ripple,
    },
    /// Removes the items. With ripple, the later items close up over the room they took.
    Remove {
        ids: Vec<String>,
        #[serde(default)]
        ripple: Ripple,
    },
}

impl ItemsOp {
    /// Whether the edit needs the length of clips whose own length is the whole of their media.
    pub fn needs_lengths(&self) -> bool {
        matches!(
            self,
            ItemsOp::Remove {
                ripple: Ripple::Track | Ripple::All,
                ..
            }
        )
    }
}

/// How long the media of the clips lasts, in frames of the timeline, by asset id. A clip with no
/// length of its own plays all of its media; media that is not here counts as lasting nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lengths {
    pub audio: HashMap<String, u64>,
    pub video: HashMap<String, u64>,
}

/// The stretch of the timeline some items cover, in frames from its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Extent {
    pub from: u64,
    pub to: u64,
}

/// One item as the edits see it: where it starts and how long it lasts. `id` is `None` for what is
/// shifted by ripple but never picked (the orders of an operator track).
struct Slot<'a> {
    id: Option<&'a str>,
    at: &'a mut u64,
    len: u64,
}

fn slots<'a>(kind: &'a mut TrackKind, lengths: &Lengths) -> Vec<Slot<'a>> {
    match kind {
        TrackKind::Audio { clips } => clips
            .iter_mut()
            .map(|clip| Slot {
                // Only clips that play all their media need it measured (see `Session::length_assets`).
                len: clip.length.unwrap_or_else(|| {
                    let all = lengths.audio.get(&clip.asset).copied().unwrap_or(0);
                    all.saturating_sub(clip.offset)
                }),
                id: Some(clip.id.as_str()),
                at: &mut clip.start,
            })
            .collect(),
        TrackKind::Video { clips } => clips
            .iter_mut()
            .map(|clip| Slot {
                len: clip.length.unwrap_or_else(|| {
                    let all = lengths.video.get(&clip.asset).copied().unwrap_or(0);
                    all.saturating_sub(clip.offset)
                }),
                id: Some(clip.id.as_str()),
                at: &mut clip.start,
            })
            .collect(),
        TrackKind::Notes { notes } => notes
            .iter_mut()
            .map(|note| Slot {
                len: note.length.unwrap_or(0),
                id: Some(note.id.as_str()),
                at: &mut note.at,
            })
            .collect(),
        TrackKind::Midi { events, .. } => events
            .iter_mut()
            .map(|event| Slot {
                len: 0,
                id: Some(event.id.as_str()),
                at: &mut event.at,
            })
            .collect(),
        TrackKind::Osc { events, .. } => events
            .iter_mut()
            .map(|event| Slot {
                len: 0,
                id: Some(event.id.as_str()),
                at: &mut event.at,
            })
            .collect(),
        TrackKind::Operator { orders, .. } => orders
            .iter_mut()
            .map(|order| Slot {
                len: 0,
                id: None,
                at: &mut order.at,
            })
            .collect(),
    }
}

fn picked(slot: &Slot, ids: &HashSet<&str>) -> bool {
    slot.id.is_some_and(|id| ids.contains(id))
}

/// Refuses an empty pick and ids that are not items of the timeline.
fn check(timeline: &mut Timeline, ids: &HashSet<&str>, lengths: &Lengths) -> Result<()> {
    if ids.is_empty() {
        return Err(ProjectError::Invalid("no items are picked".into()));
    }
    let found = timeline
        .tracks
        .iter_mut()
        .flat_map(|track| slots(&mut track.kind, lengths))
        .filter(|slot| picked(slot, ids))
        .count();
    if found == ids.len() {
        return Ok(());
    }
    Err(ProjectError::Invalid(
        "some of the items are not in the timeline".into(),
    ))
}

/// The stretches `spans` cover, in order and with those that touch joined.
fn merge(mut spans: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    spans.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (from, to) in spans {
        match merged.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    merged
}

/// How far an item starting at `at` closes up over the stretches that were removed before it.
fn closed_by(at: u64, gaps: &[(u64, u64)]) -> u64 {
    gaps.iter()
        .map(|&(from, to)| at.saturating_sub(from).min(to - from))
        .sum()
}

fn move_items(
    timeline: &mut Timeline,
    ids: &HashSet<&str>,
    delta: i64,
    ripple: Ripple,
    lengths: &Lengths,
) -> Result<()> {
    check(timeline, ids, lengths)?;
    // The first start of what is picked, on each track and on all: the later items are those from it on.
    let firsts: Vec<Option<u64>> = timeline
        .tracks
        .iter_mut()
        .map(|track| {
            slots(&mut track.kind, lengths)
                .iter()
                .filter(|slot| picked(slot, ids))
                .map(|slot| *slot.at)
                .min()
        })
        .collect();
    let first = firsts.iter().flatten().min().copied().unwrap_or(0);
    // The whole group stops at the start of the timeline.
    let delta = delta.max(-i64::try_from(first).unwrap_or(i64::MAX));
    for (track, own) in timeline.tracks.iter_mut().zip(&firsts) {
        let from = match ripple {
            Ripple::Off => None,
            Ripple::Track => *own,
            Ripple::All => Some(first),
        };
        for slot in slots(&mut track.kind, lengths) {
            if picked(&slot, ids) || from.is_some_and(|from| *slot.at >= from) {
                *slot.at = slot.at.saturating_add_signed(delta);
            }
        }
    }
    Ok(())
}

fn remove_items(
    project: &mut Project,
    active: &str,
    ids: &HashSet<&str>,
    ripple: Ripple,
    lengths: &Lengths,
) -> Result<()> {
    let timeline = timeline_mut(project, active)?;
    check(timeline, ids, lengths)?;
    let spans: Vec<Vec<(u64, u64)>> = timeline
        .tracks
        .iter_mut()
        .map(|track| {
            slots(&mut track.kind, lengths)
                .iter()
                .filter(|slot| picked(slot, ids) && slot.len > 0)
                .map(|slot| (*slot.at, *slot.at + slot.len))
                .collect()
        })
        .collect();
    let everywhere = merge(spans.iter().flatten().copied().collect());
    let mut audio_removed = false;
    for (track, own) in timeline.tracks.iter_mut().zip(spans) {
        match &mut track.kind {
            TrackKind::Audio { clips } => {
                let before = clips.len();
                clips.retain(|clip| !ids.contains(clip.id.as_str()));
                audio_removed |= clips.len() != before;
            }
            TrackKind::Video { clips } => clips.retain(|clip| !ids.contains(clip.id.as_str())),
            TrackKind::Notes { notes } => notes.retain(|note| !ids.contains(note.id.as_str())),
            TrackKind::Midi { events, .. } => {
                events.retain(|event| !ids.contains(event.id.as_str()));
            }
            TrackKind::Osc { events, .. } => {
                events.retain(|event| !ids.contains(event.id.as_str()));
            }
            TrackKind::Operator { .. } => {}
        }
        let gaps = match ripple {
            Ripple::Off => Vec::new(),
            Ripple::Track => merge(own),
            Ripple::All => everywhere.clone(),
        };
        if !gaps.is_empty() {
            for slot in slots(&mut track.kind, lengths) {
                *slot.at -= closed_by(*slot.at, &gaps);
            }
        }
    }
    // As the other ways of removing do: a song or a video left with no clip leaves the timeline.
    timeline.tracks.retain(|track| match &track.kind {
        TrackKind::Audio { clips } => !clips.is_empty(),
        TrackKind::Video { clips } => !clips.is_empty(),
        _ => true,
    });
    if audio_removed {
        let used: HashSet<String> = project
            .used_assets()
            .into_iter()
            .map(String::from)
            .collect();
        project.assets.retain(|asset| used.contains(&asset.id));
    }
    Ok(())
}

fn timeline_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("there is no timeline {id}")))
}

/// Applies `op` to the timeline `active` of `project`.
pub fn apply(project: &mut Project, active: &str, op: ItemsOp, lengths: &Lengths) -> Result<()> {
    match op {
        ItemsOp::Move { ids, delta, ripple } => {
            let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
            move_items(timeline_mut(project, active)?, &ids, delta, ripple, lengths)
        }
        ItemsOp::Remove { ids, ripple } => {
            let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
            remove_items(project, active, &ids, ripple, lengths)
        }
    }
}

/// The stretch of the timeline `active` that the items `ids` cover, or all of its content when no
/// ids are given; `None` when there is nothing. Points (events, notes with no length) cover no time.
pub fn extent(
    project: &Project,
    active: &str,
    ids: &[String],
    lengths: &Lengths,
) -> Result<Option<Extent>> {
    let mut timeline = project
        .timeline(active)
        .cloned()
        .ok_or_else(|| ProjectError::Invalid(format!("there is no timeline {active}")))?;
    let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
    if !ids.is_empty() {
        check(&mut timeline, &ids, lengths)?;
    }
    let spans = timeline
        .tracks
        .iter_mut()
        .flat_map(|track| slots(&mut track.kind, lengths))
        .filter(|slot| ids.is_empty() || picked(slot, &ids))
        .map(|slot| (*slot.at, *slot.at + slot.len))
        .collect::<Vec<_>>();
    let from = spans.iter().map(|span| span.0).min();
    let to = spans.iter().map(|span| span.1).max();
    Ok(from.zip(to).map(|(from, to)| Extent { from, to }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{MidiEvent, MidiMessage};
    use crate::model::tests::sample_project;
    use crate::model::{AudioClip, Note, Track};

    fn lengths() -> Lengths {
        Lengths {
            audio: HashMap::from([("a1".to_string(), 100)]),
            video: HashMap::new(),
        }
    }

    fn track(id: &str, kind: TrackKind) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted: false,
            gain_db: None,
            color: None,
            kind,
        }
    }

    fn clip(id: &str, start: u64, length: Option<u64>) -> AudioClip {
        AudioClip {
            length,
            ..AudioClip::new(id, "a1", start)
        }
    }

    fn note(id: &str, at: u64) -> Note {
        Note {
            id: id.into(),
            at,
            length: None,
            text: String::new(),
        }
    }

    fn event(id: &str, at: u64) -> MidiEvent {
        MidiEvent {
            first_turn_only: false,
            id: id.into(),
            at,
            message: MidiMessage::ProgramChange {
                channel: 1,
                program: 0,
            },
        }
    }

    /// Two audio tracks (clips of 10 frames at 0, 20, 40 and at 5, 30), a notes track (at 10, 30,
    /// 50) and a MIDI track (at 15, 35).
    fn project() -> Project {
        let mut project = sample_project();
        project.timelines[0].tracks = vec![
            track(
                "t1",
                TrackKind::Audio {
                    clips: vec![
                        clip("c1", 0, Some(10)),
                        clip("c2", 20, Some(10)),
                        clip("c3", 40, Some(10)),
                    ],
                },
            ),
            track(
                "t2",
                TrackKind::Audio {
                    clips: vec![clip("c4", 5, Some(10)), clip("c5", 30, Some(10))],
                },
            ),
            track(
                "t3",
                TrackKind::Notes {
                    notes: vec![note("n1", 10), note("n2", 30), note("n3", 50)],
                },
            ),
            track(
                "t4",
                TrackKind::Midi {
                    port: None,
                    events: vec![event("m1", 15), event("m2", 35)],
                },
            ),
        ];
        project
    }

    fn run(project: &mut Project, op: ItemsOp) -> Result<()> {
        apply(project, "l1", op, &lengths())
    }

    fn mv(ids: &[&str], delta: i64, ripple: Ripple) -> ItemsOp {
        ItemsOp::Move {
            ids: ids.iter().map(ToString::to_string).collect(),
            delta,
            ripple,
        }
    }

    fn rm(ids: &[&str], ripple: Ripple) -> ItemsOp {
        ItemsOp::Remove {
            ids: ids.iter().map(ToString::to_string).collect(),
            ripple,
        }
    }

    /// Every item's start, track by track, in the order the tracks hold them.
    fn starts(project: &mut Project) -> Vec<Vec<(String, u64)>> {
        let none = Lengths::default();
        project.timelines[0]
            .tracks
            .iter_mut()
            .map(|track| {
                slots(&mut track.kind, &none)
                    .into_iter()
                    .map(|slot| (slot.id.unwrap_or("").to_string(), *slot.at))
                    .collect()
            })
            .collect()
    }

    fn at(project: &mut Project, track: usize) -> Vec<u64> {
        starts(project)[track].iter().map(|item| item.1).collect()
    }

    #[test]
    fn move_should_shift_the_picked_items_across_tracks_only() {
        let mut project = project();
        run(&mut project, mv(&["c2", "n2", "m1"], 3, Ripple::Off)).unwrap();
        assert_eq!(at(&mut project, 0), [0, 23, 40]);
        assert_eq!(at(&mut project, 1), [5, 30]);
        assert_eq!(at(&mut project, 2), [10, 33, 50]);
        assert_eq!(at(&mut project, 3), [18, 35]);
    }

    #[test]
    fn move_should_stop_the_whole_group_at_the_start_of_the_timeline() {
        let mut project = project();
        // c4 is at 5 and c2 at 20: the group can only go 5 frames earlier.
        run(&mut project, mv(&["c2", "c4"], -50, Ripple::Off)).unwrap();
        assert_eq!(at(&mut project, 0), [0, 15, 40]);
        assert_eq!(at(&mut project, 1), [0, 30]);
    }

    #[test]
    fn move_should_keep_the_order_of_the_items_on_a_track() {
        let mut project = project();
        run(&mut project, mv(&["c1", "c2", "c3"], 7, Ripple::Off)).unwrap();
        let ids: Vec<_> = starts(&mut project)[0]
            .iter()
            .map(|item| item.0.clone())
            .collect();
        assert_eq!(ids, ["c1", "c2", "c3"]);
        assert_eq!(at(&mut project, 0), [7, 27, 47]);
    }

    #[test]
    fn move_with_ripple_per_track_should_shift_the_later_items_of_the_tracks_it_touches() {
        let mut project = project();
        run(&mut project, mv(&["c2"], 4, Ripple::Track)).unwrap();
        assert_eq!(at(&mut project, 0), [0, 24, 44]);
        // Other tracks do not have a picked item, so nothing moves there.
        assert_eq!(at(&mut project, 1), [5, 30]);
        assert_eq!(at(&mut project, 2), [10, 30, 50]);
        assert_eq!(at(&mut project, 3), [15, 35]);
    }

    #[test]
    fn move_with_ripple_on_all_tracks_should_shift_everything_from_the_first_picked_start() {
        let mut project = project();
        run(&mut project, mv(&["c2"], 4, Ripple::All)).unwrap();
        assert_eq!(at(&mut project, 0), [0, 24, 44]);
        assert_eq!(at(&mut project, 1), [5, 34]);
        assert_eq!(at(&mut project, 2), [10, 34, 54]);
        assert_eq!(at(&mut project, 3), [15, 39]);
    }

    #[test]
    fn move_with_ripple_should_clamp_and_shift_the_later_items_by_what_was_left() {
        let mut project = project();
        run(&mut project, mv(&["c2"], -50, Ripple::Track)).unwrap();
        // c2 can only go back 20 frames; c3 follows by the same 20.
        assert_eq!(at(&mut project, 0), [0, 0, 20]);
    }

    #[test]
    fn move_should_refuse_an_empty_pick_and_ids_that_are_not_items_and_change_nothing() {
        let mut project = project();
        let before = project.clone();
        assert!(run(&mut project, mv(&[], 3, Ripple::Off)).is_err());
        assert!(run(&mut project, mv(&["c1", "nope"], 3, Ripple::Off)).is_err());
        assert_eq!(project, before);
    }

    #[test]
    fn remove_should_take_the_items_away_and_leave_the_rest_where_it_is() {
        let mut project = project();
        run(&mut project, rm(&["c2", "n1", "m2"], Ripple::Off)).unwrap();
        assert_eq!(at(&mut project, 0), [0, 40]);
        assert_eq!(at(&mut project, 2), [30, 50]);
        assert_eq!(at(&mut project, 3), [15]);
    }

    #[test]
    fn remove_with_ripple_per_track_should_close_the_gap_on_that_track_only() {
        let mut project = project();
        run(&mut project, rm(&["c2"], Ripple::Track)).unwrap();
        // c2 lasted 10 frames: c3 comes 10 earlier.
        assert_eq!(at(&mut project, 0), [0, 30]);
        assert_eq!(at(&mut project, 1), [5, 30]);
        assert_eq!(at(&mut project, 2), [10, 30, 50]);
    }

    #[test]
    fn remove_with_ripple_on_all_tracks_should_close_the_gap_everywhere() {
        let mut project = project();
        run(&mut project, rm(&["c2"], Ripple::All)).unwrap();
        // Gap [20, 30): what starts at 30 or later moves 10 earlier; c4 at 5 stays.
        assert_eq!(at(&mut project, 0), [0, 30]);
        assert_eq!(at(&mut project, 1), [5, 20]);
        assert_eq!(at(&mut project, 2), [10, 20, 40]);
        assert_eq!(at(&mut project, 3), [15, 25]);
    }

    #[test]
    fn remove_with_ripple_should_add_up_the_gaps_before_each_item() {
        let mut project = project();
        run(&mut project, rm(&["c1", "c2"], Ripple::Track)).unwrap();
        // Gaps [0, 10) and [20, 30): c3 at 40 moves 20 earlier.
        assert_eq!(at(&mut project, 0), [20]);
    }

    #[test]
    fn remove_with_ripple_should_join_gaps_that_overlap_across_tracks() {
        let mut project = project();
        // Gaps [20, 30) and [30, 40) join: n3 at 50 moves 20, n2 at 30 moves 10.
        run(&mut project, rm(&["c2", "c5"], Ripple::All)).unwrap();
        assert_eq!(at(&mut project, 2), [10, 20, 30]);
    }

    #[test]
    fn remove_with_ripple_should_not_shift_for_points() {
        let mut project = project();
        run(&mut project, rm(&["n1", "m1"], Ripple::All)).unwrap();
        assert_eq!(at(&mut project, 2), [30, 50]);
        assert_eq!(at(&mut project, 3), [35]);
    }

    #[test]
    fn remove_with_ripple_should_take_the_length_of_a_clip_from_its_media() {
        let mut project = project();
        // c1 plays all of a1: 100 frames.
        project.timelines[0].tracks[0].kind = TrackKind::Audio {
            clips: vec![clip("c1", 0, None), clip("c2", 150, Some(10))],
        };
        run(&mut project, rm(&["c1"], Ripple::Track)).unwrap();
        assert_eq!(at(&mut project, 0), [50]);
    }

    #[test]
    fn remove_with_ripple_should_use_the_own_length_of_a_clip_without_reading_its_media() {
        let mut project = project();
        // Only clips with no length of their own have their media measured, so none is known here.
        apply(
            &mut project,
            "l1",
            rm(&["c2"], Ripple::Track),
            &Lengths::default(),
        )
        .unwrap();
        assert_eq!(at(&mut project, 0), [0, 30]);
    }

    #[test]
    fn remove_should_drop_an_audio_track_left_empty_and_its_unused_asset() {
        let mut project = project();
        run(&mut project, rm(&["c4", "c5"], Ripple::Off)).unwrap();
        assert_eq!(project.timelines[0].tracks.len(), 3);
        assert_eq!(project.assets.len(), 1);
        run(&mut project, rm(&["c1", "c2", "c3"], Ripple::Off)).unwrap();
        assert_eq!(project.timelines[0].tracks.len(), 2);
        assert!(project.assets.is_empty());
    }

    #[test]
    fn remove_should_refuse_ids_that_are_not_items_and_change_nothing() {
        let mut project = project();
        let before = project.clone();
        assert!(run(&mut project, rm(&["c1", "nope"], Ripple::Track)).is_err());
        assert_eq!(project, before);
    }

    #[test]
    fn extent_should_cover_the_picked_items_or_the_whole_timeline() {
        let project = project();
        let of = |ids: &[&str]| {
            let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
            extent(&project, "l1", &ids, &lengths()).unwrap()
        };
        assert_eq!(of(&["c2", "n1"]), Some(Extent { from: 10, to: 30 }));
        assert_eq!(of(&["m1"]), Some(Extent { from: 15, to: 15 }));
        assert_eq!(of(&[]), Some(Extent { from: 0, to: 50 }));
        let mut empty = project.clone();
        empty.timelines[0].tracks.clear();
        assert_eq!(extent(&empty, "l1", &[], &lengths()).unwrap(), None);
    }

    #[test]
    fn ops_should_read_from_the_camel_case_the_window_sends() {
        let op: ItemsOp =
            serde_json::from_str(r#"{"op":"move","ids":["c1"],"delta":-3,"ripple":"track"}"#)
                .unwrap();
        assert_eq!(op, mv(&["c1"], -3, Ripple::Track));
        let op: ItemsOp = serde_json::from_str(r#"{"op":"remove","ids":["c1"]}"#).unwrap();
        assert_eq!(op, rm(&["c1"], Ripple::Off));
    }
}
