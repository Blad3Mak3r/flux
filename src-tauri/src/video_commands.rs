//! Tauri commands for the videos of the project: what FFmpeg reads from them.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

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
    /// A still image (PNG, JPEG...): one frame, shown until it is stopped.
    pub still: bool,
}

/// What FFmpeg read from each video, by asset id: a video's bytes never change under its id.
#[derive(Default)]
pub struct VideoProbes(Mutex<HashMap<String, VideoProbe>>);

pub type SharedProbes = Arc<VideoProbes>;

static PROBES: OnceLock<SharedProbes> = OnceLock::new();

/// The one store of what FFmpeg read, shared by the commands and by the cue lists.
pub fn shared_probes() -> SharedProbes {
    Arc::clone(PROBES.get_or_init(SharedProbes::default))
}

/// How many frames of a timeline at `rate` a video of the session lasts; `None` when it cannot be
/// read, or before the app has said where its video cache is.
pub fn timeline_frames_of(
    session: &SharedSession,
    asset: &VideoAsset,
    rate: flux_project::FrameRate,
) -> Option<u64> {
    let info = probe(session, &shared_probes(), asset, CACHE.get()?).ok()?;
    Some(flux_video::timeline_frames(
        info.frames,
        flux_video::Rate::new(info.rate_num, info.rate_den),
        rate,
    ))
}

/// The size of the composition the clips are sized into and the mapping measures its picture in:
/// the one set in the project, else the size of its first video; `None` when neither is known.
pub fn picture_size(session: &SharedSession) -> Option<(u32, u32)> {
    let (composition, first) = {
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        (session.composition(), session.first_video())
    };
    if let Some(composition) = composition {
        return Some((composition.width, composition.height));
    }
    let first = first?;
    let info = probe(session, &shared_probes(), &first, CACHE.get()?).ok()?;
    Some((info.width, info.height))
}

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
        still: info.frames == 1,
    };
    probes
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(asset.id.clone(), probe.clone());
    Ok(probe)
}

/// The videos [`known_probe`] has started reading, so each is read once however often it is asked
/// for, and one FFmpeg cannot read is not tried again on every call.
static ASKED: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();

/// What FFmpeg read of `asset` if it was read already; otherwise `None`, and it is read once, on a
/// thread of its own, so a later call has it. Never blocks on FFmpeg.
pub fn known_probe(session: &SharedSession, asset: &VideoAsset) -> Option<VideoProbe> {
    let probes = shared_probes();
    if let Some(known) = probes
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&asset.id)
    {
        return Some(known.clone());
    }
    let cache = CACHE.get()?.clone();
    let first = ASKED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(asset.id.clone());
    if !first {
        return None;
    }
    let (session, asset) = (Arc::clone(session), asset.clone());
    std::thread::spawn(move || {
        if let Err(err) = probe(&session, &probes, &asset, &cache) {
            log::warn!("could not read video {}: {err}", asset.name);
        }
    });
    None
}

