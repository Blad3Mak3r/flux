use super::*;

#[tauri::command]
pub async fn output_stop(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    release(&state).await;
    Ok(())
}

/// Serializes hardware transitions with transport commands, including remote control.
static OUTPUT_TRANSITION: Mutex<()> = Mutex::new(());

pub(super) fn transition() -> MutexGuard<'static, ()> {
    OUTPUT_TRANSITION
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn apply_pending(state: &SharedOutput) -> Result<(), OutputError> {
    let mut guard = lock(state);
    let Some(running) = guard.as_mut() else {
        return Err(OutputError::NotRunning);
    };
    if running.output.reader().position().state == TransportState::Playing
        || running.pending_audio.is_none()
    {
        return Ok(());
    }
    let pending = running
        .pending_audio
        .take()
        .ok_or(OutputError::NotRunning)?;
    let position = running.output.reader().position();
    let setup = OutputSetup {
        timeline_id: pending.timeline.id.clone(),
        timecode: pending.timeline.timecode.clone(),
        output: pending.config.clone(),
        advance: running.setup.advance,
        audition: pending.audition,
    };
    let channels = running.channels.clone();
    let app = running.app.clone();
    let advance = running.advance.clone();
    let cues = running.cues.clone();
    let old = guard.take();
    drop(guard);
    drop(old);
    let output = Output::start(
        &pending.config,
        &pending.timeline,
        pending.programme,
        !pending.audition,
    )?;
    output.send(Command::Restore {
        samples: position.position_samples,
        paused: position.state == TransportState::Paused,
    })?;
    diagnostics::output_reconfigured(output.info());
    let running = Running::new(
        output,
        setup,
        pending.preferences,
        cues,
        app,
        advance,
        channels.position,
        channels.levels,
        channels.live,
        channels.routing,
    )?;
    *lock(state) = Some(running);
    Ok(())
}

pub(crate) fn send(state: &SharedOutput, command: Command) -> Result<(), OutputError> {
    let _transition = transition();
    if matches!(command, Command::Play) {
        for _ in 0..200 {
            let waiting = lock(state).as_ref().is_some_and(|r| {
                r.stationary_requested
                    && r.pending_audio.is_some()
                    && r.output.reader().position().state == TransportState::Playing
            });
            if !waiting {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        if lock(state).as_ref().is_some_and(|r| {
            r.stationary_requested
                && r.pending_audio.is_some()
                && r.output.reader().position().state == TransportState::Playing
        }) {
            return Err(OutputError::Stream(
                "waiting for the audio device to pause before applying routing".into(),
            ));
        }
        apply_pending(state)?;
    }
    {
        let mut guard = lock(state);
        let running = guard.as_mut().ok_or(OutputError::NotRunning)?;
        if matches!(command, Command::Play)
            && let Some((plan, _)) = running.output.reader().show()
        {
            plan.execution_lock.store(true, Ordering::Release);
        }
        running.output.send(command)?;
        if matches!(command, Command::Pause | Command::Stop) {
            running.stationary_requested = true;
        }
        if matches!(command, Command::Play) {
            running.stationary_requested = false;
        }
    }
    if matches!(command, Command::Pause | Command::Stop) {
        let state = Arc::clone(state);
        tauri::async_runtime::spawn_blocking(move || {
            // Wait for the callback to consume Pause/Stop before closing any stream.
            for _ in 0..200 {
                thread::sleep(Duration::from_millis(5));
                let _transition = transition();
                let stationary = lock(&state)
                    .as_ref()
                    .is_none_or(|r| r.output.reader().position().state != TransportState::Playing);
                if stationary {
                    if let Err(error) = apply_pending(&state) {
                        log::warn!("pending audio routing could not apply: {error}");
                    }
                    break;
                }
            }
        });
    }
    Ok(())
}

#[tauri::command(async)]
pub fn transport_play(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Play)
}

#[tauri::command(async)]
pub fn transport_pause(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Pause)
}

/// Stops and goes back to the start of the timeline.
#[tauri::command(async)]
pub fn transport_stop(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Stop)
}

/// Moves to a timecode given as a frame count.
#[tauri::command(async)]
pub fn transport_seek(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
    frame: u64,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Seek(frame))
}

/// The current position, or `null` when no output is running.
#[tauri::command(async)]
pub fn transport_position(state: State<'_, SharedOutput>) -> Option<Position> {
    lock(&state)
        .as_ref()
        .map(|running| running.output.reader().position())
}

pub(crate) fn show_reader(state: &SharedOutput) -> Option<flux_engine::output::PositionReader> {
    Some(lock(state).as_ref()?.output.reader())
}

pub(crate) fn show_send(
    state: &SharedOutput,
    command: flux_engine::show::ShowCommand,
) -> Result<(), OutputError> {
    let _transition = transition();
    let guard = lock(state);
    let running = guard.as_ref().ok_or(OutputError::NotRunning)?;
    let Some((plan, tick)) = running.output.reader().show() else {
        return Err(OutputError::NotRunning);
    };
    if matches!(command, flux_engine::show::ShowCommand::Go)
        && matches!(
            tick.go,
            flux_engine::show::GoAction::Start | flux_engine::show::GoAction::Resume
        )
    {
        plan.execution_lock.store(true, Ordering::Release);
    }
    if running.output.reader().position().device_lost {
        return Err(OutputError::Stream("show.error.deviceLost".into()));
    }
    if matches!(command, flux_engine::show::ShowCommand::Prepare(_))
        && running.output.reader().position().state != TransportState::Stopped
    {
        return Err(OutputError::Stream("show.error.stop".into()));
    }
    running.output.show_command(command)
}
