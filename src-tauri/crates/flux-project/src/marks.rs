//! What the user can do to the markers and regions of a timeline and to its tempo map. The window
//! only says which operation and with what; the numbering of defaults, the trimming of names, the
//! order, the snapping of a tempo change onto a bar and the lookup of the next marker live here, and
//! [`Session::marks_edit`](super::Session::marks_edit) refuses anything that would leave the
//! project invalid.

use serde::Deserialize;
use uuid::Uuid;

use super::error::{ProjectError, Result};
use super::model::{
    MAX_MARK_NAME, MAX_MARKS, Marker, Project, Region, Tempo, TempoChange, Timeline,
};
use super::tempo_map::TempoMap;

/// An operation on the markers, regions and tempo changes of the active timeline. Positions are
/// frames from the timeline origin.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MarksOp {
    /// A marker at `at` called `"{label} {n}"`, `n` being the new count.
    AddMarker {
        at: u64,
        label: String,
    },
    MoveMarker {
        id: String,
        at: u64,
    },
    RenameMarker {
        id: String,
        name: String,
    },
    /// `#rrggbb`, or none for the default colour.
    SetMarkerColor {
        id: String,
        color: Option<String>,
    },
    RemoveMarker {
        id: String,
    },
    /// A region from `start` to `end` called `"{label} {n}"`.
    AddRegion {
        start: u64,
        end: u64,
        label: String,
    },
    /// Moves the region, or resizes it by moving one end.
    SetRegion {
        id: String,
        start: u64,
        end: u64,
    },
    RenameRegion {
        id: String,
        name: String,
    },
    SetRegionColor {
        id: String,
        color: Option<String>,
    },
    RemoveRegion {
        id: String,
    },
    /// Changes the tempo and the signature from the bar closest to `at`. At the origin it changes
    /// the timeline's own tempo; on a bar that has a change it replaces that change.
    SetTempoAt {
        at: u64,
        bpm: f64,
        beats_per_bar: u8,
        beat_unit: u8,
    },
    /// Moves a change to the bar closest to `at`, which is not the origin.
    MoveTempoChange {
        id: String,
        at: u64,
    },
    RemoveTempoChange {
        id: String,
    },
}

/// `name` without the spaces around it, cut to the longest a name can be. A blank name is refused.
fn tidy(name: &str) -> Result<String> {
    let name: String = name.trim().chars().take(MAX_MARK_NAME).collect();
    if name.is_empty() {
        return Err(ProjectError::Invalid("a name needs some text".into()));
    }
    Ok(name)
}

fn numbered(label: &str, count: usize) -> Result<String> {
    tidy(&format!("{} {}", label.trim(), count + 1))
}

fn unknown(what: &str, id: &str) -> ProjectError {
    ProjectError::Invalid(format!("there is no {what} {id}"))
}

fn full(len: usize) -> Result<()> {
    if len >= MAX_MARKS {
        return Err(ProjectError::Invalid(format!(
            "a timeline holds at most {MAX_MARKS} of these"
        )));
    }
    Ok(())
}

fn marker<'a>(timeline: &'a mut Timeline, id: &str) -> Result<&'a mut Marker> {
    timeline
        .markers
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| unknown("marker", id))
}

fn region<'a>(timeline: &'a mut Timeline, id: &str) -> Result<&'a mut Region> {
    timeline
        .regions
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| unknown("region", id))
}

/// The frame of the bar line closest to `at`, in the map of `timeline` as it is.
fn bar_near(timeline: &Timeline, at: u64) -> u64 {
    let (num, den) = timeline.timecode.frame_rate.fraction();
    let (num, den) = (num as f64, den as f64);
    let seconds = TempoMap::of(timeline).nearest_line(at as f64 * den / num, true);
    (seconds * num / den).round().max(0.0) as u64
}

fn sort_marks(timeline: &mut Timeline) {
    timeline.markers.sort_by_key(|m| m.at);
    timeline.regions.sort_by_key(|r| (r.start, r.end));
    timeline.tempo_changes.sort_by_key(|c| c.at);
}

fn timeline_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("there is no timeline {id}")))
}

