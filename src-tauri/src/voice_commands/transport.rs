use super::*;

pub(super) fn transport_stopped(output: &SharedOutput) -> bool {
    !is_playing(output)
        && crate::transport_commands::snapshot(output).is_none_or(|(position, _)| {
            position.state == flux_engine::transport::TransportState::Stopped
        })
}

pub(super) fn invalidate_preview(state: &VoiceState) {
    state.preview_epoch.fetch_add(1, Ordering::AcqRel);
    let previous = state
        .preview
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    drop(previous);
}

pub fn stop_preview(state: &VoiceState) {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    invalidate_preview(state);
}

pub fn send_transport(
    state: &VoiceState,
    output: &SharedOutput,
    command: flux_engine::transport::Command,
) -> std::result::Result<(), flux_engine::output::OutputError> {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if matches!(command, flux_engine::transport::Command::Play)
        && state
            .recording
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    {
        return Err(flux_engine::output::OutputError::Recording);
    }
    invalidate_preview(state);
    crate::transport_commands::send(output, command)
}

pub(crate) fn send_show(
    state: &VoiceState,
    output: &SharedOutput,
    command: flux_engine::show::ShowCommand,
) -> std::result::Result<(), flux_engine::output::OutputError> {
    let _gate = state.control.lock().unwrap_or_else(PoisonError::into_inner);
    if state
        .recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
    {
        return Err(flux_engine::output::OutputError::Recording);
    }
    invalidate_preview(state);
    crate::transport_commands::show_send(output, command)
}
