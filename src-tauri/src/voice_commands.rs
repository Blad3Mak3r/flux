//! Credentials and speech workers. Provider responses and secrets never reach diagnostics.
pub(crate) mod preview;
pub(crate) mod recording;
use crate::voice_jobs::{JobStore, Recording, Snapshot, StoredJob};
use crate::{
    commands::SharedSession,
    preferences,
    transport_commands::{SharedOutput, is_playing},
};
use flux_project::{GeneratedVoice, OperatorOp, Project, ProjectState, TrackKind, VoiceView};
use serde::Serialize;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum Error {
    MissingKey,
    SecureStore,
    Busy,
    Stopped,
    MissingVoice,
    InvalidInput,
    Obsolete,
    Storage,
    Audio,
    Capture,
    Limit,
    MissingRoute,
    Output,
    Provider { error: flux_voice::Error },
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Default)]
pub struct VoiceState {
    key: Mutex<Option<String>>,
    recording: Mutex<Option<recording::ActiveRecording>>,
    session_only: Mutex<bool>,
    pending: Mutex<Option<Pending>>,
    serial: AtomicU64,
    preview: Mutex<Option<flux_engine::output::Output>>,
    control: Mutex<()>,
    preview_epoch: AtomicU64,
    voices: Mutex<std::collections::HashMap<String, String>>,
    transitioning: AtomicBool,
}

struct Pending {
    token: u64,
    unsaved: Option<Arc<Unsaved>>,
    saving: bool,
}
struct Unsaved {
    origin: flux_project::operator::RecordingOrigin,
    extension: String,
    voice_name: Option<String>,
    snapshot: Snapshot,
    session_id: String,
    bytes: Vec<u8>,
    samples: u64,
}

/// A worker unwinding must release its reservation without losing an already received response.
struct Reservation<'a> {
    state: &'a VoiceState,
    token: u64,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut pending = self
            .state
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pending
            .as_ref()
            .is_some_and(|p| p.token == self.token && p.unsaved.is_none())
        {
            *pending = None;
        }
    }
}

impl VoiceState {
    fn reserve(&self) -> Result<u64> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.is_some() || self.transitioning.load(Ordering::Acquire) {
            return Err(Error::Busy);
        }
        let token = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        *pending = Some(Pending {
            token,
            unsaved: None,
            saving: false,
        });
        Ok(token)
    }

    pub fn begin_transition(&self) -> flux_project::Result<VoiceTransition<'_>> {
        let _gate = self.control.lock().unwrap_or_else(PoisonError::into_inner);
        if self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
            || self.transitioning.swap(true, Ordering::AcqRel)
        {
            return Err(flux_project::ProjectError::Invalid(
                "voice work must finish and be saved before closing".into(),
            ));
        }
        invalidate_preview(self);
        Ok(VoiceTransition(self))
    }
}
pub struct VoiceTransition<'a>(&'a VoiceState);
impl Drop for VoiceTransition<'_> {
    fn drop(&mut self) {
        self.0.transitioning.store(false, Ordering::Release);
    }
}

