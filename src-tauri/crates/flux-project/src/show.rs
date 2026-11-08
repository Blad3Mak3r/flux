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

impl Session {
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
