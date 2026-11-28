use super::*;

/// Opening a MIDI port can block, so the commands run off the main thread.
async fn blocking<T: Send + 'static>(op: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .expect("the remote control task panicked")
}

#[tauri::command]
pub async fn control_get(control: State<'_, SharedControl>) -> Result<ControlView, String> {
    let control = Arc::clone(&control);
    Ok(blocking(move || control.view()).await)
}

/// Sends the view down `on_view` whenever it changes (the Art-Net packet counter moves by itself),
/// until [`crate::watch::watch_stop`].
#[tauri::command]
pub fn control_watch(
    control: State<'_, SharedControl>,
    watches: State<'_, crate::watch::Watches>,
    on_view: tauri::ipc::Channel<ControlView>,
) {
    let control = Arc::clone(&control);
    let mut changes = crate::watch::Changes::default();
    watches.start(on_view, Duration::from_secs(1), move || {
        changes.next(control.view())
    });
}

/// Keeps `prefs`, opens the inputs they ask for and answers with how that went. Sending the same
/// preferences again retries a port that was not connected.
#[tauri::command]
pub async fn control_set(
    app: AppHandle,
    control: State<'_, SharedControl>,
    prefs: ControlPrefs,
) -> Result<ControlView, String> {
    let control = Arc::clone(&control);
    Ok(blocking(move || {
        control.apply(&app, prefs);
        control.view()
    })
    .await)
}

/// A key the window heard in a VJ show, pressed or let go, for the mappings (or to learn).
#[tauri::command]
pub fn control_key(control: State<'_, SharedControl>, key: KeyMsg) {
    control.key(key);
}

/// Starts or cancels waiting for a trigger to learn: the next one is sent to the window with
/// [`CONTROL_LEARNED`] and not executed.
#[tauri::command]
pub fn control_learn(control: State<'_, SharedControl>, on: bool) {
    control.learning.store(on, Ordering::Relaxed);
}