/// Forgets what was learned of the videos while they were offline or elsewhere: what FFmpeg read,
/// and which it gave up on, so each is read again.
pub fn media_changed() {
    shared_probes()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    if let Some(asked) = ASKED.get() {
        asked.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }
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

/// The key under which the sound of the video `id` is kept in the audio cache.
pub fn audio_key(id: &str) -> String {
    format!("video:{id}")
}

/// What FFmpeg reads of `asset`, reading it now if it was not read yet: only for a thread that may
/// wait. `None` when it cannot be read, or before the app has said where its video cache is.
pub fn read_probe(session: &SharedSession, asset: &VideoAsset) -> Option<VideoProbe> {
    probe(session, &shared_probes(), asset, CACHE.get()?).ok()
}

/// What FFmpeg read of the video `id`, if it did; never reads.
pub fn probed(id: &str) -> Option<VideoProbe> {
    shared_probes()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .cloned()
}

/// The sound of a video for the mixer, decoding it the first time; `None` for a video without
/// sound. Kept in the audio cache beside the project's audio, under a key no audio asset has.
pub fn load_audio(
    session: &SharedSession,
    cache: &SharedAudio,
    asset: &VideoAsset,
) -> std::result::Result<Option<Arc<Decoded>>, AudioError> {
    let key = audio_key(&asset.id);
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
    // A VJ clip shows only the first frame, which is much quicker to decode than a strip.
    let vj = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_vj();
    let name = if vj {
        format!("{}.first", asset.id)
    } else {
        asset.id.clone()
    };
    let dir = cache.join("thumbs");
    let (jpeg_path, meta_path) = (
        dir.join(format!("{name}.jpg")),
        dir.join(format!("{name}.json")),
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
            let made = if vj {
                flux_video::poster(&path)
            } else {
                flux_video::thumbnails(&path)
            }
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

/// Adds, moves or removes a slice of the video output (see [`flux_project::mapping`]); the answer
/// is the project with the edit made.
#[tauri::command]
pub async fn video_mapping_edit(
    session: State<'_, SharedSession>,
    project: Project,
    screen: String,
    op: flux_project::mapping::MappingOp,
) -> Result<ProjectState> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.update(project);
        s.edit_mapping(&screen, op)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Adds, moves or removes a mask of a screen of the video output.
#[tauri::command]
pub async fn video_mask_edit(
    session: State<'_, SharedSession>,
    project: Project,
    screen: String,
    op: flux_project::mapping::MaskOp,
) -> Result<ProjectState> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.update(project);
        s.edit_masks(&screen, op)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Sets the size of the composition the clips are sized into; both sides empty go back to the
/// first video's size.
#[tauri::command]
pub async fn video_composition_set(
    session: State<'_, SharedSession>,
    project: Project,
    width: String,
    height: String,
) -> Result<ProjectState> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.update(project);
        s.set_composition(&width, &height)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Adds, removes or renames a screen of the video output; its window follows on its own.
#[tauri::command]
pub async fn video_screen_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: flux_project::mapping::ScreenOp,
) -> Result<ProjectState> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.update(project);
        s.edit_screens(op)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Whether this build of FFmpeg can write HAP, for the inspector to offer the conversion.
#[tauri::command]
pub fn video_hap_available() -> bool {
    flux_video::hap_available()
}

/// How far a conversion has got, sent as [`CONVERT_PROGRESS`] about once a percent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertProgress {
    pub asset: String,
    pub done: u64,
    pub total: u64,
}

pub const CONVERT_PROGRESS: &str = "video-convert-progress";

/// Set while a conversion runs: one at a time, as it already uses the whole CPU.
static CONVERTING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Writes a HAP copy of a video of the project and answers with where it is: beside the linked
/// file (`<name> (HAP).mov`; an error when that folder cannot be written), or in the video cache for
/// a video kept inside the project. The project does not change: [`project_replace_video`] puts the
/// copy in, against the project as it is when the conversion ends.
#[tauri::command]
pub async fn video_convert_hap(
    app: AppHandle,
    session: State<'_, SharedSession>,
    asset: VideoAsset,
) -> Result<PathBuf> {
    if CONVERTING.swap(true, Ordering::SeqCst) {
        return Err(ProjectError::Invalid("a video is being converted".into()));
    }
    CANCEL.store(false, Ordering::SeqCst);
    let cache = video_cache(&app);
    let session = Arc::clone(&session);
    let name = asset.name.clone();
    log::info!("converting video {name} to HAP");
    let result = tauri::async_runtime::spawn_blocking(move || {
        let cache = cache?;
        let src = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_path(&asset, &cache)?;
        let mut last = u64::MAX;
        let mut progress = |done: u64, total: u64| {
            let percent = done * 100 / total.max(1);
            if percent != last {
                last = percent;
                let id = asset.id.clone();
                let _ = app.emit(
                    CONVERT_PROGRESS,
                    ConvertProgress {
                        asset: id,
                        done,
                        total,
                    },
                );
            }
        };
        // A video that is linked gets its copy beside it, which is where it stays linked; one kept
        // inside the project (from an older version) has none, so its copy goes to the cache.
        let dst = if src.starts_with(&cache) {
            let dir = cache.join("converted");
            std::fs::create_dir_all(&dir)?;
            free_name(&dir, &asset.name)
        } else {
            src.parent()
                .map(|dir| free_name(dir, &asset.name))
                .filter(|dst| writable(dst))
                .ok_or_else(|| ProjectError::Invalid("media.error.hapFolder".into()))?
        };
        flux_video::convert_to_hap(&src, &dst, &mut progress, &CANCEL)
            .map_err(|err| ProjectError::Invalid(format!("{}: {err}", asset.name)))?;
        Ok(dst)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))
    .and_then(|result| result);
    CONVERTING.store(false, Ordering::SeqCst);
    match &result {
        Ok(path) => log::info!("converted video {name} to {}", path.display()),
        Err(err) => log::warn!("video {name} was not converted: {err}"),
    }
    result
}

/// `<name> (HAP).mov` in `dir`, or `<name> (HAP 2).mov` and on when that file exists.
fn free_name(dir: &std::path::Path, name: &str) -> PathBuf {
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    (1..)
        .map(|n| match n {
            1 => dir.join(format!("{name} (HAP).mov")),
            n => dir.join(format!("{name} (HAP {n}).mov")),
        })
        .find(|path| !path.exists())
        .unwrap_or_else(|| dir.join("video (HAP).mov"))
}

/// Whether a file can be made at `path`: the folder of an imported video may be read-only.
fn writable(path: &std::path::Path) -> bool {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .is_ok_and(|_| std::fs::remove_file(path).is_ok())
}

/// Stops the conversion running, if any; its command answers with the error.
#[tauri::command]
pub fn video_convert_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Puts the video file at `path` (the HAP copy [`video_convert_hap`] wrote) in place of the video
/// `asset` in every clip that shows it. FFmpeg reads it first.
#[tauri::command]
pub async fn project_replace_video(
    session: State<'_, SharedSession>,
    project: Project,
    asset: String,
    path: PathBuf,
) -> Result<ProjectState> {
    let session = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        flux_video::VideoFile::open(&path)
            .map_err(|err| ProjectError::Invalid(format!("{}: {err}", path.display())))?;
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.update(project);
        // A copy in the video cache is kept in the project; any other is linked.
        let link = CACHE.get().is_none_or(|cache| !path.starts_with(cache));
        s.replace_video(&asset, &path, link)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}
