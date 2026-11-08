//! Portable setlists and timeline controls. Execution state never enters the archive.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Project, ProjectError, ProjectState, Result, Session};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Setlist {
    pub id: String,
    pub name: String,
    pub entries: Vec<SetlistEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetlistEntry {
    pub id: String,
    pub timeline_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub omitted: bool,
    #[serde(default)]
    pub transition: Transition,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Transition {
    #[default]
    Wait,
    Continue,
    After {
        milliseconds: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineControl {
    pub id: String,
    pub name: String,
    pub at: u64,
    #[serde(flatten)]
    pub kind: ControlKind,
}

/// Read-only show sheet. Derived here so Edit and the prepared run share the same entry identity.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryOverview {
    pub id: String,
    pub number: usize,
    pub position: usize,
    pub name: String,
    pub source: Option<String>,
    pub omitted: bool,
    pub transition: Transition,
    pub manual_waits: usize,
    pub indefinite_loops: usize,
    pub finite_loops: usize,
    pub move_up: Option<usize>,
    pub move_down: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetlistOverview {
    pub id: String,
    pub name: String,
    pub entries: Vec<EntryOverview>,
}

pub fn overview(project: &Project) -> Vec<SetlistOverview> {
    project
        .setlists
        .iter()
        .map(|setlist| SetlistOverview {
            id: setlist.id.clone(),
            name: setlist.name.clone(),
            entries: setlist
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    let timeline = project.timeline(&entry.timeline_id)?;
                    let mut manual_waits = 0;
                    let mut indefinite_loops = 0;
                    let mut finite_loops = 0;
                    for control in &timeline.controls {
                        match control.kind {
                            ControlKind::Wait => manual_waits += 1,
                            ControlKind::Loop { turns: None, .. } => indefinite_loops += 1,
                            ControlKind::Loop { turns: Some(_), .. } => finite_loops += 1,
                        }
                    }
                    Some(EntryOverview {
                        id: entry.id.clone(),
                        number: index + 1,
                        position: index,
                        name: if entry.name.is_empty() {
                            timeline.name.clone()
                        } else {
                            entry.name.clone()
                        },
                        source: (!entry.name.is_empty() && entry.name != timeline.name)
                            .then(|| timeline.name.clone()),
                        omitted: entry.omitted,
                        transition: entry.transition,
                        manual_waits,
                        indefinite_loops,
                        finite_loops,
                        move_up: index.checked_sub(1),
                        move_down: (index + 1 < setlist.entries.len()).then_some(index + 1),
                    })
                })
                .collect(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryProgress {
    Current,
    Next,
    Done,
    Pending,
    Omitted,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunningEntry {
    #[serde(flatten)]
    pub entry: EntryOverview,
    pub status: EntryProgress,
}

impl SetlistOverview {
    pub fn progress(&self, current_id: &str, completed: bool) -> Vec<RunningEntry> {
        let current = self.entries.iter().position(|entry| entry.id == current_id);
        let next = current.and_then(|index| {
            self.entries
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, entry)| !entry.omitted)
                .map(|(index, _)| index)
        });
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let status = if entry.omitted {
                    EntryProgress::Omitted
                } else if completed || current.is_some_and(|current| index < current) {
                    EntryProgress::Done
                } else if Some(index) == current {
                    EntryProgress::Current
                } else if Some(index) == next {
                    EntryProgress::Next
                } else {
                    EntryProgress::Pending
                };
                RunningEntry {
                    entry: entry.clone(),
                    status,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ControlKind {
    Wait,
    Loop { end: u64, turns: Option<u32> },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ShowEdit {
    AddSetlist {
        name: String,
    },
    RenameSetlist {
        id: String,
        name: String,
    },
    RemoveSetlist {
        id: String,
    },
    AddEntry {
        setlist_id: String,
        timeline_id: String,
    },
    SetEntry {
        setlist_id: String,
        entry: SetlistEntry,
    },
    MoveEntry {
        setlist_id: String,
        id: String,
        to: usize,
    },
    RemoveEntry {
        setlist_id: String,
        id: String,
    },
    PutControl {
        timeline_id: String,
        control: TimelineControl,
    },
    RemoveControl {
        timeline_id: String,
        id: String,
    },
}

fn invalid(message: &str) -> ProjectError {
    ProjectError::Invalid(message.into())
}

pub(crate) fn validate(project: &Project) -> Result<()> {
    let mut ids = HashSet::new();
    let mut unique = |id: &str| {
        if id.is_empty() || !ids.insert(id.to_owned()) {
            Err(invalid("show.error.identity"))
        } else {
            Ok(())
        }
    };
    for setlist in &project.setlists {
        unique(&setlist.id)?;
        check_name(&setlist.name)?;
        for entry in &setlist.entries {
            unique(&entry.id)?;
            if !project.timelines.iter().any(|t| t.id == entry.timeline_id) {
                return Err(invalid("show.error.reference"));
            }
            if entry.name.len() > 1024 || entry.notes.len() > 16384 {
                return Err(invalid("show.error.text"));
            }
            if matches!(entry.transition, Transition::After { milliseconds } if milliseconds > 86_400_000)
            {
                return Err(invalid("show.error.delay"));
            }
        }
    }
    for timeline in &project.timelines {
        if timeline
            .controls
            .windows(2)
            .any(|pair| pair[0].at >= pair[1].at)
        {
            return Err(invalid("show.error.order"));
        }
        let mut controls: Vec<_> = timeline.controls.iter().collect();
        controls.sort_by_key(|c| c.at);
        let mut occupied_until = None;
        for control in controls {
            unique(&control.id)?;
            check_name(&control.name)?;
            let max = u64::from(timeline.timecode.frame_rate.nominal_fps()) * 86400;
            if control.at >= max || matches!(control.kind, ControlKind::Loop {end, ..} if end > max)
            {
                return Err(invalid("show.error.range"));
            }
            if occupied_until.is_some_and(|end| control.at <= end) {
                return Err(invalid("show.error.overlap"));
            }
            occupied_until = Some(match control.kind {
                ControlKind::Wait => control.at,
                ControlKind::Loop { end, turns } => {
                    if end <= control.at || turns == Some(0) {
                        return Err(invalid("show.error.loop"));
                    }
                    end
                }
            });
        }
    }
    Ok(())
}

fn check_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 1024 {
        Err(invalid("show.error.name"))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShowControlKind {
    Wait,
    Loop,
}

impl Session {
    pub fn add_show_control(
        &mut self,
        timeline_id: &str,
        frame: u64,
        kind: ShowControlKind,
        name: String,
    ) -> Result<crate::CueEdited> {
        let id = Uuid::new_v4().to_string();
        let state = self.change(|project, _| {
            let timeline = project
                .timelines
                .iter_mut()
                .find(|t| t.id == timeline_id)
                .ok_or_else(|| invalid("show.error.timeline"))?;
            let at = frame.saturating_sub(timeline.timecode.start);
            let kind = match kind {
                ShowControlKind::Wait => ControlKind::Wait,
                ShowControlKind::Loop => ControlKind::Loop {
                    end: at.saturating_add(u64::from(timeline.timecode.frame_rate.nominal_fps())),
                    turns: None,
                },
            };
            timeline.controls.push(TimelineControl {
                id: id.clone(),
                name,
                at,
                kind,
            });
            timeline.controls.sort_by_key(|c| c.at);
            Ok(())
        })?;
        Ok(crate::CueEdited {
            state,
            id: Some(id),
        })
    }

    pub fn show_edit(&mut self, op: ShowEdit) -> Result<ProjectState> {
        self.change(|project, _| {
            match op {
                ShowEdit::AddSetlist { name } => project.setlists.push(Setlist {
                    id: Uuid::new_v4().to_string(),
                    name,
                    entries: Vec::new(),
                }),
                ShowEdit::RenameSetlist { id, name } => setlist(project, &id)?.name = name,
                ShowEdit::RemoveSetlist { id } => {
                    setlist(project, &id)?;
                    project.setlists.retain(|s| s.id != id);
                }
                ShowEdit::AddEntry {
                    setlist_id,
                    timeline_id,
                } => setlist(project, &setlist_id)?.entries.push(SetlistEntry {
                    id: Uuid::new_v4().to_string(),
                    timeline_id,
                    name: String::new(),
                    notes: String::new(),
                    omitted: false,
                    transition: Transition::Wait,
                }),
                ShowEdit::SetEntry { setlist_id, entry } => {
                    let target = setlist(project, &setlist_id)?
                        .entries
                        .iter_mut()
                        .find(|e| e.id == entry.id)
                        .ok_or_else(|| invalid("show.error.entry"))?;
                    *target = entry;
                }
                ShowEdit::MoveEntry { setlist_id, id, to } => {
                    let entries = &mut setlist(project, &setlist_id)?.entries;
                    let from = entries
                        .iter()
                        .position(|e| e.id == id)
                        .ok_or_else(|| invalid("show.error.entry"))?;
                    let entry = entries.remove(from);
                    entries.insert(to.min(entries.len()), entry);
                }
                ShowEdit::RemoveEntry { setlist_id, id } => {
                    let entries = &mut setlist(project, &setlist_id)?.entries;
                    if !entries.iter().any(|e| e.id == id) {
                        return Err(invalid("show.error.entry"));
                    }
                    entries.retain(|e| e.id != id);
                }
                ShowEdit::PutControl {
                    timeline_id,
                    control,
                } => {
                    let timeline = project
                        .timelines
                        .iter_mut()
                        .find(|t| t.id == timeline_id)
                        .ok_or_else(|| invalid("show.error.timeline"))?;
                    let mut control = control;
                    if control.id.is_empty() {
                        control.id = Uuid::new_v4().to_string();
                    }
                    if let Some(old) = timeline.controls.iter_mut().find(|c| c.id == control.id) {
                        *old = control;
                    } else {
                        timeline.controls.push(control);
                    }
                    timeline.controls.sort_by_key(|c| c.at);
                }
                ShowEdit::RemoveControl { timeline_id, id } => {
                    let timeline = project
                        .timelines
                        .iter_mut()
                        .find(|t| t.id == timeline_id)
                        .ok_or_else(|| invalid("show.error.timeline"))?;
                    if !timeline.controls.iter().any(|c| c.id == id) {
                        return Err(invalid("show.error.control"));
                    }
                    timeline.controls.retain(|c| c.id != id);
                }
            }
            Ok(())
        })
    }
}

fn setlist<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Setlist> {
    project
        .setlists
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| invalid("show.error.setlist"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrameRate;

    #[test]
    fn overview_preserves_repeated_entries_omissions_and_exact_show_behavior() {
        let mut session = Session::default();
        let mut project = session.new_project("Source song", FrameRate::Fps25);
        let timeline = &mut project.timelines[0];
        timeline.controls = vec![
            TimelineControl {
                id: "wait".into(),
                name: "Stand by".into(),
                at: 0,
                kind: ControlKind::Wait,
            },
            TimelineControl {
                id: "vamp".into(),
                name: "Vamp".into(),
                at: 25,
                kind: ControlKind::Loop {
                    end: 50,
                    turns: None,
                },
            },
            TimelineControl {
                id: "repeat".into(),
                name: "Repeat".into(),
                at: 75,
                kind: ControlKind::Loop {
                    end: 100,
                    turns: Some(3),
                },
            },
        ];
        let timeline_id = timeline.id.clone();
        project.setlists = vec![Setlist {
            id: "show".into(),
            name: "Tour".into(),
            entries: (0..4)
                .map(|index| SetlistEntry {
                    id: format!("entry-{index}"),
                    timeline_id: timeline_id.clone(),
                    name: if index == 0 {
                        "Opening".into()
                    } else {
                        String::new()
                    },
                    notes: String::new(),
                    omitted: index == 1,
                    transition: Transition::After { milliseconds: 2500 },
                })
                .collect(),
        }];
        project.validate().unwrap();
        let before = project.clone();
        let sheet = overview(&project).remove(0);
        assert_eq!(sheet.entries.len(), 4);
        assert_eq!(sheet.entries[0].name, "Opening");
        assert_eq!(sheet.entries[0].source.as_deref(), Some("Source song"));
        assert_eq!(sheet.entries[2].name, "Source song");
        assert_eq!(sheet.entries[2].source, None);
        assert_eq!(
            sheet.entries[0].transition,
            Transition::After { milliseconds: 2500 }
        );
        assert_eq!(
            (
                sheet.entries[0].manual_waits,
                sheet.entries[0].indefinite_loops,
                sheet.entries[0].finite_loops
            ),
            (1, 1, 1)
        );
        assert_eq!(sheet.entries[0].move_up, None);
        assert_eq!(sheet.entries[0].move_down, Some(1));
        assert_eq!(sheet.entries[3].move_down, None);
        assert_eq!(sheet.entries[2].number, 3);
        assert_eq!(sheet.entries[2].position, 2);
        let statuses = |id, completed| {
            sheet
                .progress(id, completed)
                .into_iter()
                .map(|e| e.status)
                .collect::<Vec<_>>()
        };
        use EntryProgress::{Current, Done, Next, Omitted, Pending};
        assert_eq!(
            statuses("entry-0", false),
            vec![Current, Omitted, Next, Pending]
        );
        assert_eq!(
            statuses("entry-2", false),
            vec![Done, Omitted, Current, Next]
        );
        assert_eq!(statuses("entry-3", true), vec![Done, Omitted, Done, Done]);
        assert_eq!(project, before);
        session.update(project);
        let moved = session
            .show_edit(ShowEdit::MoveEntry {
                setlist_id: "show".into(),
                id: "entry-0".into(),
                to: 3,
            })
            .unwrap();
        let sheet = overview(&moved.project).remove(0);
        assert_eq!(sheet.entries[3].id, "entry-0");
        assert_eq!(sheet.entries[3].name, "Opening");
        assert_eq!(sheet.entries[3].number, 4);
        assert_eq!(sheet.entries[3].move_down, None);
    }

    #[test]
    fn adding_a_loop_uses_the_playhead_and_the_timeline_frame_rate() {
        let mut session = Session::default();
        let mut project = session.new_project("Song", FrameRate::Fps30);
        project.timelines[0].timecode.start = 108000;
        session.update(project.clone());
        let result = session
            .add_show_control(
                &project.timelines[0].id,
                108120,
                ShowControlKind::Loop,
                "Vamp".into(),
            )
            .unwrap();
        let control = &result.state.project.timelines[0].controls[0];
        assert_eq!(Some(control.id.clone()), result.id);
        assert_eq!(control.at, 120);
        assert_eq!(
            control.kind,
            ControlKind::Loop {
                end: 150,
                turns: None
            }
        );
        let before = session.document().unwrap().project;
        assert!(
            session
                .add_show_control(
                    &project.timelines[0].id,
                    108125,
                    ShowControlKind::Wait,
                    "Wait".into()
                )
                .is_err()
        );
        assert_eq!(session.document().unwrap().project, before);
    }

    #[test]
    fn repeated_entries_share_content_and_invalid_edits_are_atomic() {
        let mut session = Session::default();
        let project = session.new_project("Show", FrameRate::Fps25);
        let state = session
            .show_edit(ShowEdit::AddSetlist {
                name: "Concert".into(),
            })
            .unwrap();
        let id = state.project.setlists[0].id.clone();
        for _ in 0..2 {
            session
                .show_edit(ShowEdit::AddEntry {
                    setlist_id: id.clone(),
                    timeline_id: project.timelines[0].id.clone(),
                })
                .unwrap();
        }
        let before = session.document().unwrap().project;
        assert_eq!(before.setlists[0].entries.len(), 2);
        assert!(
            session
                .show_edit(ShowEdit::AddEntry {
                    setlist_id: id,
                    timeline_id: "missing".into()
                })
                .is_err()
        );
        assert_eq!(session.document().unwrap().project, before);
    }

    #[test]
    fn controls_reject_nested_loops_and_waits_inside_them() {
        let mut p = Project::new("Show", FrameRate::Fps25);
        p.timelines[0].controls = vec![
            TimelineControl {
                id: "a".into(),
                name: "Vamp".into(),
                at: 10,
                kind: ControlKind::Loop {
                    end: 20,
                    turns: None,
                },
            },
            TimelineControl {
                id: "b".into(),
                name: "Wait".into(),
                at: 15,
                kind: ControlKind::Wait,
            },
        ];
        assert!(p.validate().is_err());
        p.timelines[0].controls[1].at = 21;
        assert!(p.validate().is_ok());
    }

    #[test]
    fn show_edits_round_trip_through_an_archive_and_recovery_and_respect_the_execution_lock() {
        let mut session = Session::default();
        let p = session.new_project("Show", FrameRate::Fps25);
        let edited = session
            .show_edit(ShowEdit::AddSetlist {
                name: "Tour".into(),
            })
            .unwrap();
        let id = edited.project.setlists[0].id.clone();
        let edited = session
            .show_edit(ShowEdit::AddEntry {
                setlist_id: id,
                timeline_id: p.timelines[0].id.clone(),
            })
            .unwrap();
        let directory = tempfile::TempDir::new().unwrap();
        let file = directory.path().join("show.flx");
        let saved = session.save(&edited.project, &file).unwrap();
        let mut reopened = Session::default();
        assert_eq!(reopened.open(&file).unwrap(), saved);
        let recovery = session.recovery().unwrap();
        let recovered = Session::default().restore_recovery(recovery).unwrap();
        assert_eq!(recovered.project.setlists, saved.setlists);
        session
            .execution_lock
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(
            session
                .show_edit(ShowEdit::RemoveSetlist {
                    id: saved.setlists[0].id.clone()
                })
                .is_err()
        );
        assert!(session.remove_timeline(&saved.timelines[0].id).is_err());
        assert_eq!(session.document().unwrap().project, saved);
    }
}