const MAX_VOICE_FRAMES: u64 = flux_core::ENGINE_RATE as u64 * 3600;
fn decode_recording(bytes: Vec<u8>, extension: &str) -> Result<flux_audio::Decoded> {
    flux_audio::decode_limited(bytes, Some(extension), MAX_VOICE_FRAMES).map_err(|error| {
        if matches!(error, flux_audio::AudioError::Limit) {
            Error::Limit
        } else {
            Error::Audio
        }
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    generating: bool,
    unsaved: bool,
}
#[tauri::command]
pub fn voice_activity(state: State<'_, Arc<VoiceState>>) -> Activity {
    let pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
    Activity {
        generating: pending.as_ref().is_some_and(|p| p.unsaved.is_none()),
        unsaved: pending.as_ref().is_some_and(|p| p.unsaved.is_some()),
    }
}

#[tauri::command]
pub async fn voice_cache(audio: State<'_, flux_audio::SharedAudio>, clear: bool) -> Result<String> {
    let audio = Arc::clone(&audio);
    tauri::async_runtime::spawn_blocking(move || {
        if clear {
            audio.clear();
        }
        format!("{:.1}", audio.retained_bytes() as f64 / (1024.0 * 1024.0))
    })
    .await
    .map_err(|_| Error::Storage)
}

#[tauri::command]
pub fn operator_freeze(
    app: AppHandle,
    session: State<'_, SharedSession>,
    project: Project,
) -> Result<ProjectState> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let local = local_voice(&app)?;
    let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
    session.sync(project).map_err(|_| Error::Obsolete)?;
    session
        .change(|project, _| flux_project::operator::freeze_voices(project, local.as_deref()))
        .map_err(|_| Error::MissingVoice)
}

fn snapshot(project: &Project, id: &str, local: Option<&str>) -> Result<Snapshot> {
    for timeline in &project.timelines {
        for track in &timeline.tracks {
            if let TrackKind::Operator { orders, .. } = &track.kind
                && let Some(order) = orders.iter().find(|o| o.id == id)
            {
                let voice = flux_project::operator::effective_voice(project, track, local)
                    .0
                    .ok_or(Error::MissingVoice)?;
                if order.dialogue.trim().is_empty() && local != Some("recorded") {
                    return Err(Error::InvalidInput);
                }
                return Ok(Snapshot {
                    mode: order.mode,
                    synthesis_text: order.synthesis_text.clone(),
                    project: project.meta.id.clone(),
                    timeline: timeline.id.clone(),
                    order: id.into(),
                    dialogue: order.dialogue.clone(),
                    voice: voice.into(),
                    previous_asset: order.generated.as_ref().map(|g| g.asset.clone()),
                });
            }
        }
    }
    Err(Error::Obsolete)
}

fn entry() -> Result<keyring::v1::Entry> {
    keyring::v1::Entry::new("Flux Timecode", "elevenlabs-api-key").map_err(|_| Error::SecureStore)
}
fn key(state: &VoiceState) -> Result<String> {
    if let Some(key) = state
        .key
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        return Ok(key);
    }
    entry()?.get_password().map_err(|error| match error {
        keyring::v1::Error::NoEntry => Error::MissingKey,
        _ => Error::SecureStore,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    configured: bool,
    session_only: bool,
}

#[tauri::command]
pub async fn voice_key_status(state: State<'_, Arc<VoiceState>>) -> Result<KeyStatus> {
    let state = Arc::clone(&state);
    worker(move || {
        Ok(KeyStatus {
            configured: key(&state).is_ok(),
            session_only: *state
                .session_only
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        })
    })
    .await
}

#[tauri::command]
pub async fn voice_key_set(
    state: State<'_, Arc<VoiceState>>,
    api_key: String,
    session_only: bool,
) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        if api_key.trim().is_empty() || api_key.len() > 1024 || api_key.contains(['\r', '\n']) {
            return Err(Error::InvalidInput);
        }
        if !session_only {
            entry()?
                .set_password(api_key.trim())
                .map_err(|_| Error::SecureStore)?;
        }
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = Some(api_key.trim().into());
        *state
            .session_only
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = session_only;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn voice_key_delete(state: State<'_, Arc<VoiceState>>) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => {}
            Err(_) => return Err(Error::SecureStore),
        }
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *state
            .session_only
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = false;
        Ok(())
    })
    .await
}

async fn worker<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| Error::Storage)?
}

#[tauri::command]
pub async fn voice_list(state: State<'_, Arc<VoiceState>>) -> Result<Vec<flux_voice::Voice>> {
    let state = Arc::clone(&state);
    worker(move || {
        let api_key = key(&state)?;
        let voices = flux_voice::SpeechClient::new()
            .and_then(|client| client.voices(&api_key))
            .map_err(|error| Error::Provider { error })?;
        *state.voices.lock().unwrap_or_else(PoisonError::into_inner) = voices
            .iter()
            .map(|v| (v.voice_id.clone(), v.name.clone()))
            .collect();
        Ok(voices)
    })
    .await
}

fn local_voice(app: &AppHandle) -> Result<Option<String>> {
    preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map(|p| p.voice)
        .map_err(|_| Error::Storage)
}

#[tauri::command]
pub fn operator_views(app: AppHandle, project: Project) -> Result<Vec<VoiceView>> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    Ok(flux_project::operator::views(
        &project,
        local_voice(&app)?.as_deref(),
    ))
}

#[tauri::command]
pub fn operator_edit(
    session: State<'_, SharedSession>,
    project: Project,
    op: OperatorOp,
) -> Result<ProjectState> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
    session.sync(project).map_err(|_| Error::Obsolete)?;
    session.operator_edit(op).map_err(|_| Error::InvalidInput)
}

