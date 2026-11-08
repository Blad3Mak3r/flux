//! Microphone takes use the same durable job and attachment path as imported speech.
use super::*;

pub(super) struct ActiveRecording {
    capture: flux_engine::recording::Capture,
    snapshot: Snapshot,
    session_id: String,
    token: u64,
}

fn capture_error(error: flux_engine::recording::Error) -> Error {
    match error {
        flux_engine::recording::Error::Storage => Error::Storage,
        _ => Error::Capture,
    }
}

#[tauri::command]
pub async fn voice_inputs(driver: Option<String>) -> Result<Vec<flux_engine::recording::Input>> {
    worker(move || flux_engine::recording::inputs(driver.as_deref()).map_err(capture_error)).await
}

#[tauri::command]
pub async fn voice_record_start(
    app: AppHandle,
    session: State<'_, SharedSession>,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
    project: Project,
    id: String,
) -> Result<()> {
    project.validate().map_err(|_| Error::InvalidInput)?;
    let snapshot = snapshot(&project, &id, Some("recorded"))?;
    let prefs = preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map_err(|_| Error::Storage)?;
    let mic = prefs.microphone.ok_or(Error::InvalidInput)?;
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|_| Error::Storage)?
        .join("microphone");
    let (state, session, output) = (
        Arc::clone(&state),
        Arc::clone(&session),
        Arc::clone(&output),
    );
    worker(move || {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if !transport_stopped(&output) {
            return Err(Error::Stopped);
        }
        let session_id = {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            if session.document().is_none_or(|d| d.project != project) {
                return Err(Error::Obsolete);
            }
            session.recovery_revision().0.to_owned()
        };
        let token = state.reserve()?;
        let _reservation = Reservation {
            state: &state,
            token,
        };
        invalidate_preview(&state);
        std::fs::create_dir_all(&directory).map_err(|_| Error::Storage)?;
        let path = tempfile::Builder::new()
            .suffix(".wav")
            .tempfile_in(&directory)
            .map_err(|_| Error::Storage)?
            .into_temp_path()
            .keep()
            .map_err(|_| Error::Storage)?;
        let capture =
            flux_engine::recording::Capture::start(mic.driver, mic.device, mic.channel, path)
                .map_err(capture_error)?;
        *state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(ActiveRecording {
            capture,
            snapshot,
            session_id,
            token,
        });
        // The recording, rather than a short-lived worker, now owns the reservation.
        std::mem::forget(_reservation);
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn voice_record_status(
    state: State<'_, Arc<VoiceState>>,
) -> Option<flux_engine::recording::Status> {
    state
        .recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|r| r.capture.status())
}

#[tauri::command]
pub async fn voice_record_stop(
    app: AppHandle,
    state: State<'_, Arc<VoiceState>>,
) -> Result<String> {
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    worker(move || {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        let active = state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or(Error::Obsolete)?;
        let _reservation = Reservation {
            state: &state,
            token: active.token,
        };
        let bytes = active.capture.finish().map_err(capture_error)?;
        let decoded = decode_recording(bytes.clone(), "wav")?;
        let audio = Unsaved {
            origin: flux_project::operator::RecordingOrigin::Recorded,
            extension: "wav".into(),
            voice_name: None,
            snapshot: active.snapshot,
            session_id: active.session_id,
            samples: decoded.frames() as u64,
            bytes,
        };
        drop(decoded);
        state
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .filter(|p| p.token == active.token)
            .ok_or(Error::Obsolete)?
            .unsaved = Some(Arc::new(audio));
        persist_pending(&state, &store)
    })
    .await
}

#[tauri::command]
pub async fn voice_record_cancel(state: State<'_, Arc<VoiceState>>) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(mut active) = state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            active.capture.cancel();
            let mut pending = state.pending.lock().unwrap_or_else(PoisonError::into_inner);
            if pending.as_ref().is_some_and(|p| p.token == active.token) {
                *pending = None;
            }
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn voice_job_discard(
    app: AppHandle,
    state: State<'_, Arc<VoiceState>>,
    session: State<'_, SharedSession>,
    job_id: String,
) -> Result<()> {
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    let session = Arc::clone(&session);
    worker(move || {
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        let job = store.read(&job_id).map_err(|_| Error::Storage)?;
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        let doc = session.document().ok_or(Error::Obsolete)?;
        if doc.project.meta.id != job.snapshot.project
            || job.origin != flux_project::operator::RecordingOrigin::Recorded
            || doc.project.assets.iter().any(|a| a.id == job_id)
        {
            return Err(Error::Obsolete);
        }
        store.discard(&job_id).map_err(|_| Error::Storage)
    })
    .await
}
