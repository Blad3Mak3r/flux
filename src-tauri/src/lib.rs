pub mod audio;
mod audio_commands;
mod commands;
pub mod devices;
pub mod engine;
pub mod project;
mod recents;
mod startup;
mod transport_commands;
mod windows;

/// WSLg's Wayland compositor places every window itself and ignores the position an app asks for,
/// so the windows would not open centred. Its X11 server honours it.
#[cfg(target_os = "linux")]
fn prefer_x11_under_wsl() {
    use std::env::{set_var, var_os};

    if var_os("WSL_DISTRO_NAME").is_some() && var_os("GDK_BACKEND").is_none() {
        // SAFETY: called first thing in `run`, before any thread exists.
        unsafe { set_var("GDK_BACKEND", "x11") };
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    prefer_x11_under_wsl();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Checked and installed by the launcher window before the app opens.
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .manage(commands::SharedSession::default())
        .manage(audio::SharedAudio::default())
        .manage(transport_commands::SharedOutput::default())
        .invoke_handler(tauri::generate_handler![
            commands::project_document,
            commands::project_sync,
            commands::project_new,
            commands::project_open,
            commands::project_import_audio,
            commands::project_save,
            commands::project_delete,
            recents::recents_list,
            recents::recents_forget,
            startup::startup_run,
            windows::window_show_welcome,
            windows::window_open_editor,
            audio_commands::audio_analyze,
            devices::devices_list_audio,
            devices::devices_list_midi,
            devices::devices_find_audio,
            devices::devices_find_midi,
            transport_commands::output_start,
            transport_commands::output_set_clips,
            transport_commands::output_set_track_gain,
            transport_commands::output_stop,
            transport_commands::transport_play,
            transport_commands::transport_pause,
            transport_commands::transport_stop,
            transport_commands::transport_seek,
            transport_commands::transport_position,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