/// Applies `op` to the timeline `active` of `project`.
pub(crate) fn apply(project: &mut Project, active: &str, op: MarksOp) -> Result<()> {
    let timeline = timeline_mut(project, active)?;
    match op {
        MarksOp::AddMarker { at, label } => {
            full(timeline.markers.len())?;
            let name = numbered(&label, timeline.markers.len())?;
            timeline.markers.push(Marker {
                id: Uuid::new_v4().to_string(),
                at,
                name,
                color: None,
            });
        }
        MarksOp::MoveMarker { id, at } => marker(timeline, &id)?.at = at,
        MarksOp::RenameMarker { id, name } => marker(timeline, &id)?.name = tidy(&name)?,
        MarksOp::SetMarkerColor { id, color } => marker(timeline, &id)?.color = color,
        MarksOp::RemoveMarker { id } => {
            let before = timeline.markers.len();
            timeline.markers.retain(|m| m.id != id);
            if timeline.markers.len() == before {
                return Err(unknown("marker", &id));
            }
        }
        MarksOp::AddRegion { start, end, label } => {
            full(timeline.regions.len())?;
            let name = numbered(&label, timeline.regions.len())?;
            timeline.regions.push(Region {
                id: Uuid::new_v4().to_string(),
                start: start.min(end),
                end: start.max(end),
                name,
                color: None,
            });
        }
        MarksOp::SetRegion { id, start, end } => {
            let region = region(timeline, &id)?;
            (region.start, region.end) = (start.min(end), start.max(end));
        }
        MarksOp::RenameRegion { id, name } => region(timeline, &id)?.name = tidy(&name)?,
        MarksOp::SetRegionColor { id, color } => region(timeline, &id)?.color = color,
        MarksOp::RemoveRegion { id } => {
            let before = timeline.regions.len();
            timeline.regions.retain(|r| r.id != id);
            if timeline.regions.len() == before {
                return Err(unknown("region", &id));
            }
        }
        MarksOp::SetTempoAt {
            at,
            bpm,
            beats_per_bar,
            beat_unit,
        } => {
            let tempo = Tempo {
                bpm,
                beats_per_bar,
                beat_unit,
                click: timeline.tempo.click,
            };
            tempo.validate()?;
            let bar = bar_near(timeline, at);
            if bar == 0 {
                timeline.tempo = tempo;
            } else if let Some(change) = timeline.tempo_changes.iter_mut().find(|c| c.at == bar) {
                (change.bpm, change.beats_per_bar, change.beat_unit) =
                    (bpm, beats_per_bar, beat_unit);
            } else {
                full(timeline.tempo_changes.len())?;
                timeline.tempo_changes.push(TempoChange {
                    id: Uuid::new_v4().to_string(),
                    at: bar,
                    bpm,
                    beats_per_bar,
                    beat_unit,
                });
            }
        }
        MarksOp::MoveTempoChange { id, at } => {
            let index = timeline
                .tempo_changes
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| unknown("tempo change", &id))?;
            let moved = timeline.tempo_changes.remove(index);
            let bar = bar_near(timeline, at);
            if bar == 0 || timeline.tempo_changes.iter().any(|c| c.at == bar) {
                return Err(ProjectError::Invalid(
                    "a tempo change goes on a bar of its own, after the origin".into(),
                ));
            }
            timeline
                .tempo_changes
                .push(TempoChange { at: bar, ..moved });
        }
        MarksOp::RemoveTempoChange { id } => {
            let before = timeline.tempo_changes.len();
            timeline.tempo_changes.retain(|c| c.id != id);
            if timeline.tempo_changes.len() == before {
                return Err(unknown("tempo change", &id));
            }
        }
    }
    sort_marks(timeline);
    Ok(())
}

/// Moves every marker, region edge and tempo change to `to(at)`, as ripple on all tracks does. A
/// region that collapses is dropped, and a tempo change that lands on the origin or on the one before
/// it is dropped, so the timeline stays valid.
pub(crate) fn ripple(timeline: &mut Timeline, to: impl Fn(u64) -> u64) {
    for marker in &mut timeline.markers {
        marker.at = to(marker.at);
    }
    for region in &mut timeline.regions {
        (region.start, region.end) = (to(region.start), to(region.end));
    }
    timeline.regions.retain(|r| r.end > r.start);
    let mut last = 0;
    timeline.tempo_changes.retain_mut(|change| {
        change.at = to(change.at);
        let keep = change.at > last;
        if keep {
            last = change.at;
        }
        keep
    });
    sort_marks(timeline);
}

