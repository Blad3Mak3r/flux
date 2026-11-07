//! Credentials and speech workers. Provider responses and secrets never reach diagnostics.
use crate::{
    commands::SharedSession,
    preferences,
    transport_commands::{SharedOutput, is_playing},
};
use flux_project::{GeneratedVoice, OperatorOp, Project, ProjectState, TrackKind, VoiceView};
use serde::Serialize;
use std::io::Write;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
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
    Provider { error: flux_voice::Error },
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Default)]
pub struct VoiceState {
    key: Mutex<Option<String>>,
    session_only: Mutex<bool>,
    pending: Mutex<Option<Pending>>,
    serial: AtomicU64,
    preview: Mutex<Option<flux_engine::output::Output>>,
}

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    project: String,
    timeline: String,
    order: String,
    dialogue: String,
    voice: String,
}
struct Pending {
    token: u64,
    session_id: String,
    snapshot: Snapshot,
    audio: Option<(Vec<u8>, u64)>,
}

impl VoiceState {
    fn reserve(&self, snapshot: Snapshot, session_id: String) -> Result<u64> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.as_ref().is_some_and(|p| p.audio.is_none()) {
            return Err(Error::Busy);
        }
        let token = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        *pending = Some(Pending {
            token,
            session_id,
            snapshot,
            audio: None,
        });
        Ok(token)
    }
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
                if order.dialogue.trim().is_empty() {
                    return Err(Error::InvalidInput);
                }
                return Ok(Snapshot {
                    project: project.meta.id.clone(),
                    timeline: timeline.id.clone(),
                    order: id.into(),
                    dialogue: order.dialogue.clone(),
                    voice: voice.into(),
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
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => {}
            Err(_) => return Err(Error::SecureStore),
        }
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
        flux_voice::SpeechClient::new()
            .and_then(|c| c.voices(&api_key))
            .map_err(|error| Error::Provider { error })
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

/// Prepare only: committing against the window's latest project prevents overwriting intervening edits.
#[tauri::command]
pub async fn voice_generate(
    app: AppHandle,
    session: State<'_, SharedSession>,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<u64> {
    if crate::transport_commands::snapshot(&output)
        .is_some_and(|(p, _)| p.state != flux_engine::transport::TransportState::Stopped)
    {
        return Err(Error::Stopped);
    }
    project.validate().map_err(|_| Error::InvalidInput)?;
    let snapshot = snapshot(&project, &id, local_voice(&app)?.as_deref())?;
    let state = Arc::clone(&state);
    let session_id = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recovery_revision()
        .0
        .to_owned();
    let token = state.reserve(snapshot.clone(), session_id)?;
    worker(move || {
        let result = (|| {
            let api_key = key(&state)?;
            let bytes = flux_voice::SpeechClient::new()
                .and_then(|c| c.generate(&api_key, &snapshot.voice, &snapshot.dialogue))
                .map_err(|error| Error::Provider { error })?;
            let audio = flux_audio::decode(bytes.clone(), Some("mp3")).map_err(|_| Error::Audio)?;
            let samples = audio.frames() as u64;
            if samples == 0 || samples > u64::from(flux_core::ENGINE_RATE) * 3600 {
                return Err(Error::Audio);
            }
            Ok((bytes, samples))
        })();
        let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
        match result {
            Ok(audio) => {
                pending.as_mut().ok_or(Error::Obsolete)?.audio = Some(audio);
                Ok(token)
            }
            Err(error) => {
                *pending = None;
                Err(error)
            }
        }
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
    token: u64,
) -> Result<ProjectState> {
    let pending = {
        let mut guard = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_none_or(|p| p.token != token) {
            return Err(Error::Obsolete);
        }
        guard.take().ok_or(Error::Obsolete)?
    };
    if is_playing(&output) {
        return Err(Error::Stopped);
    }
    project.validate().map_err(|_| Error::InvalidInput)?;
    if snapshot(
        &project,
        &pending.snapshot.order,
        local_voice(&app)?.as_deref(),
    )? != pending.snapshot
    {
        return Err(Error::Obsolete);
    }
    let (bytes, samples) = pending.audio.ok_or(Error::Obsolete)?;
    let session = Arc::clone(&session);
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| Error::Storage)?
        .join("generated-voice");
    worker(move || {
        std::fs::create_dir_all(&directory).map_err(|_| Error::Storage)?;
        let mut file = tempfile::Builder::new()
            .prefix("flux-voice-")
            .suffix(".mp3")
            .tempfile_in(&directory)
            .map_err(|_| Error::Storage)?;
        file.write_all(&bytes).map_err(|_| Error::Storage)?;
        file.as_file().sync_all().map_err(|_| Error::Storage)?;
        let (_, path) = file.keep().map_err(|_| Error::Storage)?;
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        if session.recovery_revision().0 != pending.session_id
            || session.document().is_none_or(|d| {
                d.project != project
                    || d.project.meta.id != pending.snapshot.project
                    || d.active != pending.snapshot.timeline
            })
        {
            return Err(Error::Obsolete);
        }
        for asset in project
            .timelines
            .iter()
            .flat_map(|t| &t.tracks)
            .filter_map(|t| match &t.kind {
                TrackKind::Operator { orders, .. } => Some(orders),
                _ => None,
            })
            .flatten()
            .filter_map(|o| o.generated.as_ref())
            .filter_map(|g| project.assets.iter().find(|a| a.id == g.asset))
        {
            session
                .retain_voice_asset(asset, &directory)
                .map_err(|_| Error::Storage)?;
        }
        session.update(project);
        let mut asset = session.import_audio(&path).map_err(|_| Error::Storage)?;
        asset.name = pending.snapshot.dialogue.chars().take(64).collect();
        session
            .retain_voice_asset(&asset, &directory)
            .map_err(|_| Error::Storage)?;
        let generated = GeneratedVoice {
            asset: asset.id.clone(),
            dialogue: pending.snapshot.dialogue,
            voice: pending.snapshot.voice,
            model: flux_project::operator::VOICE_MODEL.into(),
            samples,
        };
        let result = session
            .change(|project, _| {
                for timeline in &mut project.timelines {
                    for track in &mut timeline.tracks {
                        if let TrackKind::Operator { orders, .. } = &mut track.kind
                            && let Some(order) =
                                orders.iter_mut().find(|o| o.id == pending.snapshot.order)
                        {
                            order.generated = Some(generated);
                            project.assets.push(asset);
                            return Ok(());
                        }
                    }
                }
                Err(flux_project::ProjectError::Invalid("unknown order".into()))
            })
            .map_err(|_| Error::InvalidInput)?;

        Ok(result)
    })
    .await
}

#[tauri::command]
pub fn voice_discard(state: State<'_, Arc<VoiceState>>, token: u64) {
    let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
    if pending
        .as_ref()
        .is_some_and(|p| p.token == token && p.audio.is_some())
    {
        *pending = None;
    }
}

pub fn stop_preview(state: &VoiceState) {
    let previous = state
        .preview
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    drop(previous);
}

#[tauri::command]
pub async fn voice_preview(
    app: AppHandle,
    state: State<'_, Arc<VoiceState>>,
    session: State<'_, SharedSession>,
    cache: State<'_, flux_audio::SharedAudio>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<()> {
    if is_playing(&output) {
        return Err(Error::Stopped);
    }
    project.validate().map_err(|_| Error::InvalidInput)?;
    let prefs = preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map_err(|_| Error::Storage)?;
    let route = prefs
        .preview
        .or(prefs.audio)
        .unwrap_or(flux_project::AudioOutput {
            device_name: String::new(),
            channels: vec![1, 2],
        });
    let state = Arc::clone(&state);
    let session = Arc::clone(&session);
    let cache = Arc::clone(&cache);
    worker(move || {
        stop_preview(&state);
        let mut timeline = project.timelines.iter().find(|t| t.tracks.iter().any(|track| matches!(&track.kind, TrackKind::Operator { orders, .. } if orders.iter().any(|o| o.id == id)))).cloned().ok_or(Error::Obsolete)?;
        timeline.timecode.start = 0;
        let mut samples = 0;
        timeline.tracks.retain_mut(|track| {
            if let TrackKind::Operator { orders, .. } = &mut track.kind {
                orders.retain(|o| o.id == id);
                if let Some(order) = orders.first_mut() {
                    order.at = 0;
                    order.alignment = flux_project::Alignment::Start;
                    samples = order.generated.as_ref().map_or(0, |g| g.samples);
                    track.muted = false;
                    return true;
                }
            }
            false
        });
        if samples == 0 { return Err(Error::Audio); }
        let mut programme = flux_engine::mixer::build(&project, &timeline, |a| crate::audio_commands::load(&session, &cache, a));
        if !programme.warnings.is_empty() { return Err(Error::Audio); }
        programme.set = programme.set.with_end(Some(samples));
        let config = flux_project::OutputConfig { driver: prefs.driver, audio: Some(route.clone()), operator: Some(route), ..Default::default() };
        let preview = flux_engine::output::Output::start(&config, &timeline, programme, false).map_err(|_| Error::Audio)?;
        preview.send(flux_engine::transport::Command::Play).map_err(|_| Error::Audio)?;
        *state.preview.lock().unwrap_or_else(PoisonError::into_inner) = Some(preview);
        Ok(())
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_requests_are_refused_before_contacting_the_provider() {
        let state = VoiceState::default();
        let snapshot = Snapshot {
            project: "show".into(),
            timeline: "timeline".into(),
            order: "order".into(),
            dialogue: "Go".into(),
            voice: "voice".into(),
        };
        let token = state.reserve(snapshot.clone(), "session".into()).unwrap();
        assert!(matches!(
            state.reserve(snapshot.clone(), "session".into()),
            Err(Error::Busy)
        ));
        assert_eq!(state.pending.lock().unwrap().as_ref().unwrap().token, token);
        state.pending.lock().unwrap().as_mut().unwrap().audio = Some((vec![1], 1));
        assert_ne!(state.reserve(snapshot, "session".into()).unwrap(), token);
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
        assert!(before == snapshot(&project, &id, Some("voice")).unwrap());
        assert!(before != snapshot(&project, &id, Some("other")).unwrap());
        flux_project::operator::apply(
            &mut project,
            &active,
            OperatorOp::EditOrder {
                id: id.clone(),
                dialogue: "Stand by".into(),
                at: 10,
                alignment: flux_project::Alignment::Start,
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
}

#[tauri::command]
pub async fn voice_preview_stop(state: State<'_, Arc<VoiceState>>) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        stop_preview(&state);
        Ok(())
    })
    .await
}
