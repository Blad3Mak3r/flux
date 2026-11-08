//! Auditions run on temporary outputs, independently of the show's clock and signals.
use super::{
    Error, Result, VoiceState, decode_recording, invalidate_preview, job_store, stop_preview,
    transport_stopped, worker,
};
use crate::{commands::SharedSession, preferences, transport_commands::SharedOutput};
use flux_project::{Project, TrackKind};
use std::sync::{Arc, PoisonError, atomic::Ordering};
use tauri::{AppHandle, Manager, State};

fn preview_epoch(state: &VoiceState, output: &SharedOutput) -> Result<u64> {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if !transport_stopped(output)
        || state.transitioning.load(Ordering::Acquire)
        || state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    {
        return Err(Error::Stopped);
    }
    invalidate_preview(state);
    Ok(state.preview_epoch.load(Ordering::Acquire))
}

fn audition(
    state: &VoiceState,
    output: &SharedOutput,
    epoch: u64,
    decoded: flux_audio::Decoded,
    route: flux_project::AudioOutput,
    driver: Option<String>,
) -> Result<()> {
    let frames = decoded.frames() as u64;
    let project = Project::new("Preview", flux_project::FrameRate::Fps25);
    let clip = flux_engine::mixer::ClipPlayer::new(Arc::new(decoded), 0, 1.0, 0);
    let set = flux_engine::mixer::ClipSet::new(vec![clip], vec![("preview".into(), 1.0)])
        .with_end(Some(frames));
    let config = flux_project::OutputConfig {
        audio_routes: Vec::new(),
        track_routes: Vec::new(),
        driver,
        audio: Some(route),
        ..Default::default()
    };
    let preview = flux_engine::output::Output::start(
        &config,
        &project.timelines[0],
        flux_engine::mixer::Programme {
            set,
            warnings: vec![],
        },
        false,
    )
    .map_err(|_| Error::Output)?;
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if state.preview_epoch.load(Ordering::Acquire) != epoch
        || !transport_stopped(output)
        || state.transitioning.load(Ordering::Acquire)
    {
        return Err(Error::Obsolete);
    }
    preview
        .send(flux_engine::transport::Command::Play)
        .map_err(|_| Error::Audio)?;
    *state.preview.lock().unwrap_or_else(PoisonError::into_inner) = Some(preview);
    Ok(())
}

#[tauri::command]
pub async fn voice_test_output(
    app: AppHandle,
    output_id: Option<String>,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
) -> Result<()> {
    let epoch = preview_epoch(&state, &output)?;
    let prefs = preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map_err(|_| Error::Storage)?;
    let route = match output_id.as_deref().unwrap_or("operator") {
        "programme" => prefs.audio.clone().or_else(|| {
            Some(flux_project::AudioOutput {
                device_name: String::new(),
                channels: vec![1, 2],
            })
        }),
        "operator" => prefs.operator.clone(),
        id => prefs.audio_routes.get(id).cloned(),
    }
    .ok_or(Error::MissingRoute)?;
    let state = Arc::clone(&state);
    let output = Arc::clone(&output);
    worker(move || {
        audition(
            &state,
            &output,
            epoch,
            decode_recording(
                include_bytes!("../../assets/operator-test.wav").to_vec(),
                "wav",
            )?,
            route,
            prefs.driver,
        )
    })
    .await
}

#[tauri::command]
pub async fn voice_job_preview(
    app: AppHandle,
    state: State<'_, Arc<VoiceState>>,
    output: State<'_, SharedOutput>,
    job_id: String,
) -> Result<()> {
    let epoch = preview_epoch(&state, &output)?;
    let prefs = preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .map_err(|_| Error::Storage)?;
    let route = prefs
        .preview
        .or(prefs.audio)
        .unwrap_or(flux_project::AudioOutput {
            device_name: String::new(),
            channels: vec![1, 2],
        });
    let store = job_store(&app)?;
    let state = Arc::clone(&state);
    let output = Arc::clone(&output);
    worker(move || {
        let job = store.read(&job_id).map_err(|_| Error::Storage)?;
        let audio = decode_recording(
            std::fs::read(store.audio(&job_id).map_err(|_| Error::Storage)?)
                .map_err(|_| Error::Storage)?,
            &job.extension,
        )?;
        if audio.frames() as u64 != job.samples {
            return Err(Error::Audio);
        }
        audition(&state, &output, epoch, audio, route, prefs.driver)
    })
    .await
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
    project.validate().map_err(|_| Error::InvalidInput)?;
    let epoch = preview_epoch(&state, &output)?;
    let session_id = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recovery_revision()
        .0
        .to_owned();
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
    let output = Arc::clone(&output);
    worker(move || {
        let mut timeline = project.timelines.iter().find(|t| t.tracks.iter().any(|track| matches!(&track.kind, TrackKind::Operator { orders, .. } if orders.iter().any(|o| o.id == id)))).cloned().ok_or(Error::Obsolete)?;
        timeline.timecode.start = 0;
        let mut samples = 0;
        timeline.tracks.retain_mut(|track| {
            if let TrackKind::Operator { orders, .. } = &mut track.kind {
                orders.retain(|o| o.id == id);
                if let Some(order) = orders.first_mut() {
                    order.at = 0;
                    order.alignment = flux_project::Alignment::Start;
                    samples = order.play_samples().unwrap_or(0);
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
        let config = flux_project::OutputConfig { audio_routes: Vec::new(), track_routes: Vec::new(), driver: prefs.driver, audio: Some(route.clone()), operator: Some(route), ..Default::default() };
        let preview = flux_engine::output::Output::start(&config, &timeline, programme, false).map_err(|_| Error::Output)?;
        let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
        if state.preview_epoch.load(Ordering::Acquire) != epoch || !transport_stopped(&output) { return Err(Error::Obsolete); }
        {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            if session.recovery_revision().0 != session_id || session.document().is_none_or(|d| d.project.meta.id != project.meta.id || d.active != timeline.id) { return Err(Error::Obsolete); }
        }
        preview.send(flux_engine::transport::Command::Play).map_err(|_| Error::Audio)?;
        *state.preview.lock().unwrap_or_else(PoisonError::into_inner) = Some(preview);
        Ok(())
    }).await
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
