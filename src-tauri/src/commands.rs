//! Tauri commands for creating, opening, importing audio into, saving and deleting `.fxtc` projects.
//!
//! The work is blocking file I/O on possibly large archives, so every command runs it on the
//! blocking thread pool instead of the main thread.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tauri::{AppHandle, Manager, State};

use crate::live_commands::samples_of;
use crate::recents;
use crate::recovery::{self, SharedRecoveries};
use crate::transport_commands::{SharedOutput, is_playing, release};
use flux_audio::{ENGINE_RATE, SharedAudio};
use flux_core::Clock;
use flux_project::{
    AudioAsset, BeatSegment, ClipOp, CueEdited, CueOp, Document, Extent, FrameRate, ItemOp,
    ItemsOp, Lengths, MarksOp, Project, ProjectError, ProjectState, Result, Session, TempoMap,
    marker_step,
};

pub type SharedSession = Arc<Mutex<Session>>;

pub(crate) async fn run<T, F>(session: &SharedSession, op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T> + Send + 'static,
{
    let session = Arc::clone(session);
    let result = tauri::async_runtime::spawn_blocking(move || {
        op(&mut session.lock().unwrap_or_else(PoisonError::into_inner))
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?;
    if let Err(err) = &result {
        log::warn!("project command failed: {err}");
    }
    result
}

/// The document being edited, or `None` before one was started. A window that was reloaded asks for
/// it, because the project lives here as much as in the window.
#[tauri::command]
pub async fn project_document(session: State<'_, SharedSession>) -> Result<Option<Document>> {
    run(&session, |s| Ok(s.document())).await
}

/// Keeps the latest state of the project being edited, for `project_document` to give back.
/// Answers with the active timeline, which an undo in the window can have changed.
#[tauri::command]
pub async fn project_sync(session: State<'_, SharedSession>, project: Project) -> Result<String> {
    run(&session, move |s| s.sync(project)).await
}

#[expect(
    clippy::too_many_arguments,
    reason = "injected state and arguments of a command"
)]
#[tauri::command]
pub async fn project_new(
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedOutput>,
    name: String,
    frame_rate: FrameRate,
    mode: Option<flux_project::ProjectMode>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<ProjectState> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .ensure_editable()?;
    let _voice_transition = voice.begin_transition()?;
    release(&output).await;
    audio.clear();
    let mode = mode.unwrap_or_default();
    log::info!("new {mode:?} project {name:?}");
    recovery::transition(&session, &store, move |s| {
        let project = s.new_project_of(&name, frame_rate, mode);
        Ok(ProjectState {
            project,
            active: s.active(),
        })
    })
    .await
}

#[tauri::command]
pub async fn project_open(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    audio: State<'_, SharedAudio>,
    output: State<'_, SharedOutput>,
    path: PathBuf,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<ProjectState> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .ensure_editable()?;
    let _voice_transition = voice.begin_transition()?;
    release(&output).await;
    audio.clear();
    let opened = path.clone();
    let voice_directory = app
        .path()
        .app_data_dir()
        .map_err(|e| ProjectError::Io(io::Error::other(e)))?
        .join("generated-voice");
    let state = recovery::transition(&session, &store, move |s| {
        let project = s.open(&path)?;
        for generation in project
            .timelines
            .iter()
            .flat_map(|t| &t.tracks)
            .filter_map(|t| match &t.kind {
                flux_project::TrackKind::Operator { orders, .. } => Some(orders),
                _ => None,
            })
            .flatten()
            .filter_map(|o| o.generated.as_ref())
        {
            if let Some(asset) = project.assets.iter().find(|a| a.id == generation.asset) {
                s.retain_voice_asset(asset, &voice_directory)?;
            }
        }
        Ok(ProjectState {
            project,
            active: s.active(),
        })
    })
    .await?;
    log::info!("opened {}", opened.display());
    recents::remember(&app, &opened);
    Ok(state)
}

/// The timelines cannot be changed while the show plays, as the view cannot change either.
fn ensure_stopped(output: &SharedOutput) -> Result<()> {
    if is_playing(output) {
        return Err(ProjectError::Invalid(
            "the show is playing: stop it before changing the timelines".into(),
        ));
    }
    Ok(())
}

/// Runs a change of the active timeline or of its timecode. The show cannot be playing: the output
/// was opened with the old timecode, so it is released and the window opens it again.
async fn retime<T, F>(session: &SharedSession, output: &SharedOutput, op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T> + Send + 'static,
{
    ensure_stopped(output)?;
    let value = run(session, op).await?;
    release(output).await;
    Ok(value)
}

// The timeline commands take the project the window shows and make it the document first, so they
// act on what the user sees even when the window's last edit has not been synced yet.

/// Adds a timeline named `"{label} N"` and makes it the active one.
#[tauri::command]
pub async fn timeline_add(
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    label: String,
) -> Result<ProjectState> {
    retime(&session, &output, move |s| {
        s.update(project);
        s.add_timeline(&label)
    })
    .await
}

#[tauri::command]
pub async fn timeline_rename(
    session: State<'_, SharedSession>,
    project: Project,
    id: String,
    name: String,
) -> Result<ProjectState> {
    run(&session, move |s| {
        s.update(project);
        s.rename_timeline(&id, &name)
    })
    .await
}

#[tauri::command]
pub async fn timeline_remove(
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<ProjectState> {
    retime(&session, &output, move |s| {
        s.update(project);
        s.remove_timeline(&id)
    })
    .await
}

#[tauri::command]
pub async fn timeline_select(
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<ProjectState> {
    retime(&session, &output, move |s| {
        s.update(project);
        s.select_timeline(&id)
    })
    .await
}

/// Moves a timeline to index `to` of the order. It changes neither the active timeline nor any
/// timecode, so the output stays open, but not while the show plays.
#[tauri::command]
pub async fn timeline_move(
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
    to: usize,
) -> Result<ProjectState> {
    ensure_stopped(&output)?;
    run(&session, move |s| {
        s.update(project);
        s.move_timeline(&id, to)
    })
    .await
}

/// Sets the frame rate and the start of the active timeline.
#[tauri::command]
pub async fn timeline_set_timecode(
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    frame_rate: FrameRate,
    start: u64,
) -> Result<ProjectState> {
    retime(&session, &output, move |s| {
        s.update(project);
        s.set_timecode(frame_rate, start)
    })
    .await
}

/// Trims, fades or splits a clip of the active timeline at a frame of it; the answer is the project
/// with the edit made. The length of the clip's asset comes from its decoded audio.
#[tauri::command]
pub async fn clip_edit(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    id: String,
    op: ClipOp,
    frame: u64,
) -> Result<ProjectState> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        let (asset, rate) = {
            let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
            s.update(project);
            s.clip_asset(&id)?
        };
        let samples = samples_of(&session, &audio, &asset).ok_or_else(|| {
            ProjectError::Invalid(format!("the audio of {} cannot be read", asset.name))
        })?;
        let asset_frames = Clock::new(ENGINE_RATE, rate).samples_to_frames(samples);
        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
        s.edit_clip(&id, op, frame, asset_frames)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Edits the tracks, clips and notes of the active timeline, and the project's audio files and name
/// (see [`ItemOp`]); the answer is the project with the edit made and the id of what it created, or
/// the reason it was refused.
#[tauri::command]
pub async fn item_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: ItemOp,
) -> Result<CueEdited> {
    run(&session, move |s| {
        s.update(project);
        s.item_edit(op)
    })
    .await
}

/// How long the media of the clips of the active timeline lasts, in frames of it, for those that
/// play all of their media. What cannot be read counts as lasting nothing.
fn clip_lengths(session: &SharedSession, audio: &SharedAudio) -> Result<Lengths> {
    let (audio_assets, videos, rate) = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .length_assets()?;
    let clock = Clock::new(ENGINE_RATE, rate);
    let mut lengths = Lengths::default();
    for asset in audio_assets {
        if let Some(samples) = samples_of(session, audio, &asset) {
            lengths
                .audio
                .insert(asset.id, clock.samples_to_frames(samples));
        }
    }
    for video in videos {
        if let Some(frames) = crate::video_commands::timeline_frames_of(session, &video, rate) {
            lengths.video.insert(video.id, frames);
        }
    }
    Ok(lengths)
}

/// Moves or removes many items of the active timeline at once, with ripple if asked (see
/// [`ItemsOp`]); the answer is the project with the edit made.
#[tauri::command]
pub async fn items_edit(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    op: ItemsOp,
) -> Result<ProjectState> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .update(project);
        let lengths = if op.needs_lengths() {
            clip_lengths(&session, &audio)?
        } else {
            Lengths::default()
        };
        let result = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .items_edit(op, &lengths);
        if let Err(err) = &result {
            log::warn!("project command failed: {err}");
        }
        result
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Edits the markers, regions and tempo changes of the active timeline (see [`MarksOp`]); the answer
/// is the project with the edit made, or the reason it was refused.
#[tauri::command]
pub async fn marks_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: MarksOp,
) -> Result<CueEdited> {
    run(&session, move |s| {
        s.update(project);
        s.marks_edit(op)
    })
    .await
}

/// The segments of a timeline's tempo map, in frames, for the ruler and the grid to draw.
#[tauri::command]
pub fn tempo_segments(project: Project, timeline: String) -> Vec<BeatSegment> {
    project
        .timeline(&timeline)
        .map_or_else(Vec::new, |timeline| {
            let (num, den) = timeline.timecode.frame_rate.fraction();
            TempoMap::of(timeline).beat_segments(num as f64 / den as f64)
        })
}

/// The frame count that typed timecode text means at `rate`, `None` when it is not a timecode.
#[tauri::command]
pub fn parse_timecode(text: String, rate: flux_core::FrameRate) -> Option<u64> {
    flux_core::Timecode::parse_frames(&text, rate)
}

/// The frame of the marker before or after `from` on a timeline, `None` when there is none that way.
#[tauri::command]
pub fn marker_step_at(project: Project, timeline: String, from: u64, forward: bool) -> Option<u64> {
    marker_step(project.timeline(&timeline)?, from, forward)
}

/// The stretch of the active timeline that the items `ids` cover, or all its content when none are
/// given; `None` when there is nothing. The window turns it into a zoom.
#[tauri::command]
pub async fn items_extent(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    ids: Vec<String>,
) -> Result<Option<Extent>> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .update(project);
        let lengths = clip_lengths(&session, &audio)?;
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .items_extent(&ids, &lengths)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Edits the MIDI and OSC tracks and the OSC destinations (see [`CueOp`]); the answer is the project
/// with the edit made, and the id of what it created.
#[tauri::command]
pub async fn cue_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: CueOp,
) -> Result<CueEdited> {
    run(&session, move |s| {
        s.update(project);
        s.cue_edit(op)
    })
    .await
}

/// Edits the DMX tracks and destinations (see [`DmxOp`]); the answer is the project with the edit
/// made, and the id of what it created.
#[tauri::command]
pub async fn dmx_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: flux_project::DmxOp,
) -> Result<CueEdited> {
    run(&session, move |s| {
        s.update(project);
        s.dmx_edit(op)
    })
    .await
}

