//! Read-only copies of what the document holds.

use super::{ProjectState, Session};
use crate::model::{Project, Timeline};

impl Session {
    /// The composition size set in the project, if any.
    pub fn composition(&self) -> Option<crate::mapping::Composition> {
        self.document.as_ref()?.project.output.composition
    }

    /// The project's first video, which the mapping measures its picture in.
    pub fn first_video(&self) -> Option<crate::VideoAsset> {
        self.document.as_ref()?.project.videos.first().cloned()
    }

    /// The screens of the video output, the main one first; only the main one with no project.
    pub fn video_screens(&self) -> Vec<crate::mapping::VideoScreen> {
        crate::mapping::screens(
            self.document
                .as_ref()
                .map_or(&[], |document| &document.project.output.screens),
        )
    }

    /// Whether the project open is a VJ project.
    pub fn is_vj(&self) -> bool {
        self.document
            .as_ref()
            .is_some_and(|document| document.project.mode == crate::ProjectMode::Vj)
    }

    /// A copy of the VJ grid, `None` unless the project is a VJ project.
    pub fn grid_copy(&self) -> Option<crate::vj::Vj> {
        let project = &self.document.as_ref()?.project;
        (project.mode == crate::ProjectMode::Vj).then(|| project.vj.clone())
    }

    /// The project of a VJ show, where it is held.
    pub fn vj_project(&self) -> Option<&Project> {
        let project = &self.document.as_ref()?.project;
        (project.mode == crate::ProjectMode::Vj).then_some(project)
    }

    /// The project and its active timeline as they are now, copied.
    pub fn project_state(&self) -> Option<ProjectState> {
        let document = self.document.as_ref()?;
        Some(ProjectState {
            project: document.project.clone(),
            active: document.active.clone(),
        })
    }

    /// The VJ grid of a VJ project, where it is held.
    pub fn grid_ref(&self) -> Option<&crate::vj::Vj> {
        let project = &self.document.as_ref()?.project;
        (project.mode == crate::ProjectMode::Vj).then_some(&project.vj)
    }

    /// The tempo of the timeline being played, in beats a minute; 120 with no project.
    pub fn tempo_bpm(&self) -> f64 {
        let Some(document) = self.document.as_ref() else {
            return 120.0;
        };
        document
            .project
            .timeline(&document.active)
            .map_or(120.0, |timeline| timeline.tempo.bpm)
    }

    /// A copy of the project's video `id`.
    pub fn video_copy(&self, id: &str) -> Option<crate::VideoAsset> {
        let document = self.document.as_ref()?;
        document.project.videos.iter().find(|v| v.id == id).cloned()
    }

    /// A copy of the timeline being edited and played.
    pub fn active_timeline_copy(&self) -> Option<Timeline> {
        let document = self.document.as_ref()?;
        document.project.timeline(&document.active).cloned()
    }
}
