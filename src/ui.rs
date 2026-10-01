use std::sync::Mutex;
use std::sync::atomic::Ordering;

use anyhow::Result;
use tauri::{Manager, WebviewWindow, WindowEvent};

use crate::runtime_control::SharedRuntimeControl;
use crate::settings::{self, SavedSettings};
use crate::{LatestFrame, SharedStatus, Shutdown};

pub struct UiState {
    pub latest: LatestFrame,
    pub status: SharedStatus,
    pub shutdown: Shutdown,
    pub settings: Mutex<SavedSettings>,
    pub runtime: SharedRuntimeControl,
}

#[tauri::command]
fn runtime_snapshot(state: tauri::State<'_, UiState>) -> crate::state::RuntimeSnapshot {
    state
        .status
        .lock()
        .expect("runtime status mutex poisoned")
        .snapshot(&state.latest)
}

#[tauri::command]
fn saved_settings(state: tauri::State<'_, UiState>) -> SavedSettings {
    state
        .settings
        .lock()
        .expect("settings mutex poisoned")
        .clone()
}

#[tauri::command]
fn save_settings(settings: SavedSettings, state: tauri::State<'_, UiState>) -> Result<(), String> {
    settings.validate().map_err(|error| error.to_string())?;
    settings::save(&settings::default_directory(), &settings).map_err(|error| error.to_string())?;
    *state.settings.lock().expect("settings mutex poisoned") = settings.clone();
    *state.latest.lock().expect("latest frame mutex poisoned") = None;
    state.runtime.replace(settings);
    tracing::info!("Flux configuration applied from the desktop UI");
    Ok(())
}

#[tauri::command]
fn reconnect_device(state: tauri::State<'_, UiState>) {
    tracing::info!("Device reconnect requested from Flux panel");
    state.runtime.restart();
}

#[tauri::command]
fn quit_flux(app: tauri::AppHandle, state: tauri::State<'_, UiState>) {
    state.shutdown.store(true, Ordering::Relaxed);
    app.exit(0);
}

pub fn run(state: UiState) -> Result<()> {
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            runtime_snapshot,
            saved_settings,
            save_settings,
            reconnect_device,
            quit_flux
        ])
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("Tauri configured main window is missing"))?;
            install_tray(app, &window)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                tracing::info!("Flux window hidden to the system tray");
            }
        })
        .run(tauri::generate_context!())
        .map_err(anyhow::Error::msg)
}

fn install_tray(app: &tauri::App, window: &WebviewWindow) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let open = MenuItem::with_id(app, "open", "Open Flux", true, None::<&str>)?;
    let reconnect = MenuItem::with_id(app, "reconnect", "Reconnect device", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Flux", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &reconnect, &quit])?;
    let panel = window.clone();
    TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .expect("Flux icon missing")
                .clone(),
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_panel(app),
            "reconnect" => restart_runtime(app),
            "quit" => request_shutdown(app),
            _ => {}
        })
        .on_tray_icon_event(move |_tray, event| {
            if matches!(
                event,
                tauri::tray::TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    ..
                }
            ) {
                let _ = panel.show();
                let _ = panel.set_focus();
            }
        })
        .build(app)?;
    Ok(())
}

fn show_panel(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn restart_runtime(app: &tauri::AppHandle) {
    tracing::info!("Device reconnect requested from the system tray");
    app.state::<UiState>().runtime.restart();
}

fn request_shutdown(app: &tauri::AppHandle) {
    app.state::<UiState>()
        .shutdown
        .store(true, Ordering::Relaxed);
    app.exit(0);
}