#[tauri::command]
pub async fn output_signal_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: flux_project::SignalOp,
) -> Result<ProjectState> {
    run(&session, move |s| {
        s.update(project);
        s.edit_signal(op)
    })
    .await
}

#[tauri::command]
pub async fn audio_routing_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: flux_project::routing::AudioRoutingOp,
) -> std::result::Result<ProjectState, flux_project::routing::RoutingError> {
    run(&session, move |s| {
        s.update(project);
        s.edit_audio_routing(op)
    })
    .await
    .map_err(|error| match error {
        ProjectError::AudioRouting(code) => code,
        _ => flux_project::routing::RoutingError::InvalidProject,
    })
}

/// Registers an audio file; the returned asset is for the frontend to add to its project.
#[tauri::command]
pub async fn project_import_audio(
    session: State<'_, SharedSession>,
    path: PathBuf,
) -> Result<AudioAsset> {
    log::info!("importing audio {}", path.display());
    run(&session, move |s| s.import_audio(&path)).await
}

/// Adds a video file to the active timeline: a new video track with a clip at `at` (frames from the
/// timeline start). FFmpeg reads it first, so a file it cannot decode is refused with the reason.
#[tauri::command]
pub async fn project_import_video(
    session: State<'_, SharedSession>,
    project: Project,
    path: PathBuf,
    at: u64,
) -> Result<ProjectState> {
    log::info!("importing video {}", path.display());
    run(&session, move |s| {
        check_video(&path)?;
        s.update(project);
        s.import_video(&path, at)
    })
    .await
}

