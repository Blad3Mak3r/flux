//! Where the VJ screen's dock layout is kept (one `vj-layout.json` in the app data folder, per
//! machine) and the commands that edit it. Every command answers with the whole state so the UI
//! only draws it. The rules are in `flux_project::vj_layout`.

use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Manager};

use flux_project::vj_layout::{Layout, MonitorKind, State, Store, Target};
use flux_project::{ProjectError, Result};

/// Commands read and rewrite the whole file, and windows can ask at the same time.
static STORE: Mutex<()> = Mutex::new(());

/// Reads the store, lets `edit` change it and writes it back (only when `edit` succeeds); answers
/// with what the UI shows plus whatever `edit` returned.
fn edit<T>(app: &AppHandle, edit: impl FnOnce(&mut Store) -> Result<T>) -> Result<(State, T)> {
    let _guard = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    let file = app
        .path()
        .app_data_dir()
        .map_err(|err| ProjectError::Invalid(format!("there is no app data folder: {err}")))?
        .join("vj-layout.json");
    let mut store = Store::load(&file)?;
    let out = edit(&mut store)?;
    store.save(&file)?;
    Ok((store.state(), out))
}

fn on_current(app: &AppHandle, op: impl FnOnce(&mut Layout) -> Result<()>) -> Result<State> {
    edit(app, |store| op(&mut store.current)).map(|(state, ())| state)
}

#[tauri::command]
pub fn vj_layout_get(app: AppHandle) -> Result<State> {
    on_current(&app, |_| Ok(()))
}

#[tauri::command]
pub fn vj_layout_move(app: AppHandle, panel: String, target: Target) -> Result<State> {
    on_current(&app, |layout| layout.move_panel(&panel, &target))
}

#[tauri::command]
pub fn vj_layout_reorder(app: AppHandle, group: String, from: usize, to: usize) -> Result<State> {
    on_current(&app, |layout| layout.reorder_tab(&group, from, to))
}

#[tauri::command]
pub fn vj_layout_activate(app: AppHandle, group: String, index: usize) -> Result<State> {
    on_current(&app, |layout| layout.activate(&group, index))
}

#[tauri::command]
pub fn vj_layout_close(app: AppHandle, panel: String) -> Result<State> {
    on_current(&app, |layout| layout.close_panel(&panel))
}

/// Opens a closed fixed panel, or a new monitor when `panel` is `"monitor"` (with its `monitor`
/// kind, preview by default).
#[tauri::command]
pub fn vj_layout_open(
    app: AppHandle,
    panel: String,
    monitor: Option<MonitorKind>,
) -> Result<State> {
    on_current(&app, |layout| layout.open_panel(&panel, monitor).map(drop))
}

#[tauri::command]
pub fn vj_layout_resize(app: AppHandle, split: String, sizes: Vec<f32>) -> Result<State> {
    on_current(&app, |layout| layout.resize(&split, sizes))
}

#[tauri::command]
pub fn vj_layout_set_monitor(app: AppHandle, id: String, kind: MonitorKind) -> Result<State> {
    on_current(&app, |layout| layout.set_monitor(&id, kind))
}

/// Makes a built-in (`default`, `performance`, `monitors`) or saved layout the current one.
#[tauri::command]
pub fn vj_layout_apply(app: AppHandle, id: String) -> Result<State> {
    edit(&app, |store| store.apply(&id)).map(|(state, ())| state)
}

/// Keeps the current layout under `name`; the same name is overwritten.
#[tauri::command]
pub fn vj_layout_save(app: AppHandle, name: String) -> Result<State> {
    edit(&app, |store| store.save_as(&name)).map(|(state, ())| state)
}

#[tauri::command]
pub fn vj_layout_rename(app: AppHandle, id: String, name: String) -> Result<State> {
    edit(&app, |store| store.rename(&id, &name)).map(|(state, ())| state)
}

#[tauri::command]
pub fn vj_layout_delete(app: AppHandle, id: String) -> Result<State> {
    edit(&app, |store| store.delete(&id)).map(|(state, ())| state)
}

/// Puts the default layout back as the current one.
#[tauri::command]
pub fn vj_layout_reset(app: AppHandle) -> Result<State> {
    edit(&app, |store| {
        store.reset();
        Ok(())
    })
    .map(|(state, ())| state)
}
