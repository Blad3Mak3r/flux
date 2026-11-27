//! The `project.json` of a VJ show. A VJ show and a cue show are two apps in one, and their files
//! have shapes of their own: a cue show's holds timelines, tracks, audio and timecode outputs; a VJ
//! show's holds its grid, its videos, the screens they go to and the clock's tempo and timecode,
//! and nothing else.
//!
//! In memory both are a [`Project`], so the editing, undo and output code is shared: a VJ show is
//! a project whose one timeline carries the clock and never holds a track. That timeline is not in
//! the file; it is made again when the file is read.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mapping::{Composition, VideoScreen};
use crate::model::{Autosave, FORMAT_VERSION, Meta, Project, Tempo, TimecodeSettings};
use crate::vj::{ProjectMode, Vj};

/// Id of the timeline that carries the clock of a VJ show in memory.
const CLOCK: &str = "clock";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VjFile {
    format_version: u32,
    mode: ProjectMode,
    name: String,
    meta: Meta,
    #[serde(default)]
    autosave: Autosave,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    videos: Vec<crate::VideoAsset>,
    /// The clock: its tempo and the timecode it counts.
    tempo: Tempo,
    timecode: TimecodeSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    screens: Vec<VideoScreen>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    composition: Option<Composition>,
    #[serde(flatten)]
    pub(crate) grid: Vj,
}

impl VjFile {
    /// The file of `project`, a VJ show that passed validation (so it holds nothing else).
    pub(crate) fn of(project: &Project) -> Self {
        let clock = &project.timelines[0];
        Self {
            format_version: FORMAT_VERSION,
            mode: ProjectMode::Vj,
            name: project.name.clone(),
            meta: project.meta.clone(),
            autosave: project.autosave,
            videos: project.videos.clone(),
            tempo: clock.tempo,
            timecode: clock.timecode.clone(),
            screens: project.output.screens.clone(),
            composition: project.output.composition,
            grid: project.vj.clone(),
        }
    }

    pub(crate) fn into_project(self) -> Project {
        let mut project = Project::new(self.name, self.timecode.frame_rate);
        project.meta = self.meta;
        project.autosave = self.autosave;
        project.videos = self.videos;
        project.output.screens = self.screens;
        project.output.composition = self.composition;
        project.mode = ProjectMode::Vj;
        project.vj = self.grid;
        let clock = &mut project.timelines[0];
        CLOCK.clone_into(&mut clock.id);
        clock.tempo = self.tempo;
        clock.timecode = self.timecode;
        project
    }
}

/// Whether `value` is the file of a VJ show in its own shape. A VJ show saved before it had one
/// has `timelines`, and is read as a project (and cleaned, see `migrate::vj_without_cues`).
pub(crate) fn is_vj_file(value: &Value) -> bool {
    value.get("mode").and_then(Value::as_str) == Some("vj") && value.get("timelines").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Timeline;

    fn clock(project: &Project) -> &Timeline {
        &project.timelines[0]
    }

    fn show() -> Project {
        let mut project = Project::new("Gira", crate::FrameRate::Fps30);
        project.mode = ProjectMode::Vj;
        let mut n = 0;
        project.vj = Vj::starting(|| {
            n += 1;
            format!("id{n}")
        });
        project.timelines[0].tempo.bpm = 128.0;
        project.timelines[0].timecode.start = 90_000;
        project
    }

    #[test]
    fn a_vj_file_should_hold_the_grid_and_the_clock_and_nothing_of_a_cue_show() {
        let value = serde_json::to_value(VjFile::of(&show())).unwrap();
        for cue in ["timelines", "assets", "setlists", "output", "vj", "tracks"] {
            assert!(value.get(cue).is_none(), "{cue} in a VJ file");
        }
        for vj in ["layers", "decks", "deck", "tempo", "timecode", "videos"] {
            if vj != "videos" {
                assert!(value.get(vj).is_some(), "{vj} missing");
            }
        }
        assert!(is_vj_file(&value));
        assert!(!is_vj_file(
            &serde_json::to_value(Project::new("Cue", crate::FrameRate::Fps25)).unwrap()
        ));
    }

    #[test]
    fn a_vj_file_should_read_back_the_show_it_was_written_from() {
        let mut project = show();
        CLOCK.clone_into(&mut project.timelines[0].id);
        let value = serde_json::to_value(VjFile::of(&project)).unwrap();
        let back = serde_json::from_value::<VjFile>(value)
            .unwrap()
            .into_project();
        assert_eq!(clock(&back).tempo.bpm, 128.0);
        assert_eq!(clock(&back).timecode.start, 90_000);
        back.validate().unwrap();
        assert_eq!(back.vj, project.vj);
        assert_eq!(back.timelines[0].tracks.len(), 0);
    }
}
