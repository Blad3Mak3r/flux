use super::*;

pub(super) fn local_voice(app: &AppHandle) -> Result<Option<String>> {
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
        let uses_video = project.timelines.iter().any(|timeline| {
            timeline.tracks.iter().any(|track| {
                matches!(&track.kind, flux_project::TrackKind::Video { clips } if !clips.is_empty())
            })
        });
        // A screen on a display that is not connected is reported first; otherwise whether any
        // screen opens at all.
        let chosen: Vec<String> = flux_project::mapping::screens(&project.output.screens)
            .iter()
            .filter_map(|screen| prefs.video_display_for(&screen.id))
            .collect();
        let missing = chosen
            .iter()
            .find(|id| !crate::video_output::connected(&app, id));
        report.check_video(
            uses_video,
            missing.or(chosen.first()).map(String::as_str),
            missing.is_none(),
        );
        Ok(report)
    })
    .await
}
