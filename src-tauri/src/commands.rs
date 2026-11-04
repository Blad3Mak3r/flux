//! Tauri commands for creating, opening, importing audio into, saving and deleting `.fxtc` projects.
//!
//! The work is blocking file I/O on possibly large archives, so every command runs it on the
//! blocking thread pool instead of the main thread.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, State};

use crate::audio::SharedAudio;
use crate::project::{AudioAsset, Document, FrameRate, Project, ProjectError, Result, Session};
use crate::recents;

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
    app: AppHandle,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    path: PathBuf,
) -> Result<Project> {
    audio.clear();
    let opened = path.clone();
    let project = run(&session, move |s| s.open(&path)).await?;
    recents::remember(&app, &opened);
    Ok(project)
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
    app: AppHandle,
    session: State<'_, SharedSession>,
    project: Project,
    path: PathBuf,
) -> Result<()> {
    let saved = path.clone();
    run(&session, move |s| s.save(&project, &path)).await?;
    recents::remember(&app, &saved);
    Ok(())
}

/// Permanently deletes a `.fxtc` from disk. The frontend must have the user's confirmation first.
#[tauri::command]
pub async fn project_delete(app: AppHandle, path: PathBuf) -> Result<()> {
    let deleted = path.clone();
    tauri::async_runtime::spawn_blocking(move || delete_show(&path))
        .await
        .map_err(|err| ProjectError::Io(io::Error::other(err)))??;
    recents::remove(&app, &deleted);
    Ok(())
}

/// Only a regular file named `*.fxtc` is removed, so a wrong path can never take anything else.
fn delete_show(path: &Path) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fxtc"))
    {
        return Err(ProjectError::Invalid("not a .fxtc file".into()));
    }
    if !fs::metadata(path)?.is_file() {
        return Err(ProjectError::Invalid("not a file".into()));
    }
    Ok(fs::remove_file(path)?)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn delete_show_should_remove_the_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.FXTC");
        fs::write(&path, b"x").unwrap();

        delete_show(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn delete_show_should_refuse_anything_that_is_not_a_fxtc() {
        let dir = TempDir::new().unwrap();
        let other = dir.path().join("notes.txt");
        fs::write(&other, b"x").unwrap();

        assert!(matches!(delete_show(&other), Err(ProjectError::Invalid(_))));
        assert!(other.exists());
    }

    #[test]
    fn delete_show_should_refuse_a_directory_named_like_a_project() {
        let dir = TempDir::new().unwrap();
        let folder = dir.path().join("show.fxtc");
        fs::create_dir(&folder).unwrap();

        assert!(matches!(
            delete_show(&folder),
            Err(ProjectError::Invalid(_))
        ));
        assert!(folder.is_dir());
    }

    #[test]
    fn delete_show_should_fail_for_a_file_that_is_already_gone() {
        let dir = TempDir::new().unwrap();

        assert!(matches!(
            delete_show(&dir.path().join("gone.fxtc")),
            Err(ProjectError::Io(_))
        ));
    }
}
