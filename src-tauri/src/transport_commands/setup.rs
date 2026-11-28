use super::*;

/// Opening devices and joining threads can block, so both run off the main thread. The previous
/// output is dropped outside the lock, because stopping it waits for threads that read the state.
// A Tauri command takes the managed state it uses and each channel the window gives it as an
// argument of its own, so it cannot be narrower.
#[expect(
    clippy::too_many_arguments,
    reason = "injected state and channels of a command"
)]
#[tauri::command]
pub async fn output_start(
    app: AppHandle,
    state: State<'_, SharedOutput>,
    prefs: State<'_, crate::preferences::Store>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    advance: bool,
    audition: bool,
    setlist_id: Option<String>,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
    on_audio_routing: Channel<AudioRoutingState>,
) -> Result<OutputInfo, OutputError> {
    project.validate()?;
    let prefs =
        crate::preferences::preferences_get(app.clone(), prefs).map_err(OutputError::Stream)?;
    let audition = audition && prefs.preview.is_some();
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let started = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ensure_not_running()?;
        let previous = lock(&state).take();
        let parked = previous.as_ref().map(|r| {
            (
                r.setup.timeline_id.clone(),
                r.setup.timecode.clone(),
                r.output.reader().position(),
            )
        });
        drop(previous);

        let show = if let Some(id) = &setlist_id {
            Some(crate::show_commands::prepare_plan(
                &project, id, &session, &audio,
            )?)
        } else {
            None
        };
        let timeline = if let Some(plan) = &show {
            &plan.entries[0].timeline
        } else {
            active_timeline(&session, &project)?
        };
        let mut resolved = prefs.resolve_project(&project, timeline, audition);
        if let Some(plan) = &show {
            for entry in &plan.entries {
                let other = prefs.resolve_project(&project, &entry.timeline, false);
                for route in other.track_routes {
                    if !resolved
                        .track_routes
                        .iter()
                        .any(|r| r.track_id == route.track_id)
                    {
                        resolved.track_routes.push(route);
                    }
                }
            }
        }
        let list = cue_list(&project, timeline, &session, &audio);
        let programme = programme(&project, timeline, &session, &audio, &list);
        let mut output =
            Output::start(&resolved, timeline, programme, !audition && show.is_none())?;
        if let Some(plan) = &show {
            output.install_show(Arc::clone(plan), &resolved.osc)?;
        }
        if let Some((id, timecode, position)) = parked
            && setlist_id.is_none()
            && id == timeline.id
            && timecode == timeline.timecode
        {
            output.send(Command::Restore {
                samples: position.position_samples,
                paused: position.state == TransportState::Paused,
            })?;
        }
        let setup = OutputSetup {
            timeline_id: timeline.id.clone(),
            timecode: timeline.timecode.clone(),
            output: resolved,
            advance,
            audition,
        };
        let info = output.info().clone();
        diagnostics::output_started(&info);
        let cues = SharedCues::new(list);
        // With `advance`, the show running to its end makes the next timeline the active one.
        let advance = (advance && setlist_id.is_none()).then(|| Advance {
            app: app.clone(),
            session: Arc::clone(&session),
        });
        let refresh = app.clone();
        *lock(&state) = Some(Running::new(
            output,
            setup,
            prefs,
            cues,
            app,
            advance,
            on_position,
            on_levels,
            on_live,
            on_audio_routing,
        )?);
        // The grid goes on playing across a restart of the output.
        crate::vj_audio::refresh(&refresh);
        Ok(info)
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    if let Err(err) = &started {
        log::error!("output could not start: {err}");
    }
    started
}

/// Brings the running output's LTC and MTC outputs in line with the project's, without touching the
/// programme's stream, so the change applies while playing. `needsRestart` says the change reaches
/// the programme's stream (its device, channels, the LTC on it, the metronome, the frame rate or
/// start): nothing was touched and the output has to be started again.
#[tauri::command]
pub async fn output_reconfigure(
    audio: State<'_, SharedAudio>,
    app: AppHandle,
    prefs: State<'_, crate::preferences::Store>,
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    project: Project,
    audition: bool,
) -> Result<Reconfigured, OutputError> {
    project.validate()?;
    if let Some(running) = lock(&state).as_ref()
        && running.output.reader().show().is_some()
    {
        return Ok(Reconfigured::Applied {
            info: Box::new(running.output.info().clone()),
        });
    }
    let prefs = crate::preferences::preferences_get(app, prefs).map_err(OutputError::Stream)?;
    let audition = audition && prefs.preview.is_some();
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let done = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
        let timeline = active_timeline(&session, &project)?;
        let resolved = prefs.resolve_project(&project, timeline, audition);
        {
            let mut guard = lock(&state);
            if let Some(running) = guard.as_mut()
                && running.setup.output == resolved
                && running.setup.timecode == timeline.timecode
                && running.setup.audition == audition
            {
                running.pending_audio = None;
                running.preferences = prefs.clone();
                *running
                    .desired_audio
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = resolved;
                return Ok(Reconfigured::Applied {
                    info: Box::new(running.output.info().clone()),
                });
            }
        }
        let list = cue_list(&project, timeline, &session, &audio);
        let programme = programme(&project, timeline, &session, &audio, &list);
        let done = match lock(&state).as_mut() {
            Some(running) => {
                *running
                    .desired_audio
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = resolved.clone();
                let position = running.output.reader().position();
                let needs_restart = audition != running.setup.audition
                    || (running.output.playing_or_pending()
                        && !position.device_lost
                        && !prefs.same_routing(&running.preferences));
                let done = if needs_restart {
                    Reconfigured::NeedsRestart
                } else {
                    running.output.reconfigure(&resolved, timeline)
                };
                if matches!(done, Reconfigured::NeedsRestart) {
                    running.pending_audio = Some(PendingAudio {
                        config: resolved.clone(),
                        timeline: timeline.clone(),
                        programme,
                        preferences: prefs.clone(),
                        audition,
                    });
                } else {
                    running.pending_audio = None;
                }
                // The LTC and MTC outputs now play what the project asks for, so a window that
                // attaches later must find them as requested.
                if matches!(done, Reconfigured::Applied { .. }) {
                    running.setup.output = resolved;
                    running.preferences = prefs;
                }
                Ok(done)
            }
            None => Err(OutputError::NotRunning),
        };
        if matches!(done, Ok(Reconfigured::NeedsRestart)) && !is_playing(&state) {
            apply_pending(&state)?;
            let info = lock(&state)
                .as_ref()
                .ok_or(OutputError::NotRunning)?
                .output
                .info()
                .clone();
            return Ok(Reconfigured::Applied {
                info: Box::new(info),
            });
        }
        done
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    match &done {
        Ok(Reconfigured::Applied { info }) => diagnostics::output_reconfigured(info),
        Ok(Reconfigured::NeedsRestart) => log::info!("output settings changed, restart needed"),
        Err(err) => log::warn!("the output could not be reconfigured: {err}"),
    }
    done
}

