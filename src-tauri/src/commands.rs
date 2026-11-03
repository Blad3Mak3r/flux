//! Tauri commands for creating, opening, importing audio into and saving `.fxtc` projects.
//!
//! The work is blocking file I/O on possibly large archives, so every command runs it on the
//! blocking thread pool instead of the main thread.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;

use crate::audio::SharedAudio;
use crate::project::{AudioAsset, Document, FrameRate, Project, ProjectError, Result, Session};

pub type SharedSession = Arc<Mutex<Session>>;

async fn run<T, F>(session: &SharedSession, op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T> + Send + 'static,
{
    let session = Arc::clone(session);
    tauri::async_runtime::spawn_blocking(move || {
        op(&mut session.lock().unwrap_or_else(PoisonError::into_inner))
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// The document being edited, or `None` before one was started. A window that was reloaded asks for
/// it, because the project lives here as much as in the window.
#[tauri::command]
pub async fn project_document(session: State<'_, SharedSession>) -> Result<Option<Document>> {
    run(&session, |s| Ok(s.document())).await
}

/// Keeps the latest state of the project being edited, for `project_document` to give back.
#[tauri::command]
pub async fn project_sync(session: State<'_, SharedSession>, project: Project) -> Result<()> {
    run(&session, move |s| {
        s.update(project);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn project_new(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    name: String,
    frame_rate: FrameRate,
) -> Result<Project> {
    audio.clear();
    run(&session, move |s| Ok(s.new_project(&name, frame_rate))).await
}

#[tauri::command]
pub async fn project_open(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    path: PathBuf,
) -> Result<Project> {
    audio.clear();
    run(&session, move |s| s.open(&path)).await
}

/// Registers an audio file; the returned asset is for the frontend to add to its project.
#[tauri::command]
pub async fn project_import_audio(
    session: State<'_, SharedSession>,
    path: PathBuf,
) -> Result<AudioAsset> {
    run(&session, move |s| s.import_audio(&path)).await
}

#[tauri::command]
pub async fn project_save(
    session: State<'_, SharedSession>,
    project: Project,
    path: PathBuf,
) -> Result<()> {
    run(&session, move |s| s.save(&project, &path)).await
}
