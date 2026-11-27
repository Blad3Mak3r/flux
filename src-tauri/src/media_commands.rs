//! Tauri commands for the project's media: which files it links, which are offline, and how to find,
//! relink or gather them. The logic is in `flux_project::media`; these only wire it to the session.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{SharedSession, run};
use crate::video_output::SharedVideoOutput;
use flux_audio::SharedAudio;
use flux_project::media::MediaList;
use flux_project::{Project, ProjectError, ProjectState, Result};

pub const COLLECT_PROGRESS: &str = "media-collect-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectProgress {
    done_bytes: u64,
    total_bytes: u64,
}

/// A project changed by finding media, and how many it found.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Relinked {
    pub state: ProjectState,
    pub found: usize,
    /// Media still offline.
    pub missing: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collected {
    pub state: ProjectState,
    pub copied: usize,
    /// Offline media, which had nothing to copy.
    pub skipped: usize,
}

/// Drops the cached thumbnails of the videos whose link differs between two states of the project,
/// so a different file under the same id does not show the old pictures.
fn forget_thumbnails(app: &AppHandle, before: &Project, after: &Project) {
    let Ok(cache) = crate::video_commands::video_cache(app) else {
        return;
    };
    let dir = cache.join("thumbs");
    for video in &after.videos {
        if before
            .videos
            .iter()
            .any(|old| old.id == video.id && old.link == video.link)
        {
            continue;
        }
        for name in ["jpg", "json", "first.jpg", "first.json"] {
            let _ = std::fs::remove_file(dir.join(format!("{}.{name}", video.id)));
        }
    }
}

/// What was learned of media while it was offline is wrong now: decode, read and draw it again.
fn media_changed(audio: &SharedAudio, output: &SharedVideoOutput) {
    audio.clear();
    crate::video_commands::media_changed();
    output.retry_media();
}

/// The project's media and which of it is offline. It looks at the files, so it is asked again when
/// the project changes and when the window gets the focus back.
#[tauri::command]
pub async fn media_list(project: Project) -> Result<MediaList> {
    tauri::async_runtime::spawn_blocking(move || flux_project::media::list(&project))
        .await
        .map_err(|err| ProjectError::Io(io::Error::other(err)))
}

/// Links the media `id` to the file at `path`, and the other offline media found by name beside it.
#[tauri::command]
pub async fn media_relink(
    app: AppHandle,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedVideoOutput>,
    project: Project,
    id: String,
    path: PathBuf,
) -> Result<Relinked> {
    log::info!("relinking media to {}", path.display());
    let before = project.clone();
    let relinked = run(&session, move |s| {
        s.update(project);
        let (state, found) = s.media_relink(&id, &path)?;
        let missing = flux_project::media::offline_ids(&state.project).len();
        Ok(Relinked {
            state,
            found,
            missing,
        })
    })
    .await?;
    forget_thumbnails(&app, &before, &relinked.state.project);
    media_changed(&audio, &output);
    Ok(relinked)
}

/// Looks for the offline media by file name in `folder` and under it.
#[tauri::command]
pub async fn media_search_folder(
    app: AppHandle,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedVideoOutput>,
    project: Project,
    folder: PathBuf,
) -> Result<Relinked> {
    log::info!("searching {} for media", folder.display());
    let before = project.clone();
    let relinked = run(&session, move |s| {
        s.update(project);
        let (state, found, missing) = s.media_search(&folder)?;
        Ok(Relinked {
            state,
            found,
            missing,
        })
    })
    .await?;
    forget_thumbnails(&app, &before, &relinked.state.project);
    media_changed(&audio, &output);
    Ok(relinked)
}

/// Copies the project's linked media, and takes the embedded media out of the archive, into the
/// folder `<project> Media` beside the project file, and links it there. The copy runs without the
/// session locked, and reports its progress on [`COLLECT_PROGRESS`].
#[tauri::command]
pub async fn media_collect(
    app: AppHandle,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedVideoOutput>,
    project: Project,
) -> Result<Collected> {
    let shared = Arc::clone(&session);
    let before = project.clone();
    let handle = app.clone();
    let collected = tauri::async_runtime::spawn_blocking(move || {
        let plan = {
            let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
            s.update(project);
            s.media_collect_plan()?
        };
        log::info!(
            "collecting {} media files into {}",
            plan.jobs.len(),
            plan.folder.display()
        );
        let total_bytes = plan.total_bytes();
        let mut last = Instant::now();
        let placed = plan.run(|done_bytes| {
            if last.elapsed() >= Duration::from_millis(100) || done_bytes >= total_bytes {
                last = Instant::now();
                let _ = app.emit(
                    COLLECT_PROGRESS,
                    CollectProgress {
                        done_bytes,
                        total_bytes,
                    },
                );
            }
        })?;
        let state = shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .media_collect_apply(&placed)?;
        Ok(Collected {
            state,
            copied: placed.len(),
            skipped: plan.skipped,
        })
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))
    .and_then(|result| result);
    match &collected {
        Ok(done) => forget_thumbnails(&handle, &before, &done.state.project),
        Err(err) => log::warn!("media was not collected: {err}"),
    }
    media_changed(&audio, &output);
    collected
}
