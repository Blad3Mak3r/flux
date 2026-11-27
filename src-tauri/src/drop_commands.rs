//! Tauri commands for files dragged in from the file explorer: they say what each file is and
//! place the ones Flux takes (see [`flux_project::media_kind`]), counting the rest.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use serde::Serialize;
use tauri::State;

use crate::commands::{SharedSession, check_video, run};
use crate::live_commands::samples_of;
use flux_audio::{ENGINE_RATE, SharedAudio};
use flux_core::Clock;
use flux_project::{
    Dropped, MediaKind, Project, ProjectError, ProjectState, Result, Staged, media_kind,
};

/// What a drop answers: the project with the files placed and how many were left out.
#[derive(Serialize)]
pub struct DropPlaced {
    #[serde(flatten)]
    state: ProjectState,
    skipped: usize,
}

/// Places audio and video files on the active timeline from frame `at` (from its start), one after
/// another: on the track `track` when it takes them, else on a new track of the right kind.
#[tauri::command]
pub async fn project_drop_timeline(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    paths: Vec<PathBuf>,
    track: Option<String>,
    at: u64,
) -> Result<DropPlaced> {
    log::info!("{} files dropped on the timeline", paths.len());
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        let rate = {
            let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
            s.update(project);
            s.active_timeline_copy()
                .ok_or_else(|| ProjectError::Invalid("no timeline".into()))?
                .timecode
                .frame_rate
        };
        let mut staged = Vec::new();
        for path in &paths {
            let file = match media_kind(path) {
                Some(kind @ (MediaKind::Audio | MediaKind::Video)) => {
                    stage(&session, &audio, path, kind, rate)
                }
                _ => None,
            };
            match file {
                Some(file) => staged.push(file),
                None => log::warn!("dropped file {} left out", path.display()),
            }
        }
        let skipped = paths.len() - staged.len();
        let state = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drop_on_timeline(staged, track.as_deref(), at)?;
        Ok(DropPlaced { state, skipped })
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Registers a file and finds how long it lasts on a timeline at `rate`; `None` when it cannot be read.
fn stage(
    session: &SharedSession,
    audio: &SharedAudio,
    path: &std::path::Path,
    kind: MediaKind,
    rate: flux_project::FrameRate,
) -> Option<Staged> {
    let frames = if kind == MediaKind::Video {
        let info = flux_video::VideoFile::open(path).ok()?.info().clone();
        flux_video::timeline_frames(info.frames, info.rate, rate)
    } else {
        0
    };
    let media = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .stage_drop(path, kind)
        .ok()?;
    let frames = match &media {
        Dropped::Audio(asset) => {
            let samples = samples_of(session, audio, asset)?;
            Clock::new(ENGINE_RATE, rate).samples_to_frames(samples)
        }
        Dropped::Video(_) => frames,
    };
    Some(Staged { media, frames })
}

/// Puts video files and images in the cells of the VJ grid layer `layer` from `column` to the
/// right, each in place of the clip there. What is not media, or does not open, or finds no cell, is
/// counted as left out.
#[tauri::command]
pub async fn project_drop_vj(
    session: State<'_, SharedSession>,
    project: Project,
    paths: Vec<PathBuf>,
    layer: String,
    column: String,
) -> Result<DropPlaced> {
    log::info!("{} files dropped on the grid", paths.len());
    run(&session, move |s| {
        let files: Vec<PathBuf> = paths
            .iter()
            .filter(|path| matches!(media_kind(path), Some(MediaKind::Video | MediaKind::Image)))
            .filter(|path| check_video(path).is_ok())
            .cloned()
            .collect();
        s.update(project);
        let (state, placed) = s.drop_on_grid(&files, &layer, &column)?;
        Ok(DropPlaced {
            state,
            skipped: paths.len() - placed,
        })
    })
    .await
}