/// Adds a video file to the cell of the VJ grid at `layer` and `column`, in place of its clip, or
/// to the layer's first empty cell in the deck shown with no `column`. FFmpeg reads it first, like
/// [`project_import_video`].
#[tauri::command]
pub async fn project_import_vj_video(
    session: State<'_, SharedSession>,
    project: Project,
    path: PathBuf,
    layer: String,
    column: Option<String>,
) -> Result<ProjectState> {
    log::info!("importing video {} into the grid", path.display());
    run(&session, move |s| {
        check_video(&path)?;
        s.update(project);
        s.import_vj_video(&path, &layer, column.as_deref())
    })
    .await
}

/// Adds an image to the project's media without putting it on the grid or the timeline, for the
/// test card's logo. FFmpeg reads it first, like [`project_import_video`].
#[tauri::command]
pub async fn project_import_image(
    session: State<'_, SharedSession>,
    project: Project,
    path: PathBuf,
) -> Result<ProjectState> {
    log::info!("importing image {}", path.display());
    run(&session, move |s| {
        check_video(&path)?;
        s.update(project);
        s.import_media(&path)
    })
    .await
}

/// Opens a video with FFmpeg and logs what it is; a file it cannot decode is refused with the reason.
pub(crate) fn check_video(path: &Path) -> Result<()> {
    let video = flux_video::VideoFile::open(path)
        .map_err(|err| ProjectError::Invalid(format!("{}: {err}", path.display())))?;
    let info = video.info();
    log::info!(
        "video {}x{} {} at {}/{} fps, {} frames",
        info.width,
        info.height,
        info.codec,
        info.rate.num,
        info.rate.den,
        info.frames
    );
    Ok(())
}

