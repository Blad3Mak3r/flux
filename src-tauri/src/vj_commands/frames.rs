use super::*;

/// What the grid sends to the screens now, before their mapping, as a small picture for the
/// editor's output monitor (see [`crate::video_output::VideoOutputs::composition`]); empty outside
/// a VJ project. Sent as bytes, not as JSON, so it is cheap enough to ask for thirty times a second.
#[tauri::command]
pub async fn vj_output_frame(
    app: tauri::AppHandle,
    output: State<'_, crate::video_output::SharedVideoOutput>,
) -> std::result::Result<tauri::ipc::Response, String> {
    let output = std::sync::Arc::clone(&output);
    let picture = tauri::async_runtime::spawn_blocking(move || output.composition(&app))
        .await
        .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(picture.unwrap_or_default()))
}

/// What one layer (`kind` `"layer"`) or one group (`"group"`) of the grid contributes to the
/// composition, `id` being its id, as a small picture in the format of [`vj_output_frame`] (see
/// [`crate::video_output::VideoOutputs::scoped`]); empty when there is no such layer or group.
#[tauri::command]
pub async fn vj_monitor_frame(
    app: tauri::AppHandle,
    output: State<'_, crate::video_output::SharedVideoOutput>,
    kind: String,
    id: String,
) -> std::result::Result<tauri::ipc::Response, String> {
    use crate::video_output::Scope;
    let scope = match kind.as_str() {
        "layer" => Scope::Layer(id),
        "group" => Scope::Group(id),
        other => return Err(format!("no monitor of kind {other}")),
    };
    let output = std::sync::Arc::clone(&output);
    let picture = tauri::async_runtime::spawn_blocking(move || output.scoped(&app, &scope))
        .await
        .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(picture))
}

/// What the selected clip alone looks like in the composition, as a small picture in the format of
/// [`vj_output_frame`] (see [`crate::video_output::VideoOutputs::preview`]); it plays on its own,
/// whatever is on air. `None` stops it; empty when there is no such clip.
#[tauri::command]
pub async fn vj_preview_frame(
    app: tauri::AppHandle,
    output: State<'_, crate::video_output::SharedVideoOutput>,
    clip: Option<String>,
) -> std::result::Result<tauri::ipc::Response, String> {
    let output = std::sync::Arc::clone(&output);
    let picture =
        tauri::async_runtime::spawn_blocking(move || output.preview(&app, clip.as_deref()))
            .await
            .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(picture))
}
