pub mod audio;
mod audio_commands;
mod commands;
pub mod devices;
pub mod engine;
mod live_commands;
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
        .manage(commands::SharedSession::default())
        .manage(audio::SharedAudio::default())
        .manage(transport_commands::SharedOutput::default())
        .manage(windows::PendingOpen::from_args(std::env::args_os().skip(1)))
        .invoke_handler(tauri::generate_handler![
            commands::project_document,
            commands::project_sync,
            commands::timeline_add,
            commands::timeline_rename,
            commands::timeline_remove,
            commands::timeline_select,
            commands::timeline_move,
            commands::timeline_set_timecode,
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
            audio_commands::audio_detect_bpm,
            devices::devices_list_audio,
            devices::devices_list_midi,
            devices::devices_find_audio,
            devices::devices_find_midi,
            live_commands::live_cues,
            live_commands::live_state,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS passes the opened file in an event, not on the command line. Once the launcher
            // is past, the editor is already up and replacing it could lose unsaved work.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = event {
                use tauri::Manager;

                if let Some(path) = urls.iter().find_map(|url| url.to_file_path().ok()) {
                    app.state::<windows::PendingOpen>().set(path);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
