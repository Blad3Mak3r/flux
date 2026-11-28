use super::*;

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
