pub mod audio;
mod audio_commands;
mod commands;
pub mod devices;
pub mod engine;
pub mod project;
mod transport_commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::SharedSession::default())
        .manage(audio::SharedAudio::default())
        .manage(transport_commands::SharedOutput::default())
        .invoke_handler(tauri::generate_handler![
            commands::project_new,
            commands::project_open,
            commands::project_import_audio,
            commands::project_save,
            audio_commands::audio_analyze,
            devices::devices_list_audio,
            devices::devices_list_midi,
            devices::devices_find_audio,
            devices::devices_find_midi,
            transport_commands::output_start,
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
