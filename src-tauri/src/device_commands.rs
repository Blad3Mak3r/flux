//! Tauri commands that list and look up devices; the work is in [`flux_engine::devices`].

use std::io;

use flux_engine::devices::{
    AudioDevice, AudioHost, DeviceError, MidiPort, find_by_name, list_audio, list_hosts, list_midi,
};

/// Enumeration can probe hardware and block, so the commands run it off the main thread.
async fn blocking<T, F>(op: F) -> Result<T, DeviceError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DeviceError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|err| DeviceError::Io(io::Error::other(err)))?
}

#[tauri::command]
pub async fn devices_list_hosts() -> Result<Vec<AudioHost>, DeviceError> {
    blocking(|| Ok(list_hosts())).await
}

/// The output devices of the audio driver `driver` names; `null` is the system's default driver.
#[tauri::command]
pub async fn devices_list_audio(driver: Option<String>) -> Result<Vec<AudioDevice>, DeviceError> {
    blocking(move || Ok(list_audio(driver.as_deref()))).await
}

#[tauri::command]
pub async fn devices_list_midi() -> Result<Vec<MidiPort>, DeviceError> {
    blocking(list_midi).await
}

/// The audio device a project's name hint refers to on `driver`, or `null` when it is not connected.
#[tauri::command]
pub async fn devices_find_audio(
    name: String,
    driver: Option<String>,
) -> Result<Option<AudioDevice>, DeviceError> {
    blocking(move || {
        Ok(find_by_name(&name, &list_audio(driver.as_deref()), 0, |d| &d.name).cloned())
    })
    .await
}

/// The MIDI port a project's name hint refers to, or `null` when it is not connected. A system
/// without MIDI support has no ports, so that also resolves to `null` instead of an error.
#[tauri::command]
pub async fn devices_find_midi(
    name: String,
    index: usize,
) -> Result<Option<MidiPort>, DeviceError> {
    blocking(move || {
        let ports = list_midi().unwrap_or_default();
        Ok(find_by_name(&name, &ports, index, |p| &p.name).cloned())
    })
    .await
}
