//! Starting, opening, saving and recovering the document.

use std::collections::HashSet;
use std::path::Path;

use uuid::Uuid;

use super::{Document, ProjectState, Session, archive_sources};
use crate::archive::{self, AssetSource};
use crate::error::{ProjectError, Result};
use crate::media;
use crate::model::{FORMAT_VERSION, Project};
use flux_core::FrameRate;

impl Session {
    /// Lets the worker skip cloning an unchanged document on each timer tick.
    pub fn recovery_revision(&self) -> (&str, u64) {
        (&self.recovery_id, self.recovery_revision)
    }

    /// Captures metadata and source paths only; never reads audio or preview bytes.
    pub fn recovery(&self) -> Option<crate::recovery::Recovery> {
        self.document.as_ref().map(|document| {
            crate::recovery::Recovery::new(
                self.recovery_id.clone(),
                document.clone(),
                self.sources.clone(),
            )
        })
    }

    pub fn restore_recovery(&mut self, recovery: crate::recovery::Recovery) -> Result<Document> {
        self.ensure_editable()?;
        recovery.validate()?;
        self.recovery_id.clone_from(&recovery.id);
        self.recovery_revision += 1;
        self.sources = recovery.sources;
        self.voice_sources = self
            .sources
            .iter()
            .filter(|(_, s)| matches!(s, AssetSource::Disk(_)))
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect();
        self.previews.clear();
        self.document = Some(recovery.document.clone());
        Ok(recovery.document)
    }

    pub fn set_autosave(&mut self, autosave: crate::model::Autosave) -> Result<ProjectState> {
        autosave.validate()?;
        self.change(|project, _| {
            project.autosave = autosave;
            Ok(())
        })
    }

    /// Forgets the open document and its audio sources without touching any project files.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn new_project(&mut self, name: &str, frame_rate: FrameRate) -> Project {
        self.new_project_of(name, frame_rate, crate::ProjectMode::Timeline)
    }

    /// Starts a new project of the given kind, which it keeps for good; a VJ project starts with
    /// the grid of [`crate::vj::Vj::starting`].
    pub fn new_project_of(
        &mut self,
        name: &str,
        frame_rate: FrameRate,
        mode: crate::ProjectMode,
    ) -> Project {
        let mut project = Project::new(name, frame_rate);
        if mode == crate::ProjectMode::Vj {
            project.mode = mode;
            project.vj = crate::vj::Vj::starting(|| Uuid::new_v4().to_string());
        }
        self.start(project)
    }

    /// Starts a new VJ project with the structure of a template (see [`crate::vj_presets`]),
    /// called `name`.
    pub fn new_project_from_template(
        &mut self,
        template: &crate::vj_presets::Template,
        name: &str,
    ) -> Result<Project> {
        Ok(self.start(template.project(name)?))
    }

    /// Makes `project` the new, unsaved document.
    fn start(&mut self, project: Project) -> Project {
        self.recovery_id = Uuid::new_v4().to_string();
        self.recovery_revision += 1;
        self.sources.clear();
        self.voice_sources.clear();
        self.previews.clear();
        self.document = Some(Document::new(project.clone(), None));
        project
    }

    /// Opens and migrates a project in memory. Only an explicit save upgrades the archive on disk.
    pub fn open(&mut self, path: &Path) -> Result<Project> {
        self.ensure_editable()?;
        let path = std::path::absolute(path)?;
        let (mut project, _archive) = archive::open(&path)?;
        // A file that moved with the project is found again; the rest stays offline, and opening
        // never fails for it.
        media::resolve(&mut project, &path);
        self.recovery_id = Uuid::new_v4().to_string();
        self.recovery_revision += 1;
        self.sources = archive_sources(&project, &path);
        self.voice_sources.clear();
        self.previews.clear();
        self.document = Some(Document::new(project.clone(), Some(path.to_path_buf())));
        Ok(project)
    }

    /// Remembers the latest state of the project being edited.
    pub fn update(&mut self, project: Project) {
        if self.ensure_editable().is_err() {
            return;
        }
        if self
            .document
            .as_ref()
            .is_some_and(|doc| doc.project == project)
        {
            return;
        }
        self.recovery_revision += 1;
        match &mut self.document {
            Some(document) => {
                document.project = project;
                document.settle();
            }
            None => {
                self.recovery_id = Uuid::new_v4().to_string();
                self.document = Some(Document::new(project, None));
            }
        }
    }

    /// A delayed window sync must not resurrect a closed document or replace a different one.
    pub fn sync(&mut self, project: Project) -> Result<String> {
        if self
            .document
            .as_ref()
            .is_none_or(|doc| doc.project.meta.id != project.meta.id)
        {
            return Err(ProjectError::Invalid(
                "the document is no longer open".into(),
            ));
        }
        if self.document.as_ref().is_some_and(|d| d.project != project) {
            self.ensure_editable()?;
        }
        self.update(project);
        Ok(self.active())
    }

    /// Tells a reader whether the document changed: it changes whenever an edit is made.
    pub fn revision(&self) -> (u64, &str) {
        (
            self.recovery_revision,
            self.document.as_ref().map_or("", |d| d.active.as_str()),
        )
    }

    /// Writes `project` to `path` and returns it as written, with the save recorded in its
    /// metadata. Afterwards every asset is read from that file.
    pub fn save(&mut self, project: &Project, path: &Path) -> Result<Project> {
        let path = std::path::absolute(path)?;
        let mut saved = project.clone();
        saved.format_version = FORMAT_VERSION;
        saved.meta.stamp();
        // Removing a video track leaves its file behind; the save drops what no clip shows.
        let used: HashSet<String> = saved.used_assets().into_iter().map(String::from).collect();
        saved.videos.retain(|video| used.contains(&video.id));
        media::relativize(&mut saved, &path);
        archive::save_from(&path, &saved, &self.sources, &self.previews, self.path())?;
        let before = std::mem::replace(&mut self.sources, archive_sources(&saved, &path));
        self.sources.extend(self.voice_sources.clone());
        // What the save left out stays known where it was, so an undo past the save finds it: a
        // file on disk, or a video read from the archive it replaced, still in the video cache.
        for (id, source) in before {
            self.sources.entry(id).or_insert(source);
        }
        // They are in the file now, which is where they are read from from here on.
        self.previews.clear();
        let mut document = Document::new(saved.clone(), Some(path.to_path_buf()));
        if let Some(previous) = &self.document {
            document.select(&previous.active);
        }
        self.document = Some(document);
        self.recovery_revision += 1;
        Ok(saved)
    }
}