#[tauri::command]
pub async fn operator_preflight(
    app: AppHandle,
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
) -> Result<flux_engine::preflight::Report> {
    if !transport_stopped(&output) {
        return Err(Error::Stopped);
    }
    project.validate().map_err(|_| Error::InvalidInput)?;
    let prefs = preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map_err(|_| Error::Storage)?;

    let session = Arc::clone(&session);
    worker(move || {
        let mut routes = std::collections::HashMap::new();
        for timeline in &project.timelines {
            let resolved = prefs.resolve_project(&project, timeline, false);
            for bus in &resolved.audio_routes {
                let mut config = resolved.clone();
                config.operator = bus.route.clone();
                routes
                    .entry(bus.id.clone())
                    .or_insert_with(|| flux_engine::preflight::probe(&config));
            }
        }
        let mut checked = std::collections::HashMap::new();
        let mut report = flux_engine::preflight::check_routes(
            &project,
            prefs.voice.as_deref(),
            |_, track| {
                flux_project::routing::target(track)
                    .and_then(|id| routes.get(id))
                    .copied()
                    .unwrap_or(flux_engine::preflight::RouteCheck {
                        available: false,
                        ltc_conflict: false,
                    })
            },
            |asset| {
                *checked.entry(asset.id.clone()).or_insert_with(|| {
                    let bytes = session
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .asset_bytes(asset)
                        .ok()?;
                    let extension = std::path::Path::new(&asset.file)
                        .extension()
                        .and_then(|e| e.to_str())?;
                    Some(decode_recording(bytes, extension).ok()?.frames() as u64)
                })
            },
        );
        // Cue tracks and timecode outputs are the same for every timeline; the output config only
        // varies per timeline in its audio routes.
        let output = prefs.resolve(&project.output, false);
        let hardware = flux_engine::preflight::Hardware::scan(output.driver.as_deref());
        report.extend(flux_engine::preflight::check_hardware(
            &project, &output, &hardware,
        ));
        Ok(report)
    })
    .await
}

fn job_store(app: &AppHandle) -> Result<JobStore> {
    Ok(JobStore::new(
        app.path()
            .app_data_dir()
            .map_err(|_| Error::Storage)?
            .join("generated-voice")
            .join("jobs"),
    ))
}

#[tauri::command]
pub async fn voice_import(
    app: AppHandle,
    session: State<'_, SharedSession>,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
    path: std::path::PathBuf,
) -> Result<String> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let snapshot = snapshot(&project, &id, Some("recorded"))?;
    let session_id = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recovery_revision()
        .0
        .to_owned();
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    let token = {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if !transport_stopped(&output) {
            return Err(Error::Stopped);
        }
        if session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .document()
            .is_none_or(|d| d.project != project)
        {
            return Err(Error::Obsolete);
        }
        state.reserve()?
    };
    worker(move || {
        let _reservation = Reservation {
            state: &state,
            token,
        };
        let result = (|| {
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .ok_or(Error::InvalidInput)?
                .to_ascii_lowercase();
            if !["wav", "mp3", "aif", "aiff", "flac", "ogg"].contains(&extension.as_str()) {
                return Err(Error::InvalidInput);
            }
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .map_err(|_| Error::Storage)?
                .take(128 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::Storage)?;
            if bytes.len() > 128 * 1024 * 1024 {
                return Err(Error::Limit);
            }
            let decoded = decode_recording(bytes.clone(), &extension)?;
            Ok(Unsaved {
                snapshot,
                session_id,
                samples: decoded.frames() as u64,
                bytes,
                extension,
                origin: flux_project::operator::RecordingOrigin::Imported,
                voice_name: path.file_name().and_then(|n| n.to_str()).map(str::to_owned),
            })
        })();
        {
            let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
            match result {
                Ok(audio) => {
                    pending
                        .as_mut()
                        .filter(|p| p.token == token)
                        .ok_or(Error::Obsolete)?
                        .unsaved = Some(Arc::new(audio));
                }
                Err(error) => {
                    *pending = None;
                    return Err(error);
                }
            }
        }
        persist_pending(&state, &store)
    })
    .await
}

