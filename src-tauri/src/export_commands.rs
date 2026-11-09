//! Tauri commands that write the project out as files for other people: today the cue sheet.
//!
//! The content is built by [`flux_engine::cue_sheet`] from the same cues the Live view shows. The
//! cue lengths can need decoding audio, so the work runs on the blocking pool and never touches a
//! running output.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::commands::SharedSession;
use crate::live_commands::{active_timeline, cue_list};
use flux_audio::{AudioError, SharedAudio};
use flux_engine::cue_sheet::{self, SheetLabels, SheetTimeline};
use flux_project::{Project, ProjectError};

/// Which timelines go into the sheet.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SheetScope {
    /// The timeline the backend has active.
    Active,
    /// Every timeline, in the order of the project.
    All,
}

/// A web page for `.html` and `.htm`; anything else is a CSV.
fn is_html(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

/// Writes the cue sheet of `project` to `path`: HTML when the extension says so, CSV otherwise.
/// A clip whose audio cannot be read is a cue without length, never an error.
#[tauri::command]
pub async fn cue_sheet_export(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    scope: SheetScope,
    labels: SheetLabels,
    path: PathBuf,
) -> Result<(), AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    let exported = tauri::async_runtime::spawn_blocking(move || {
        let timelines = match scope {
            SheetScope::Active => vec![active_timeline(&session, &project)?],
            SheetScope::All => project.timelines.iter().collect(),
        };
        let lists: Vec<_> = timelines
            .iter()
            .map(|timeline| cue_list(&project, timeline, &session, &audio))
            .collect();
        let sheet: Vec<SheetTimeline> = timelines
            .iter()
            .zip(&lists)
            .map(|(timeline, cues)| SheetTimeline {
                name: &timeline.name,
                cues,
            })
            .collect();
        let text = if is_html(&path) {
            cue_sheet::html(&project.name, &sheet, &labels)
        } else {
            cue_sheet::csv(&sheet, &labels)
        };
        std::fs::write(&path, text).map_err(ProjectError::from)?;
        Ok::<_, AudioError>(())
    })
    .await
    .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))?;
    match &exported {
        Ok(()) => log::info!("cue sheet exported"),
        Err(err) => log::warn!("cue sheet could not be exported: {err}"),
    }
    exported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_html_should_follow_the_extension_in_any_case() {
        assert!(is_html(Path::new("show.html")));
        assert!(is_html(Path::new("show.HTM")));
        assert!(!is_html(Path::new("show.csv")));
        assert!(!is_html(Path::new("show")));
    }
}
