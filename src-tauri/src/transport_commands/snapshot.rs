use super::*;

/// Whether the transport of the running output is playing.
pub fn is_playing(state: &SharedOutput) -> bool {
    lock(state)
        .as_ref()
        .is_some_and(|running| running.output.playing_or_pending())
}

/// Where the transport of the running output is and the cues it plays, for the remote control.
pub(crate) fn snapshot(state: &SharedOutput) -> Option<(Position, Arc<CueList>)> {
    lock(state).as_ref().map(|running| {
        let reader = running.output.reader();
        if let Some((plan, tick, position)) = reader.show_snapshot() {
            (position, Arc::new(plan.entries[tick.entry].cues.clone()))
        } else {
            (reader.position(), running.cues.current())
        }
    })
}

/// What the pads of a MIDI controller light from: the transport, the cues and the tick of the
/// prepared setlist, read the way the Live view reads them (`show_commands::state`): a lost audio
/// device stops the transport and takes GO away.
pub(crate) fn light_snapshot(
    state: &SharedOutput,
) -> Option<(Position, Arc<CueList>, Option<flux_engine::show::ShowTick>)> {
    lock(state).as_ref().map(|running| {
        let reader = running.output.reader();
        if let Some((plan, mut tick, mut position)) = reader.show_snapshot() {
            if position.device_lost {
                tick.go = flux_engine::show::GoAction::None;
                position.state = TransportState::Stopped;
            }
            (
                position,
                Arc::new(plan.entries[tick.entry].cues.clone()),
                Some(tick),
            )
        } else {
            (reader.position(), running.cues.current(), None)
        }
    })
}

/// How the running output ended up, for the diagnostics.
pub fn running_info(state: &SharedOutput) -> Option<OutputInfo> {
    lock(state)
        .as_ref()
        .map(|running| running.output.info().clone())
}

pub fn routing_snapshot(state: &SharedOutput) -> Option<AudioRoutingState> {
    let guard = lock(state);
    let running = guard.as_ref()?;
    let desired = running
        .desired_audio
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Some(AudioRoutingState {
        active: running.output.reader().audio_routes(),
        active_tracks: running.setup.output.track_routes.clone(),
        desired_tracks: desired.track_routes.clone(),
        pending: desired.audio_routes != running.setup.output.audio_routes
            || desired.track_routes != running.setup.output.track_routes,
    })
}

/// Closes the running output, if any. Taken first and dropped after: stopping waits for threads,
/// and the lock must be free for the `transport_*` commands, which run on the main thread, in the
/// meantime.
pub async fn release(state: &SharedOutput) {
    let state = Arc::clone(state);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
        let previous = lock(&state).take();
        let running = previous.is_some();
        drop(previous);
        if running {
            log::info!("output stopped");
        }
    })
    .await;
}