/// Edits the VJ grid of a VJ project (see [`flux_project::vj`]); the answer is the project with
/// the edit made.
#[tauri::command]
pub async fn vj_edit(
    app: AppHandle,
    session: State<'_, SharedSession>,
    project: Project,
    op: flux_project::VjOp,
) -> Result<ProjectState> {
    let state = run(&session, move |s| {
        s.update(project);
        s.edit_vj(op)
    })
    .await?;
    // A level, a side, a bypass or a solo changes how loud the playing clips are.
    crate::modulation::wake();
    crate::vj_audio::refresh(&app);
    Ok(state)
}

/// What importing a mappings file answers: the project with them and how many were taken.
#[derive(serde::Serialize)]
pub struct MappingsImported {
    #[serde(flatten)]
    state: ProjectState,
    imported: usize,
    skipped: usize,
    kept: usize,
}

/// Writes the mappings and dashboard links of the VJ grid to a `.flxmap` file.
#[tauri::command]
pub async fn vj_mappings_export(
    session: State<'_, SharedSession>,
    project: Project,
    path: String,
) -> Result<()> {
    run(&session, move |s| {
        s.update(project);
        let text = s.export_vj_mappings()?;
        std::fs::write(&path, text)?;
        log::info!("mappings exported to {path}");
        Ok(())
    })
    .await
}

/// What reading a `.flxmap` file would do to the VJ grid: how much resolves and which mappings it
/// would overwrite. Nothing is changed.
#[tauri::command]
pub async fn vj_mappings_import_preview(
    session: State<'_, SharedSession>,
    project: Project,
    path: String,
) -> Result<flux_project::vj_mapfile::ImportPlan> {
    run(&session, move |s| {
        let file = flux_project::vj_mapfile::parse(&std::fs::read_to_string(&path)?)?;
        s.update(project);
        s.plan_vj_mappings(&file)
    })
    .await
}

/// Reads a `.flxmap` file into the VJ grid as the user decided about what it overwrites; the
/// answer is the project to show (one undo step) and how many mappings were taken, skipped and
/// kept.
#[tauri::command]
pub async fn vj_mappings_import(
    session: State<'_, SharedSession>,
    project: Project,
    path: String,
    choice: flux_project::vj_mapfile::ImportChoice,
) -> Result<MappingsImported> {
    run(&session, move |s| {
        let file = flux_project::vj_mapfile::parse(&std::fs::read_to_string(&path)?)?;
        s.update(project);
        let (state, report) = s.import_vj_mappings(&file, &choice)?;
        log::info!(
            "mappings imported from {path}: {} taken, {} skipped",
            report.imported,
            report.skipped
        );
        Ok(MappingsImported {
            state,
            imported: report.imported,
            skipped: report.skipped,
            kept: report.kept,
        })
    })
    .await
}

/// Moves a level or a clip's transform as a fader or a drag goes, without the project going back
/// and forth; `vj_edit` keeps the value when the hand lets go.
#[tauri::command]
pub fn vj_preview(
    app: AppHandle,
    session: State<'_, SharedSession>,
    op: flux_project::VjOp,
) -> Result<()> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .preview_vj(op)?;
    crate::vj_audio::refresh(&app);
    Ok(())
}

/// What the inspector and the effects browser are drawn from: how many effects a stack holds, the
/// categories effects are listed in, and every kind with its category and each setting's range,
/// unit and default; and what a modulation is made of (shapes, bands, limits and the fields of a
/// layer and a clip that can be modulated); and how each kind of mapping target is driven.
#[tauri::command]
pub fn vj_effect_catalog() -> serde_json::Value {
    serde_json::json!({
        "max": flux_project::vj_effects::MAX_EFFECTS,
        "categories": flux_project::vj_effects::CATEGORIES,
        "effects": flux_project::vj_effects::catalog(),
        "modulation": flux_project::vj_modulation::catalog(),
        "targets": flux_project::vj_mapping::target_kinds(),
    })
}

