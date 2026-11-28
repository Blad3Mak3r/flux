use super::*;

/// What the audio analysis listens to now, loading the preferences if they are not yet.
pub fn analysis_source(app: &AppHandle) -> AnalysisSource {
    let state = app.state::<Store>();
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .get_or_insert_with(|| {
            file(app)
                .map(|p| load(&p))
                .unwrap_or_else(|error| Preferences {
                    warning: Some(error),
                    ..Default::default()
                })
        })
        .analysis
        .clone()
        .unwrap_or_default()
}

#[tauri::command]
pub fn preferences_get(app: AppHandle, state: State<'_, Store>) -> Result<Preferences, String> {
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(guard
        .get_or_insert_with(|| {
            file(&app)
                .map(|p| load(&p))
                .unwrap_or_else(|error| Preferences {
                    warning: Some(error),
                    ..Default::default()
                })
        })
        .clone())
}

#[tauri::command]
pub fn preferences_change(
    app: AppHandle,
    state: State<'_, Store>,
    change: Change,
) -> Result<Preferences, String> {
    let path = file(&app)?;
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    let current = guard.get_or_insert_with(|| load(&path));
    let mut next = current.clone();
    next.apply(change)?;
    next.warning = None;
    save(&path, &next)?;
    next.revision = current.revision + 1;
    *current = next.clone();
    let _ = app.emit("preferences-changed", &next);
    Ok(next)
}

#[derive(Serialize)]
pub struct ResolvedOutputs {
    choices: Vec<flux_project::routing::AudioChoice>,
    live: OutputConfig,
    edit: OutputConfig,
    audition: bool,
}

#[tauri::command]
pub fn preferences_resolve(
    app: AppHandle,
    state: State<'_, Store>,
    project: flux_project::Project,
    session: State<'_, crate::commands::SharedSession>,
) -> Result<ResolvedOutputs, String> {
    project.validate().map_err(|e| e.to_string())?;
    let timeline = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .active_timeline(&project)
        .map_err(|e| e.to_string())?
        .clone();
    let prefs = preferences_get(app, state)?;
    let audition = prefs.preview.is_some();
    Ok(ResolvedOutputs {
        choices: flux_project::routing::choices(&project),
        live: prefs.resolve_project(&project, &timeline, false),
        edit: prefs.resolve_project(&project, &timeline, audition),
        audition,
    })
}