/// Replaces a window's subscriptions without opening devices or sending transport commands.
#[tauri::command(async)]
pub fn output_attach(
    state: State<'_, SharedOutput>,
    setup: OutputSetup,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
    on_audio_routing: Channel<AudioRoutingState>,
) -> Option<AttachedOutput> {
    let mut running = lock(&state);
    let running = running.as_mut()?;
    let position = running.output.reader().position();
    running.channels = OutputChannels {
        position: on_position.clone(),
        levels: on_levels.clone(),
        live: on_live.clone(),
        routing: on_audio_routing.clone(),
    };
    let _ = running.on_position.send(on_position);
    let _ = running.on_levels.send(on_levels);
    let _ = running.on_live.send(on_live);
    let _ = running.on_audio_routing.send(on_audio_routing);
    Some(AttachedOutput {
        info: running.output.info().clone(),
        position,
        timeline_id: running.setup.timeline_id.clone(),
        start: running.setup.timecode.start,
        matches: setup == running.setup,
    })
}

/// The audio of the timeline's clips, decoding what has not been decoded yet, and where the show
/// ends according to `cues`. A file that cannot be read is left out and reported, never an error.
pub(crate) fn programme(
    project: &Project,
    timeline: &Timeline,
    session: &SharedSession,
    audio: &SharedAudio,
    cues: &CueList,
) -> Programme {
    let mut programme = mixer::build_with_video(
        project,
        timeline,
        |asset| load(session, audio, asset),
        |video| crate::video_commands::load_audio(session, audio, video),
    );
    programme.set = programme.set.with_end(Some(cues.end_samples()));
    programme
}

/// Sends the project's current clips to the running output, after tracks were muted or clips
/// moved. Returns the warnings about files that could not be used.
#[tauri::command]
pub async fn output_set_clips(
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
) -> Result<Vec<String>, OutputError> {
    if lock(&state)
        .as_ref()
        .is_some_and(|r| r.output.reader().show().is_some())
    {
        return Ok(Vec::new());
    }
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let updated = tauri::async_runtime::spawn_blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        let list = cue_list(&project, timeline, &session, &audio);
        let Programme { set, mut warnings } =
            programme(&project, timeline, &session, &audio, &list);
        let _transition = transition();
        match lock(&state).as_mut() {
            Some(running) => {
                running.cues.replace(list);
                // The cue tracks follow the edit too (their schedules, and any new port).
                warnings.extend(running.output.set_events(timeline, &project.output.osc));
                if let Some(pending) = &mut running.pending_audio {
                    pending.programme = Programme {
                        set: set.clone(),
                        warnings: warnings.clone(),
                    };
                    pending.timeline = timeline.clone();
                }
                running.output.set_clips(set).map(|()| warnings)
            }
            None => Err(OutputError::NotRunning),
        }
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    match &updated {
        Ok(warnings) => diagnostics::log_warnings(warnings),
        Err(err) => log::warn!("the clips could not be sent to the output: {err}"),
    }
    updated
}

/// Moves one track's fader on the running output, without rebuilding the programme, so it can
/// follow a drag. The project keeps the value once the edit is committed.
#[tauri::command(async)]
pub fn output_set_track_gain(
    state: State<'_, SharedOutput>,
    track_id: String,
    gain_db: f32,
) -> Result<(), OutputError> {
    match lock(&state).as_ref() {
        Some(running) => {
            running
                .output
                .levels()
                .current()
                .set_gain_db(&track_id, gain_db);
            Ok(())
        }
        None => Err(OutputError::NotRunning),
    }
}

/// Moves the metronome to the active timeline's tempo or click switch on the running output, so an
/// edit is heard at once. No output, or none with a metronome, is not an error: the next output
/// reads the tempo from the project.
#[tauri::command(async)]
pub fn output_set_tempo(
    state: State<'_, SharedOutput>,
    tempo: Tempo,
    changes: Vec<flux_project::TempoChange>,
    frame_rate: flux_project::FrameRate,
) {
    if let Some(running) = lock(&state).as_ref() {
        running.output.set_tempo(tempo, &changes, frame_rate);
    }
}
