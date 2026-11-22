//! Tauri commands for the videos of the project: what FFmpeg reads from them.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::SharedSession;
use flux_audio::{AudioError, Decoded, SharedAudio};
use flux_project::{Project, ProjectError, ProjectState, Result, VideoAsset, VideoClipOp};

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

/// What FFmpeg read from each video, by asset id: a video's bytes never change under its id.
#[derive(Default)]
pub struct VideoProbes(Mutex<HashMap<String, VideoProbe>>);

pub type SharedProbes = Arc<VideoProbes>;

/// Reads a video of the session with FFmpeg, or answers from what was read before.
fn probe(
    session: &SharedSession,
    probes: &VideoProbes,
    asset: &VideoAsset,
    cache: &std::path::Path,
) -> Result<VideoProbe> {
    if let Some(known) = probes
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&asset.id)
    {
        return Ok(known.clone());
    }
    let path = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .video_path(asset, cache)?;
    let video = flux_video::VideoFile::open(&path)
        .map_err(|err| ProjectError::Invalid(format!("{}: {err}", asset.name)))?;
    let info = video.info();
    let probe = VideoProbe {
        frames: info.frames,
        rate_num: info.rate.num,
        rate_den: info.rate.den,
        width: info.width,
        height: info.height,
        codec: info.codec.clone(),
    };
    probes
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(asset.id.clone(), probe.clone());
    Ok(probe)
}

/// The video cache folder, once the app has said where it is.
static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Where videos kept inside a project archive are copied for FFmpeg to read.
pub fn video_cache(app: &AppHandle) -> Result<PathBuf> {
    if let Some(dir) = CACHE.get() {
        return Ok(dir.clone());
    }
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|err| ProjectError::Io(io::Error::other(err)))?
        .join("video");
    Ok(CACHE.get_or_init(|| dir).clone())
}

/// The sound of a video for the mixer, decoding it the first time; `None` for a video without
/// sound. Kept in the audio cache beside the project's audio, under a key no audio asset has.
pub fn load_audio(
    session: &SharedSession,
    cache: &SharedAudio,
    asset: &VideoAsset,
) -> std::result::Result<Option<Arc<Decoded>>, AudioError> {
    let key = format!("video:{}", asset.id);
    let decoded = match cache.get(&key) {
        Some(decoded) => decoded,
        None => {
            let dir = CACHE
                .get()
                .ok_or_else(|| AudioError::Decode("the video cache is not ready".into()))?;
            let path = session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .video_path(asset, dir)?;
            let decoded = Arc::new(
                match flux_video::audio_wav(&path)
                    .map_err(|err| AudioError::Decode(err.to_string()))?
                {
                    Some(wav) => flux_audio::decode(wav, Some("wav"))?,
                    // Kept empty, so a silent video is read once.
                    None => Decoded {
                        samples: Vec::new(),
                        source_rate: flux_audio::ENGINE_RATE,
                    },
                },
            );
            cache.insert(&key, Arc::clone(&decoded));
            decoded
        }
    };
    Ok((decoded.frames() > 0).then_some(decoded))
}