/// The frame of the marker before (`forward` false) or after the frame `from`, or `None` when there
/// is none that way.
pub fn marker_step(timeline: &Timeline, from: u64, forward: bool) -> Option<u64> {
    let at = timeline.markers.iter().map(|m| m.at);
    if forward {
        at.filter(|&at| at > from).min()
    } else {
        at.filter(|&at| at < from).max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::sample_project;

    /// The sample's first timeline: 3/4 at 128 BPM, 29.97 drop, a change to 6/8 at 90 BPM at
    /// frame 600, a marker at 300 and a region from 300 to 900.
    fn sample() -> (Project, String) {
        let project = sample_project();
        let id = project.timelines[0].id.clone();
        (project, id)
    }

    /// Applies `op` and validates, leaving the project as it was when either fails.
    fn run(project: &mut Project, active: &str, op: MarksOp) -> Result<()> {
        let mut next = project.clone();
        apply(&mut next, active, op)?;
        next.validate()?;
        *project = next;
        Ok(())
    }

    fn add_marker(project: &mut Project, tl: &str, at: u64) {
        let op = MarksOp::AddMarker {
            at,
            label: "Marker".into(),
        };
        run(project, tl, op).unwrap();
    }

    #[test]
    fn a_marker_should_be_added_moved_renamed_coloured_and_removed() {
        let (mut project, tl) = sample();
        add_marker(&mut project, &tl, 100);
        let markers = &project.timelines[0].markers;
        assert_eq!(markers.len(), 2);
        assert_eq!((markers[0].at, markers[0].name.as_str()), (100, "Marker 2"));
        let id = markers[0].id.clone();
        let op = MarksOp::MoveMarker {
            id: id.clone(),
            at: 700,
        };
        run(&mut project, &tl, op).unwrap();
        assert_eq!(
            project.timelines[0].markers[1].id, id,
            "kept in order of position"
        );
        let rename = |name: &str| MarksOp::RenameMarker {
            id: id.clone(),
            name: name.into(),
        };
        run(&mut project, &tl, rename("  Chorus  ")).unwrap();
        assert_eq!(project.timelines[0].markers[1].name, "Chorus");
        assert!(run(&mut project, &tl, rename("  ")).is_err());
        let colour = |color: &str| MarksOp::SetMarkerColor {
            id: id.clone(),
            color: Some(color.into()),
        };
        run(&mut project, &tl, colour("#00ff00")).unwrap();
        assert!(run(&mut project, &tl, colour("green")).is_err());
        let remove = MarksOp::RemoveMarker { id };
        run(&mut project, &tl, remove.clone()).unwrap();
        assert!(run(&mut project, &tl, remove).is_err());
        assert_eq!(project.timelines[0].markers.len(), 1);
    }

    #[test]
    fn a_region_should_keep_its_ends_in_order() {
        let (mut project, tl) = sample();
        let add = MarksOp::AddRegion {
            start: 2000,
            end: 1000,
            label: "Section".into(),
        };
        run(&mut project, &tl, add).unwrap();
        let region = project.timelines[0].regions[1].clone();
        assert_eq!((region.start, region.end), (1000, 2000));
        let set = |start, end| MarksOp::SetRegion {
            id: region.id.clone(),
            start,
            end,
        };
        run(&mut project, &tl, set(50, 80)).unwrap();
        assert_eq!(project.timelines[0].regions[0].id, region.id);
        // An empty region is refused by the validation.
        assert!(run(&mut project, &tl, set(80, 80)).is_err());
        assert_eq!(project.timelines[0].regions[0].end, 80);
    }

    fn tempo_at(at: u64, bpm: f64, beats_per_bar: u8, beat_unit: u8) -> MarksOp {
        MarksOp::SetTempoAt {
            at,
            bpm,
            beats_per_bar,
            beat_unit,
        }
    }

    #[test]
    fn a_tempo_change_should_land_on_the_closest_bar() {
        let (mut project, tl) = sample();
        // 3/4 at 128 BPM is a bar of 1.40625 s: 42.1 frames at 29.97, so 100 is closest to the
        // third bar line (84.2 frames).
        run(&mut project, &tl, tempo_at(100, 100.0, 4, 4)).unwrap();
        let changes = &project.timelines[0].tempo_changes;
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].at, 84);
        assert_eq!((changes[0].bpm, changes[0].beats_per_bar), (100.0, 4));
        // Again at about the same place replaces the change, it does not stack another.
        run(&mut project, &tl, tempo_at(85, 90.0, 4, 4)).unwrap();
        assert_eq!(project.timelines[0].tempo_changes.len(), 2);
        assert_eq!(project.timelines[0].tempo_changes[0].bpm, 90.0);
    }

    #[test]
    fn a_tempo_change_at_the_origin_should_edit_the_first_point() {
        let (mut project, tl) = sample();
        run(&mut project, &tl, tempo_at(3, 99.0, 5, 8)).unwrap();
        let tempo = project.timelines[0].tempo;
        assert_eq!(
            (tempo.bpm, tempo.beats_per_bar, tempo.beat_unit),
            (99.0, 5, 8)
        );
        assert!(tempo.click, "the click switch is not part of the map");
        assert_eq!(project.timelines[0].tempo_changes.len(), 1);
    }

    #[test]
    fn a_tempo_should_be_refused_out_of_range_and_a_change_moved_and_removed() {
        let (mut project, tl) = sample();
        for (bpm, beats, unit) in [(10.0, 4, 4), (120.0, 0, 4), (120.0, 4, 3), (500.0, 4, 4)] {
            assert!(run(&mut project, &tl, tempo_at(100, bpm, beats, unit)).is_err());
        }
        let id = project.timelines[0].tempo_changes[0].id.clone();
        let to = |at| MarksOp::MoveTempoChange { id: id.clone(), at };
        run(&mut project, &tl, to(2000)).unwrap();
        assert!(project.timelines[0].tempo_changes[0].at > 600);
        assert!(run(&mut project, &tl, to(0)).is_err());
        let remove = MarksOp::RemoveTempoChange { id: id.clone() };
        run(&mut project, &tl, remove.clone()).unwrap();
        assert!(project.timelines[0].tempo_changes.is_empty());
        assert!(run(&mut project, &tl, remove).is_err());
    }

    #[test]
    fn the_next_and_previous_marker_should_skip_the_one_the_playhead_is_on() {
        let (mut project, tl) = sample();
        for at in [50, 400, 900] {
            add_marker(&mut project, &tl, at);
        }
        let timeline = &project.timelines[0];
        assert_eq!(marker_step(timeline, 0, true), Some(50));
        assert_eq!(marker_step(timeline, 50, true), Some(300));
        assert_eq!(marker_step(timeline, 400, true), Some(900));
        assert_eq!(marker_step(timeline, 900, true), None);
        assert_eq!(marker_step(timeline, 400, false), Some(300));
        assert_eq!(marker_step(timeline, 50, false), None);
    }

    #[test]
    fn an_old_file_should_read_as_a_single_tempo_map_and_a_new_one_should_round_trip() {
        let mut json: serde_json::Value =
            serde_json::from_str(crate::model::tests::FIXTURE).unwrap();
        let timeline = json["timelines"][0].as_object_mut().unwrap();
        for key in ["tempoChanges", "markers", "regions"] {
            timeline.remove(key);
        }
        let old: Project = serde_json::from_value(json).unwrap();
        old.validate().unwrap();
        let timeline = &old.timelines[0];
        assert!(timeline.tempo_changes.is_empty() && timeline.markers.is_empty());
        let map = TempoMap::of(timeline);
        assert!(map.is_constant());
        assert_eq!(map.segments()[0].bpm, 128.0);
        // A project with none of them writes none of them: older builds see the file unchanged.
        let written = serde_json::to_value(&old).unwrap();
        assert!(written["timelines"][0].get("markers").is_none());
        assert!(written["timelines"][0].get("tempoChanges").is_none());
        assert!(written["timelines"][0]["tempo"].get("beatUnit").is_none());
    }

    #[test]
    fn the_session_should_answer_each_edit_with_the_project_and_an_older_one_should_undo_it() {
        let mut session = crate::session::Session::default();
        session.update(sample_project());
        let before = sample_project();
        let marker = MarksOp::AddMarker {
            at: 42,
            label: "Marker".into(),
        };
        let state = session.marks_edit(marker).unwrap();
        assert_eq!(state.project.timelines[0].markers.len(), 2);
        let state = session.marks_edit(tempo_at(100, 100.0, 4, 4)).unwrap();
        assert_eq!(state.project.timelines[0].tempo_changes.len(), 2);
        // A refused edit leaves the document as it was.
        assert!(session.marks_edit(tempo_at(100, 1.0, 4, 4)).is_err());
        // The window undoes by handing back an older project, and the next edit builds on that.
        session.update(before.clone());
        let state = session
            .marks_edit(MarksOp::RemoveMarker { id: "mk1".into() })
            .unwrap();
        assert!(state.project.timelines[0].markers.is_empty());
        assert_eq!(state.project.timelines[0].tempo_changes.len(), 1);
    }

    #[test]
    fn ripple_should_move_marks_and_drop_those_that_collapse() {
        let (mut project, _) = sample();
        let timeline = &mut project.timelines[0];
        // Close 300..400 up: the marker at 300 sits at the gap's start, the region 300..900 shrinks.
        ripple(timeline, |at| at - at.saturating_sub(300).min(100));
        assert_eq!(timeline.markers[0].at, 300);
        assert_eq!(
            (timeline.regions[0].start, timeline.regions[0].end),
            (300, 800)
        );
        assert_eq!(timeline.tempo_changes[0].at, 500);
        // A region that falls wholly inside the gap goes, and so does a change put on the origin.
        ripple(timeline, |at| at.saturating_sub(5_000));
        assert!(timeline.regions.is_empty() && timeline.tempo_changes.is_empty());
    }
}
