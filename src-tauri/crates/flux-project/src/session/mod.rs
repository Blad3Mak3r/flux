mod assets;
mod edits;
mod imports;
mod lifecycle;
mod queries;
#[cfg(test)]
mod tests;
mod timelines;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::AssetSource;
use crate::error::{ProjectError, Result};
use crate::model::{AudioClip, Project, Timeline, TrackKind};
use crate::preview::StoredPreview;

/// The document being edited, as the backend last heard of it. The window that edits it can be
/// reloaded, and picks it up from here again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub project: Project,
    /// Where the `.fxtc` lives, or `None` for a project that was never saved.
    pub path: Option<PathBuf>,
    /// The project as it was last opened or saved, or as it started for a new one: the document is
    /// unsaved when `project` differs from it.
    pub baseline: Project,
    /// Id of the timeline being edited and played. Only the backend chooses it.
    pub active: String,
}

impl Document {
    fn new(project: Project, path: Option<PathBuf>) -> Self {
        Self {
            active: first_timeline(&project),
            baseline: project.clone(),
            project,
            path,
        }
    }

    /// Makes `id` the active timeline when the project has it.
    fn select(&mut self, id: &str) {
        if self.project.timeline(id).is_some() {
            id.clone_into(&mut self.active);
        }
    }

    /// An undo in the window can take the active timeline away: the first one takes over.
    fn settle(&mut self) {
        if self.project.timeline(&self.active).is_none() {
            self.active = first_timeline(&self.project);
        }
    }
}

fn first_timeline(project: &Project) -> String {
    project
        .timelines
        .first()
        .map(|timeline| timeline.id.clone())
        .unwrap_or_default()
}

/// A project and the timeline that is active in it: what the window shows after the backend
/// changed something.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectState {
    pub project: Project,
    pub active: String,
}

/// Backend state for the project being edited.
///
/// The frontend edits the `Project` and sends each state of it to the session, which keeps the
/// latest as the [`Document`]. For the audio, the session remembers where the bytes of each asset
/// live: in the `.fxtc` it was opened or saved from, or in a file imported from disk that is not in
/// any archive yet. It also holds the waveforms generated since the project was opened, which the
/// next save writes into the file.
#[derive(Debug, Default)]
pub struct Session {
    pub execution_lock: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The operator's show lock: while set, nothing that could disrupt a running show is accepted.
    /// Session state, never saved with the project and off at start.
    pub show_lock: std::sync::Arc<std::sync::atomic::AtomicBool>,
    recovery_id: String,
    recovery_revision: u64,
    document: Option<Document>,
    sources: HashMap<String, AssetSource>,
    voice_sources: HashMap<String, AssetSource>,
    previews: HashMap<String, StoredPreview>,
}

impl Session {
    /// Fails while a setlist runs or the operator has locked the show: the project cannot change,
    /// open, close or be replaced.
    pub fn ensure_editable(&self) -> Result<()> {
        self.ensure_not_running()?;
        if self.show_lock.load(std::sync::atomic::Ordering::Acquire) {
            return Err(ProjectError::Invalid("show.error.showLocked".into()));
        }
        Ok(())
    }

    /// Fails only while a setlist runs. The show lock does not refuse this one: the operator must
    /// still be able to start the output to play.
    pub fn ensure_not_running(&self) -> Result<()> {
        if self
            .execution_lock
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(ProjectError::Invalid("show.error.locked".into()));
        }
        Ok(())
    }

    /// Applies `edit` to a copy of the project and the active id, and keeps the result only when it
    /// is still a valid project, so a refused change leaves the document as it was.
    pub fn change(
        &mut self,
        edit: impl FnOnce(&mut Project, &mut String) -> Result<()>,
    ) -> Result<ProjectState> {
        self.ensure_editable()?;
        let document = self.document.as_mut().ok_or_else(no_project)?;
        let mut project = document.project.clone();
        let mut active = document.active.clone();
        edit(&mut project, &mut active)?;
        project.validate()?;
        document.project = project.clone();
        document.active.clone_from(&active);
        self.recovery_revision += 1;
        Ok(ProjectState { project, active })
    }

    /// The document being edited, if one was started.
    pub fn document(&self) -> Option<Document> {
        self.document.clone()
    }
}

fn no_project() -> ProjectError {
    ProjectError::Invalid("no project is open".into())
}

fn no_timeline() -> ProjectError {
    ProjectError::Invalid("a project needs a timeline".into())
}

fn unknown_timeline(id: &str) -> ProjectError {
    ProjectError::Invalid(format!("unknown timeline: {id}"))
}

fn audio_clips(timeline: &Timeline) -> impl Iterator<Item = &AudioClip> {
    timeline.tracks.iter().flat_map(|track| match &track.kind {
        TrackKind::Audio { clips } => clips.as_slice(),
        _ => &[],
    })
}

fn unknown_clip(id: &str) -> ProjectError {
    ProjectError::Invalid(format!("there is no clip {id}"))
}

fn timeline_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == id)
        .ok_or_else(|| unknown_timeline(id))
}

fn archive_sources(project: &Project, path: &Path) -> HashMap<String, AssetSource> {
    let audio = project
        .assets
        .iter()
        .filter(|a| a.link.is_none())
        .map(|a| &a.id);
    let video = project
        .videos
        .iter()
        .filter(|v| v.link.is_none())
        .map(|v| &v.id);
    audio
        .chain(video)
        .map(|id| (id.clone(), AssetSource::Archive(path.to_path_buf())))
        .collect()
}