/// Opens a video of the project with FFmpeg and answers with its length, rate and size.
#[tauri::command]
pub async fn video_probe(
    app: AppHandle,
    session: State<'_, SharedSession>,
    probes: State<'_, SharedProbes>,
    asset: VideoAsset,
) -> Result<VideoProbe> {
    let cache = video_cache(&app)?;
    let (session, probes) = (Arc::clone(&session), Arc::clone(&probes));
    tauri::async_runtime::spawn_blocking(move || probe(&session, &probes, &asset, &cache))
        .await
        .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Moves, trims, splits or removes a video clip of the active timeline at `frame` of it (see
/// [`VideoClipOp`]); the answer is the project with the edit made. The length of the video comes
/// from FFmpeg.
#[tauri::command]
pub async fn video_clip_edit(
    app: AppHandle,
    session: State<'_, SharedSession>,
    probes: State<'_, SharedProbes>,
    project: Project,
    id: String,
    op: VideoClipOp,
    frame: u64,
) -> Result<ProjectState> {
    let cache = video_cache(&app)?;
    let (session, probes) = (Arc::clone(&session), Arc::clone(&probes));
    tauri::async_runtime::spawn_blocking(move || {
        let (asset, rate) = {
            let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
            s.update(project);
            s.video_clip_asset(&id)?
        };
        let info = probe(&session, &probes, &asset, &cache)?;
        let frames = flux_video::timeline_frames(
            info.frames,
            flux_video::Rate::new(info.rate_num, info.rate_den),
            rate,
        );
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .edit_video_clip(&id, op, frame, frames)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// The thumbnails of a video for its clips on the timeline: `count` pictures of `width` x `height`
/// side by side in one JPEG, the `i`th of them showing the video at `i * seconds` seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoThumbs {
    pub width: u32,
    pub height: u32,
    pub count: u32,
    pub seconds: f64,
    /// The strip as a `data:` URL, for an `<img>` or a CSS background.
    #[serde(skip_deserializing)]
    pub data: String,
}

/// Decodes the thumbnails of a video once and keeps them in the cache by the asset's id, which
/// never names other bytes; later calls read them back.
fn thumbs(
    session: &SharedSession,
    asset: &VideoAsset,
    cache: &std::path::Path,
) -> Result<VideoThumbs> {
    let dir = cache.join("thumbs");
    let (jpeg_path, meta_path) = (
        dir.join(format!("{}.jpg", asset.id)),
        dir.join(format!("{}.json", asset.id)),
    );
    let cached = std::fs::read(&jpeg_path).ok().zip(
        std::fs::read(&meta_path)
            .ok()
            .and_then(|meta| serde_json::from_slice::<VideoThumbs>(&meta).ok()),
    );
    let (jpeg, mut meta) = match cached {
        Some(found) => found,
        None => {
            let path = session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .video_path(asset, cache)?;
            let made = flux_video::thumbnails(&path)
                .map_err(|err| ProjectError::Invalid(format!("{}: {err}", asset.name)))?;
            let meta = VideoThumbs {
                width: made.width,
                height: made.height,
                count: made.count,
                seconds: made.seconds,
                data: String::new(),
            };
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&jpeg_path, &made.jpeg)?;
            std::fs::write(
                &meta_path,
                serde_json::to_vec(&meta).map_err(|err| ProjectError::Io(io::Error::other(err)))?,
            )?;
            (made.jpeg, meta)
        }
    };
    meta.data = format!("data:image/jpeg;base64,{}", BASE64.encode(jpeg));
    Ok(meta)
}

/// Answers with the thumbnails of a video of the project (see [`VideoThumbs`]).
#[tauri::command]
pub async fn video_thumbnails(
    app: AppHandle,
    session: State<'_, SharedSession>,
    asset: VideoAsset,
) -> Result<VideoThumbs> {
    let cache = video_cache(&app)?;
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || thumbs(&session, &asset, &cache))
        .await
        .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// What the timeline draws of a video's sound: its waveform, as for an audio file, and its tempo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSound {
    pub duration_seconds: f64,
    pub peaks_per_second: u32,
    /// `[min0, max0, min1, max1, ...]` as signed bytes, full scale at -128 and 127.
    pub peaks: Vec<i8>,
    /// Beats per minute, or `None` when the sound shows no steady pulse.
    pub bpm: Option<f64>,
}

/// Decodes the sound of a video (or takes it from the audio cache, where playback finds it too)
/// and answers with its waveform and tempo; `None` for a video without sound.
#[tauri::command]
pub async fn video_sound(
    app: AppHandle,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    asset: VideoAsset,
) -> std::result::Result<Option<VideoSound>, AudioError> {
    video_cache(&app)?;
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        Ok(
            load_audio(&session, &audio, &asset)?.map(|decoded| VideoSound {
                duration_seconds: decoded.duration_seconds(),
                peaks_per_second: flux_audio::PEAKS_PER_SECOND,
                peaks: flux_audio::peaks(&decoded),
                bpm: flux_audio::detect_bpm(&decoded),
            }),
        )
    })
    .await
    .map_err(|err| AudioError::Worker(err.to_string()))?
}
