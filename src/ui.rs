use std::sync::atomic::Ordering;

use anyhow::Result;
use tauri::{Manager, WebviewWindow, WebviewWindowBuilder};

use crate::{LatestFrame, SharedStatus, Shutdown};

pub struct UiState {
    pub latest: LatestFrame,
    pub status: SharedStatus,
    pub shutdown: Shutdown,
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
fn reconnect_device() {
    tracing::info!("Device reconnect requested from Flux panel");
}

#[tauri::command]
fn quit_flux(app: tauri::AppHandle, state: tauri::State<'_, UiState>) {
    state.shutdown.store(true, Ordering::Relaxed);
    app.exit(0);
}

pub fn run(state: UiState) -> Result<()> {
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![runtime_snapshot, reconnect_device, quit_flux])
        .setup(|app| {
            let window = WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Flux")
                .visible(false)
                .decorations(false)
                .resizable(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .build()?;
            install_tray(app, &window)?;
            Ok(())
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
        .icon(app.default_window_icon().expect("Flux icon missing").clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_panel(app),
            "reconnect" => tracing::info!("Device reconnect requested from tray"),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if matches!(event, tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. }) {
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
