//! The timelines of the project: add, rename, remove, order and select the active one.

use std::collections::HashSet;

use super::{ProjectState, Session, no_timeline, timeline_mut, unknown_timeline};
use crate::error::{ProjectError, Result};
use crate::model::{Project, TimecodeSettings, Timeline};
use flux_core::FrameRate;

impl Session {
    /// The id of the active timeline, empty before a document was started.
    pub fn active(&self) -> String {
        self.document
            .as_ref()
            .map(|document| document.active.clone())
            .unwrap_or_default()
    }

    /// The timeline of `project` that is active in the document, else its first one. The engine
    /// plays the project the window sends, and this says which of its timelines.
    pub fn active_timeline<'a>(&self, project: &'a Project) -> Result<&'a Timeline> {
        self.document
            .as_ref()
            .and_then(|document| project.timeline(&document.active))
            .or_else(|| project.timelines.first())
            .ok_or_else(no_timeline)
    }

    /// Adds a timeline called `"{label} {n}"` with the frame rate of the active one, and activates
    /// it. The window supplies the translated `label`; the number is the new count.
    pub fn add_timeline(&mut self, label: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            let rate = project
                .timeline(active)
                .or(project.timelines.first())
                .ok_or_else(no_timeline)?
                .timecode
                .frame_rate;
            let timeline = Timeline::new(format!("{label} {}", project.timelines.len() + 1), rate);
            *active = timeline.id.clone();
            project.timelines.push(timeline);
            Ok(())
        })
    }

    pub fn rename_timeline(&mut self, id: &str, name: &str) -> Result<ProjectState> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProjectError::Invalid("a timeline needs a name".into()));
        }
        self.change(|project, _| {
            timeline_mut(project, id)?.name = name.to_owned();
            Ok(())
        })
    }

    /// Removes a timeline and the audio and video no clip uses any more. The one that takes its place becomes
    /// active when it was the active one.
    pub fn remove_timeline(&mut self, id: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            if project
                .setlists
                .iter()
                .flat_map(|s| &s.entries)
                .any(|e| e.timeline_id == id)
            {
                return Err(ProjectError::Invalid("show.error.referenced".into()));
            }
            let index = project
                .timelines
                .iter()
                .position(|timeline| timeline.id == id)
                .ok_or_else(|| unknown_timeline(id))?;
            if project.timelines.len() == 1 {
                return Err(no_timeline());
            }
            project.timelines.remove(index);
            let used: HashSet<String> = project
                .used_assets()
                .into_iter()
                .map(String::from)
                .collect();
            project.assets.retain(|asset| used.contains(&asset.id));
            project.videos.retain(|video| used.contains(&video.id));
            if active == id {
                let neighbour = index.min(project.timelines.len() - 1);
                *active = project.timelines[neighbour].id.clone();
            }
            Ok(())
        })
    }

    /// Puts the timeline at index `to` of the order (clamped to the last). The active one stays.
    pub fn move_timeline(&mut self, id: &str, to: usize) -> Result<ProjectState> {
        self.change(|project, _| {
            let from = project
                .timelines
                .iter()
                .position(|timeline| timeline.id == id)
                .ok_or_else(|| unknown_timeline(id))?;
            let timeline = project.timelines.remove(from);
            project
                .timelines
                .insert(to.min(project.timelines.len()), timeline);
            Ok(())
        })
    }

    /// Makes the timeline after the active one the active one, as when a show ends and the next
    /// is made ready. `None` when the active one is the last: the show stays where it is.
    pub fn next_timeline(&mut self) -> Result<Option<ProjectState>> {
        self.step_timeline(1)
    }

    /// Makes the timeline `delta` places from the active one the active one. `None` when there is
    /// none there (the first has no previous, the last no next).
    pub fn step_timeline(&mut self, delta: isize) -> Result<Option<ProjectState>> {
        let Some(document) = &self.document else {
            return Err(ProjectError::Invalid("no project is open".into()));
        };
        let timelines = &document.project.timelines;
        let target = timelines
            .iter()
            .position(|timeline| timeline.id == document.active)
            .and_then(|at| at.checked_add_signed(delta))
            .and_then(|at| timelines.get(at))
            .map(|timeline| timeline.id.clone());
        target.map(|id| self.select_timeline(&id)).transpose()
    }

    /// Makes the timeline in this place, from 1, the active one. `None` when there is no such place.
    pub fn select_timeline_at(&mut self, place: usize) -> Result<Option<ProjectState>> {
        let Some(document) = &self.document else {
            return Err(ProjectError::Invalid("no project is open".into()));
        };
        let target = place
            .checked_sub(1)
            .and_then(|at| document.project.timelines.get(at))
            .map(|timeline| timeline.id.clone());
        target.map(|id| self.select_timeline(&id)).transpose()
    }

    /// Where the active timeline is among the project's, from 0, and how many there are, `None`
    /// before a document was started. The remote control lights its timeline pads from it.
    pub fn timeline_place(&self) -> Option<(usize, usize)> {
        let document = self.document.as_ref()?;
        let timelines = &document.project.timelines;
        let at = timelines
            .iter()
            .position(|timeline| timeline.id == document.active)?;
        Some((at, timelines.len()))
    }

    /// The frame rate and start of the active timeline, `None` before a document was started.
    pub fn active_timecode(&self) -> Option<TimecodeSettings> {
        let document = self.document.as_ref()?;
        let timeline = document
            .project
            .timeline(&document.active)
            .or(document.project.timelines.first())?;
        Some(timeline.timecode.clone())
    }

    pub fn select_timeline(&mut self, id: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            timeline_mut(project, id)?;
            id.clone_into(active);
            Ok(())
        })
    }

    /// Sets the frame rate and the start of the active timeline.
    pub fn set_timecode(&mut self, frame_rate: FrameRate, start: u64) -> Result<ProjectState> {
        self.change(|project, active| {
            let timecode = &mut timeline_mut(project, active)?.timecode;
            timecode.frame_rate = frame_rate;
            timecode.start = start;
            Ok(())
        })
    }

    /// Makes `id` the active timeline when the project has it, without the checks of an edit.
    pub fn execution_select(&mut self, id: &str) {
        if let Some(document) = &mut self.document
            && document.project.timelines.iter().any(|t| t.id == id)
        {
            document.active = id.into();
        }
    }
}
