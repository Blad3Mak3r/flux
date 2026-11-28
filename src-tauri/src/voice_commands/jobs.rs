use super::*;

pub(super) fn job_store(app: &AppHandle) -> Result<JobStore> {
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

pub(super) fn persist_pending(state: &VoiceState, store: &JobStore) -> Result<String> {
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

pub(super) fn attach_recording(
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