#[tauri::command]
pub async fn voice_generate(
    app: AppHandle,
    session: State<'_, SharedSession>,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<String> {
    if !transport_stopped(&output) {
        return Err(Error::Stopped);
    }
    project.validate().map_err(|_| Error::InvalidInput)?;
    let snapshot = snapshot(&project, &id, local_voice(&app)?.as_deref())?;
    let session_id = {
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        if session
            .document()
            .is_none_or(|d| d.project.meta.id != snapshot.project || d.active != snapshot.timeline)
        {
            return Err(Error::Obsolete);
        }
        session.recovery_revision().0.to_owned()
    };
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    let token = {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if !transport_stopped(&output) {
            return Err(Error::Stopped);
        }
        if session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recovery_revision()
            .0
            != session_id
        {
            return Err(Error::Obsolete);
        }
        state.reserve()?
    };
    worker(move || {
        let _reservation = Reservation {
            state: &state,
            token,
        };
        let result = (|| {
            let api_key = key(&state)?;
            let bytes = flux_voice::SpeechClient::new()
                .and_then(|c| {
                    c.generate(
                        &api_key,
                        &snapshot.voice,
                        snapshot
                            .synthesis_text
                            .as_deref()
                            .unwrap_or(&snapshot.dialogue),
                    )
                })
                .map_err(|error| Error::Provider { error })?;
            let audio = decode_recording(bytes.clone(), "mp3")?;
            let samples = audio.frames() as u64;
            if samples == 0 || samples > u64::from(flux_core::ENGINE_RATE) * 3600 {
                return Err(Error::Audio);
            }
            Ok(Unsaved {
                voice_name: state
                    .voices
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&snapshot.voice)
                    .cloned(),
                origin: Default::default(),
                extension: "mp3".into(),
                snapshot,
                session_id,
                bytes,
                samples,
            })
        })();
        {
            let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
            match result {
                Ok(audio) => {
                    let current = pending
                        .as_mut()
                        .filter(|p| p.token == token)
                        .ok_or(Error::Obsolete)?;
                    current.unsaved = Some(Arc::new(audio));
                }
                Err(error) => {
                    *pending = None;
                    return Err(error);
                }
            }
        }
        persist_pending(&state, &store)
    })
    .await
}