/// The font families a text block can use, each with the names of its faces: those the machine has,
/// and the bundled one. Reading the system's fonts takes a moment the first time, so it is off the
/// UI thread.
#[tauri::command]
pub async fn vj_fonts() -> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(|| {
        flux_text::families()
            .into_iter()
            .map(|f| serde_json::json!({ "name": f.name, "styles": f.styles }))
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// The cameras and capture cards a Live input clip can show, each as `{ name, label }`: `name` is
/// what the clip stores, `label` what to show. Empty when there are none. Listing asks the system,
/// which can take a moment, so it is off the UI thread.
#[tauri::command]
pub async fn vj_capture_devices() -> Vec<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(|| {
        flux_capture::devices()
            .into_iter()
            .map(|d| serde_json::json!({ "name": d.name, "label": d.label }))
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// What the open capture of `device` says, as `{ state, refused }`: `state` is `idle` (no clip
/// shows it now), `notConnected`, `connected` or `busy` (another program holds it); `refused` is
/// true when it does not offer the format the clip asked and is open as Auto.
#[tauri::command]
pub fn vj_capture_status(device: String) -> serde_json::Value {
    use flux_capture::State;
    match flux_capture::status(&device) {
        None => serde_json::json!({ "state": "idle", "refused": false }),
        Some(status) => {
            let state = match status.state {
                State::NotConnected => "notConnected",
                State::Connected => "connected",
                State::Busy => "busy",
            };
            serde_json::json!({ "state": state, "refused": status.refused })
        }
    }
}

#[tauri::command]
pub async fn project_save(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    project: Project,
    path: PathBuf,
) -> Result<Project> {
    let saved = path.clone();
    let project = recovery::transition(&session, &store, move |s| s.save(&project, &path)).await?;
    log::info!("saved {}", saved.display());
    recents::remember(&app, &saved);
    Ok(project)
}

#[tauri::command]
pub async fn project_set_autosave(
    session: State<'_, SharedSession>,
    project: Project,
    enabled: bool,
    interval_seconds: String,
) -> Result<ProjectState> {
    let interval_seconds = interval_seconds
        .parse()
        .map_err(|_| ProjectError::Invalid("invalid autosave interval".into()))?;
    run(&session, move |s| {
        s.update(project);
        s.set_autosave(flux_project::Autosave {
            enabled,
            interval_seconds,
        })
    })
    .await
}

/// Permanently deletes a project from disk. The frontend must have the user's confirmation first.
#[tauri::command]
pub async fn project_delete(app: AppHandle, path: PathBuf) -> Result<()> {
    let deleted = path.clone();
    tauri::async_runtime::spawn_blocking(move || delete_show(&path))
        .await
        .map_err(|err| ProjectError::Io(io::Error::other(err)))??;
    log::info!("deleted {}", deleted.display());
    recents::remove(&app, &deleted);
    Ok(())
}

/// Only a regular `.flx` or `.fxtc` file is removed, so a wrong path cannot take anything else.
fn delete_show(path: &Path) -> Result<()> {
    if !flux_project::is_project_path(path) {
        return Err(ProjectError::Invalid("not a .flx or .fxtc file".into()));
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
    fn ensure_stopped_should_allow_a_change_when_no_output_runs() {
        assert!(ensure_stopped(&SharedOutput::default()).is_ok());
    }

    #[test]
    fn delete_show_should_remove_the_file() {
        let dir = TempDir::new().unwrap();
        for name in ["show.flx", "show.FLX", "show.fxtc", "show.FXTC"] {
            let path = dir.path().join(name);
            fs::write(&path, b"x").unwrap();

            delete_show(&path).unwrap();

            assert!(!path.exists());
        }
    }

    #[test]
    fn delete_show_should_refuse_anything_that_is_not_a_project() {
        let dir = TempDir::new().unwrap();
        let other = dir.path().join("notes.txt");
        fs::write(&other, b"x").unwrap();

        assert!(matches!(delete_show(&other), Err(ProjectError::Invalid(_))));
        assert!(other.exists());
    }

    #[test]
    fn delete_show_should_refuse_a_directory_named_like_a_project() {
        let dir = TempDir::new().unwrap();
        for name in ["show.flx", "show.fxtc"] {
            let folder = dir.path().join(name);
            fs::create_dir(&folder).unwrap();

            assert!(matches!(
                delete_show(&folder),
                Err(ProjectError::Invalid(_))
            ));
            assert!(folder.is_dir());
        }
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
