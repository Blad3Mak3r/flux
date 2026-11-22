//! Tauri commands for the videos of the project: what FFmpeg reads from them.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::SharedSession;
use flux_project::{ProjectError, Result, VideoAsset};

/// What the timeline needs of a video: its length and rate, and what to say about it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoProbe {
    pub frames: u64,
    /// Frames per second as `num / den`, e.g. 30000/1001.
    pub rate_num: u32,
    pub rate_den: u32,
    pub width: u32,
    pub height: u32,
    pub codec: String,
}

/// Where videos kept inside a project archive are copied for FFmpeg to read.
pub fn video_cache(app: &AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|err| ProjectError::Io(io::Error::other(err)))?
        .join("video"))
}

/// Opens a video of the project with FFmpeg and answers with its length, rate and size.
#[tauri::command]
pub async fn video_probe(
    app: AppHandle,
    session: State<'_, SharedSession>,
    asset: VideoAsset,
) -> Result<VideoProbe> {
    let cache = video_cache(&app)?;
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let path = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_path(&asset, &cache)?;
        let video = flux_video::VideoFile::open(&path)
            .map_err(|err| ProjectError::Invalid(format!("{}: {err}", asset.name)))?;
        let info = video.info();
        Ok(VideoProbe {
            frames: info.frames,
            rate_num: info.rate.num,
            rate_den: info.rate.den,
            width: info.width,
            height: info.height,
            codec: info.codec.clone(),
        })
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}