fn persist_pending(state: &VoiceState, store: &JobStore) -> Result<String> {
    let (token, audio) = {
        let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
        let pending = pending.as_mut().ok_or(Error::Busy)?;
        if pending.saving {
            return Err(Error::Busy);
        }
        let audio = Arc::clone(pending.unsaved.as_ref().ok_or(Error::Busy)?);
        pending.saving = true;
        (pending.token, audio)
    };
    let result = store
        .save_recording(
            audio.snapshot.clone(),
            audio.session_id.clone(),
            Recording {
                bytes: &audio.bytes,
                samples: audio.samples,
                extension: &audio.extension,
                origin: audio.origin,
                voice_name: audio.voice_name.clone(),
            },
        )
        .map_err(|_| Error::Storage);
    let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
    let current = pending
        .as_mut()
        .filter(|p| p.token == token)
        .ok_or(Error::Obsolete)?;
    current.saving = false;
    match result {
        Ok(job) => {
            *pending = None;
            Ok(job.id)
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub fn voice_pending(state: State<'_, Arc<VoiceState>>) -> bool {
    state
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|p| p.unsaved.as_ref())
        .is_some()
}

#[tauri::command]
pub async fn voice_retry_save(app: AppHandle, state: State<'_, Arc<VoiceState>>) -> Result<String> {
    let state = Arc::clone(&state);
    let store = job_store(&app)?;
    worker(move || persist_pending(&state, &store)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverableJob {
    id: String,
    order_id: String,
    dialogue: String,
    voice: String,
    can_attach: bool,
    matching: bool,
    created_millis: u64,
    duration: String,
    voice_name: Option<String>,
    origin: flux_project::operator::RecordingOrigin,
}

#[tauri::command]
pub async fn voice_jobs(app: AppHandle, project: Project) -> Result<Vec<RecoverableJob>> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let local = local_voice(&app)?;
    let store = job_store(&app)?;
    worker(move || {
        Ok(store
            .list(&project.meta.id)
            .map_err(|_| Error::Storage)?
            .into_iter()
            .filter_map(|job| {
                let current = project
                    .timelines
                    .iter()
                    .flat_map(|t| &t.tracks)
                    .filter_map(|t| match &t.kind {
                        TrackKind::Operator { orders, .. } => Some(orders),
                        _ => None,
                    })
                    .flatten()
                    .find(|o| o.id == job.snapshot.order);
                if current.is_some_and(|o| o.generated.as_ref().is_some_and(|g| g.asset == job.id))
                {
                    return None;
                }
                let matching = job_matches(&project, &job, local.as_deref());
                Some(RecoverableJob {
                    created_millis: job.created_millis,
                    duration: format!(
                        "{:.2}",
                        job.samples as f64 / f64::from(flux_core::ENGINE_RATE)
                    ),
                    voice_name: job.voice_name,
                    origin: job.origin,
                    id: job.id,
                    order_id: job.snapshot.order,
                    dialogue: job.snapshot.dialogue,
                    voice: job.snapshot.voice,
                    can_attach: current.is_some(),
                    matching,
                })
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn voice_commit(
    app: AppHandle,
    state: State<'_, Arc<VoiceState>>,
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    project: Project,
    job_id: String,
    recover: bool,
) -> Result<ProjectState> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    let session = Arc::clone(&session);
    let output = Arc::clone(&output);
    worker(move || {
        let job = store.read(&job_id).map_err(|_| Error::Storage)?;
        let audio = store.audio(&job_id).map_err(|_| Error::Storage)?;
        let decoded = decode_recording(
            std::fs::read(&audio).map_err(|_| Error::Storage)?,
            &job.extension,
        )?;
        if decoded.frames() as u64 != job.samples {
            return Err(Error::Audio);
        }
        drop(decoded);
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if !transport_stopped(&output) {
            return Err(Error::Stopped);
        }
        let local = local_voice(&app)?;
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        attach_recording(
            &mut session,
            project,
            job,
            &audio,
            local.as_deref(),
            recover,
        )
    })
    .await
}

fn job_matches(project: &Project, job: &StoredJob, local: Option<&str>) -> bool {
    let imported = job.origin != flux_project::operator::RecordingOrigin::Generated;
    snapshot(
        project,
        &job.snapshot.order,
        if imported { Some("recorded") } else { local },
    )
    .is_ok_and(|mut current| {
        if imported {
            current.voice.clone_from(&job.snapshot.voice);
            current
                .synthesis_text
                .clone_from(&job.snapshot.synthesis_text);
        }
        current == job.snapshot
    })
}

fn attach_recording(
    session: &mut flux_project::Session,
    project: Project,
    job: StoredJob,
    audio: &std::path::Path,
    local: Option<&str>,
    recover: bool,
) -> Result<ProjectState> {
    if job.snapshot.project != project.meta.id {
        return Err(Error::Obsolete);
    }
    if !recover && session.recovery_revision().0 != job.session_id {
        return Err(Error::Obsolete);
    }
    if !recover && !job_matches(&project, &job, local) {
        return Err(Error::Obsolete);
    }
    if session.document().is_none_or(|d| d.project != project) {
        return Err(Error::Obsolete);
    }
    let asset = session
        .import_voice_job(audio, &job.id, &job.snapshot.dialogue)
        .map_err(|_| Error::Storage)?;
    session
        .change(|project, _| {
            let order = project
                .timelines
                .iter_mut()
                .find(|t| t.id == job.snapshot.timeline)
                .into_iter()
                .flat_map(|t| &mut t.tracks)
                .filter_map(|t| match &mut t.kind {
                    TrackKind::Operator { orders, .. } => Some(orders),
                    _ => None,
                })
                .flatten()
                .find(|o| o.id == job.snapshot.order)
                .ok_or_else(|| flux_project::ProjectError::Invalid("unknown voice order".into()))?;
            order.mode = if job.origin == flux_project::operator::RecordingOrigin::Generated {
                flux_project::operator::VoiceMode::Generated
            } else {
                flux_project::operator::VoiceMode::Manual
            };
            order.trim_start = 0;
            order.trim_end = 0;
            order.generated = Some(GeneratedVoice {
                origin: job.origin,
                synthesis_text: if job.origin != flux_project::operator::RecordingOrigin::Generated
                {
                    None
                } else {
                    job.snapshot.synthesis_text
                },
                asset: asset.id.clone(),
                dialogue: job.snapshot.dialogue,
                voice: job.snapshot.voice,
                model: if job.origin != flux_project::operator::RecordingOrigin::Generated {
                    String::new()
                } else {
                    flux_project::operator::VOICE_MODEL.into()
                },
                samples: job.samples,
            });
            if !project.assets.iter().any(|a| a.id == asset.id) {
                project.assets.push(asset);
            }
            Ok(())
        })
        .map_err(|_| Error::Obsolete)
}

fn transport_stopped(output: &SharedOutput) -> bool {
    !is_playing(output)
        && crate::transport_commands::snapshot(output).is_none_or(|(position, _)| {
            position.state == flux_engine::transport::TransportState::Stopped
        })
}

fn invalidate_preview(state: &VoiceState) {
    state.preview_epoch.fetch_add(1, Ordering::AcqRel);
    let previous = state
        .preview
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    drop(previous);
}

pub fn stop_preview(state: &VoiceState) {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    invalidate_preview(state);
}

pub fn send_transport(
    state: &VoiceState,
    output: &SharedOutput,
    command: flux_engine::transport::Command,
) -> std::result::Result<(), flux_engine::output::OutputError> {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if matches!(command, flux_engine::transport::Command::Play)
        && state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    {
        return Err(flux_engine::output::OutputError::Recording);
    }
    invalidate_preview(state);
    crate::transport_commands::send(output, command)
}

pub(crate) fn send_show(
    state: &VoiceState,
    output: &SharedOutput,
    command: flux_engine::show::ShowCommand,
) -> std::result::Result<(), flux_engine::output::OutputError> {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if state
        .recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
    {
        return Err(flux_engine::output::OutputError::Recording);
    }
    invalidate_preview(state);
    crate::transport_commands::show_send(output, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_transitions_and_reservations_exclude_each_other() {
        let state = VoiceState::default();
        let epoch = state.preview_epoch.load(Ordering::Acquire);
        let transition = state.begin_transition().unwrap();
        assert!(state.reserve().is_err());
        assert!(state.begin_transition().is_err());
        assert_ne!(state.preview_epoch.load(Ordering::Acquire), epoch);
        drop(transition);
        state.reserve().unwrap();
        assert!(state.begin_transition().is_err());
        *state.pending.lock().unwrap() = None;
        assert!(state.begin_transition().is_ok());
    }

    #[test]
    fn an_unwinding_worker_does_not_leave_the_document_permanently_locked() {
        let state = VoiceState::default();
        let token = state.reserve().unwrap();
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = Reservation {
                state: &state,
                token,
            };
            panic!("simulated decoder failure");
        }));
        assert!(failed.is_err());
        assert!(state.begin_transition().is_ok());
        assert!(state.reserve().is_ok());
    }

    #[test]
    fn imported_recording_round_trips_offline_with_source_format_and_resets_trims() {
        recording_round_trip(flux_project::operator::RecordingOrigin::Imported);
    }

    #[test]
    fn microphone_take_survives_trim_save_reopen_and_offline_playback() {
        recording_round_trip(flux_project::operator::RecordingOrigin::Recorded);
    }

    fn recording_round_trip(origin: flux_project::operator::RecordingOrigin) {
        let dir = tempfile::tempdir().unwrap();
        let mut session = flux_project::Session::default();
        session.new_project("Show", flux_project::FrameRate::Fps25);
        session
            .operator_edit(OperatorOp::AddTrack {
                name: "Operator".into(),
            })
            .unwrap();
        let track = session.document().unwrap().project.timelines[0].tracks[0]
            .id
            .clone();
        let mut p = session
            .operator_edit(OperatorOp::AddOrder {
                track_id: track,
                at: 100,
            })
            .unwrap()
            .project;
        let TrackKind::Operator { orders, .. } = &mut p.timelines[0].tracks[0].kind else {
            panic!()
        };
        orders[0].mode = flux_project::operator::VoiceMode::Manual;
        orders[0].dialogue = "Operator output test".into();
        let id = orders[0].id.clone();
        session.sync(p.clone()).unwrap();
        let bytes = include_bytes!("../assets/operator-test.wav");
        let decoded = decode_recording(bytes.to_vec(), "wav").unwrap();
        assert!(decoded.samples.iter().any(|s| *s != 0));
        let store = JobStore::new(dir.path().join("jobs"));
        let job = store
            .save_recording(
                snapshot(&p, &id, Some("recorded")).unwrap(),
                session.recovery_revision().0.into(),
                Recording {
                    bytes,
                    samples: decoded.frames() as u64,
                    extension: "wav",
                    origin,
                    voice_name: Some("Human recording.wav".into()),
                },
            )
            .unwrap();
        let audio = store.audio(&job.id).unwrap();
        let mut changed = p.clone();
        let TrackKind::Operator { orders, .. } = &mut changed.timelines[0].tracks[0].kind else {
            panic!()
        };
        orders[0].mode = flux_project::operator::VoiceMode::Generated;
        session.sync(changed.clone()).unwrap();
        assert!(matches!(
            attach_recording(&mut session, changed, job.clone(), &audio, None, false),
            Err(Error::Obsolete)
        ));
        assert!(store.read(&job.id).is_ok());
        session.sync(p.clone()).unwrap();
        let first = attach_recording(&mut session, p, job.clone(), &audio, None, false).unwrap();
        assert!(first.project.assets[0].file.ends_with(".wav"));
        let trimmed = session
            .operator_edit(OperatorOp::SetTrim {
                id: id.clone(),
                start_ms: Some("10".into()),
                end_ms: Some("20".into()),
            })
            .unwrap();
        let restored =
            attach_recording(&mut session, trimmed.project, job, &audio, None, true).unwrap();
        let TrackKind::Operator { orders, .. } = &restored.project.timelines[0].tracks[0].kind
        else {
            panic!()
        };
        assert_eq!((orders[0].trim_start, orders[0].trim_end), (0, 0));
        assert_eq!(orders[0].status(None), flux_project::VoiceStatus::Synced);
        assert_eq!(orders[0].generated.as_ref().unwrap().origin, origin);
        let archive = dir.path().join("show.flx");
        session.save(&restored.project, &archive).unwrap();
        std::fs::remove_file(audio).unwrap();
        let mut reopened = flux_project::Session::default();
        let p = reopened.open(&archive).unwrap();
        let programme = flux_engine::mixer::build(&p, &p.timelines[0], |asset| {
            Ok(Arc::new(flux_audio::decode(
                reopened.asset_bytes(asset)?,
                Some("wav"),
            )?))
        });
        assert!(programme.warnings.is_empty());
        assert!(!programme.set.is_empty());
    }
    #[test]
    fn duplicate_requests_are_refused_before_contacting_the_provider() {
        let state = VoiceState::default();
        let token = state.reserve().unwrap();
        assert!(matches!(state.reserve(), Err(Error::Busy)));
        assert_eq!(state.pending.lock().unwrap().as_ref().unwrap().token, token);
        *state.pending.lock().unwrap() = None;
        assert_ne!(state.reserve().unwrap(), token);
    }

    #[test]
    fn stopping_preview_invalidates_an_inflight_start() {
        let state = VoiceState::default();
        let epoch = state.preview_epoch.load(Ordering::Acquire);
        stop_preview(&state);
        assert_ne!(state.preview_epoch.load(Ordering::Acquire), epoch);
    }

    #[test]
    fn failed_disk_write_preserves_paid_result_and_retry_never_calls_provider() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("jobs");
        std::fs::write(&root, b"blocks directory creation").unwrap();
        let store = JobStore::new(root.clone());
        let state = VoiceState::default();
        let token = state.reserve().unwrap();
        state.pending.lock().unwrap().as_mut().unwrap().unsaved = Some(Arc::new(Unsaved {
            origin: Default::default(),
            extension: "mp3".into(),
            voice_name: None,
            snapshot: Snapshot {
                mode: Default::default(),
                synthesis_text: None,
                project: "project".into(),
                timeline: "timeline".into(),
                order: "order".into(),
                dialogue: "Go".into(),
                voice: "voice".into(),
                previous_asset: None,
            },
            session_id: "session".into(),
            bytes: b"paid result".to_vec(),
            samples: 48_000,
        }));
        assert!(matches!(
            persist_pending(&state, &store),
            Err(Error::Storage)
        ));
        assert_eq!(state.pending.lock().unwrap().as_ref().unwrap().token, token);
        assert!(matches!(state.reserve(), Err(Error::Busy)));
        std::fs::remove_file(root).unwrap();
        let id = persist_pending(&state, &store).unwrap();
        assert!(state.pending.lock().unwrap().is_none());
        assert_eq!(
            std::fs::read(store.audio(&id).unwrap()).unwrap(),
            b"paid result"
        );
    }

    #[test]
    fn generation_snapshot_detects_dialogue_voice_order_and_project_changes() {
        let mut project = Project::new("Show", flux_project::FrameRate::Fps25);
        let active = project.timelines[0].id.clone();
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::AddTrack {
                name: "Operator".into(),
            },
        )
        .unwrap();
        let track = project.timelines[0].tracks[0].id.clone();
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::AddOrder {
                track_id: track,
                at: 10,
            },
        )
        .unwrap();
        let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind else {
            panic!("operator track");
        };
        orders[0].dialogue = "Go".into();
        let id = orders[0].id.clone();
        let before = snapshot(&project, &id, Some("voice")).unwrap();
        project.name = "Renamed show".into();
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::EditOrder {
                id: id.clone(),
                dialogue: None,
                at: Some(50),
                alignment: Some(flux_project::Alignment::End),
            },
        )
        .unwrap();
        assert!(before == snapshot(&project, &id, Some("voice")).unwrap());
        assert!(before != snapshot(&project, &id, Some("other")).unwrap());
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::EditOrder {
                id: id.clone(),
                dialogue: Some("Stand by".into()),
                at: None,
                alignment: None,
            },
        )
        .unwrap();
        assert!(before != snapshot(&project, &id, Some("voice")).unwrap());
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::RemoveOrder { id: id.clone() },
        )
        .unwrap();
        assert!(matches!(
            snapshot(&project, &id, Some("voice")),
            Err(Error::Obsolete)
        ));
    }

    #[test]
    fn provider_errors_do_not_serialize_private_details() {
        let json = serde_json::to_string(&Error::Provider {
            error: flux_voice::Error::Credentials,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"code":"provider","error":{"code":"credentials"}}"#
        );
    }

    #[test]
    fn attaching_preserves_anchor_and_rejects_stale_edits_without_losing_the_job() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path().join("jobs"));
        let mut session = flux_project::Session::default();
        session.new_project("Show", flux_project::FrameRate::Fps25);
        session
            .operator_edit(OperatorOp::AddTrack {
                name: "Operator".into(),
            })
            .unwrap();
        let track = session.document().unwrap().project.timelines[0].tracks[0]
            .id
            .clone();
        let mut project = session
            .operator_edit(OperatorOp::AddOrder {
                track_id: track,
                at: 50,
            })
            .unwrap()
            .project;
        let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind else {
            panic!()
        };
        orders[0].dialogue = "Go".into();
        let id = orders[0].id.clone();
        session.sync(project.clone()).unwrap();
        let job = store
            .save(
                snapshot(&project, &id, Some("voice")).unwrap(),
                session.recovery_revision().0.into(),
                b"audio fixture",
                48_000,
            )
            .unwrap();
        let audio = store.audio(&job.id).unwrap();
        let old_command = project;
        let moved = session
            .operator_edit(OperatorOp::EditOrder {
                id: id.clone(),
                dialogue: None,
                at: Some(100),
                alignment: Some(flux_project::Alignment::End),
            })
            .unwrap()
            .project;
        assert!(matches!(
            attach_recording(
                &mut session,
                old_command,
                job.clone(),
                &audio,
                Some("voice"),
                false
            ),
            Err(Error::Obsolete)
        ));
        assert_eq!(session.document().unwrap().project, moved);
        let attached = attach_recording(
            &mut session,
            moved,
            job.clone(),
            &audio,
            Some("voice"),
            false,
        )
        .unwrap();
        let TrackKind::Operator { orders, .. } = &attached.project.timelines[0].tracks[0].kind
        else {
            panic!()
        };
        assert_eq!(
            (orders[0].at, orders[0].alignment),
            (100, flux_project::Alignment::End)
        );
        assert_eq!(orders[0].generated.as_ref().unwrap().asset, job.id);
        let changed = session
            .operator_edit(OperatorOp::EditOrder {
                id,
                dialogue: Some("New draft".into()),
                at: None,
                alignment: None,
            })
            .unwrap()
            .project;
        assert!(matches!(
            attach_recording(
                &mut session,
                changed.clone(),
                job.clone(),
                &audio,
                Some("voice"),
                false
            ),
            Err(Error::Obsolete)
        ));
        let recovered = attach_recording(
            &mut session,
            changed,
            job.clone(),
            &audio,
            Some("other"),
            true,
        )
        .unwrap();
        let TrackKind::Operator { orders, .. } = &recovered.project.timelines[0].tracks[0].kind
        else {
            panic!()
        };
        assert_eq!(orders[0].dialogue, "New draft");
        assert_eq!(orders[0].generated.as_ref().unwrap().dialogue, "Go");
        assert_eq!(
            orders[0].status(Some("other")),
            flux_project::VoiceStatus::Stale
        );
        assert_eq!(
            recovered.project.assets.len(),
            1,
            "recovering twice reuses the durable asset id"
        );
        assert_eq!(store.list(&recovered.project.meta.id).unwrap().len(), 1);
    }
}
