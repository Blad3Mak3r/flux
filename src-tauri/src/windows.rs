//! The windows Flux goes through: the launcher, the welcome window and the editor.
//!
//! Only the launcher is created from `tauri.conf.json`; the others are declared there with
//! `create: false` and built here, one after the other. The next window is always created before
//! the previous one closes, so the app never has zero windows and never exits in between.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::commands::SharedSession;
use crate::recovery::{self, SharedRecoveries};
use crate::transport_commands::{SharedOutput, release};

const LAUNCHER: &str = "launcher";
const WELCOME: &str = "welcome";
const EDITOR: &str = "main";

fn build(app: &AppHandle, label: &str, url: &str) -> Result<WebviewWindow, String> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == label)
        .cloned()
        .ok_or_else(|| format!("no window `{label}` in tauri.conf.json"))?;
    config.url = WebviewUrl::App(url.into());
    WebviewWindowBuilder::from_config(app, &config)
        .and_then(WebviewWindowBuilder::build)
        .map_err(|err| err.to_string())
}

fn close(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

/// Percent-encodes everything but unreserved characters, for a path in a query string.
fn encode_component(path: &Path) -> String {
    use std::fmt::Write;

    path.to_string_lossy()
        .bytes()
        .fold(String::new(), |mut out, byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                out.push(char::from(byte));
            } else {
                let _ = write!(out, "%{byte:02X}");
            }
            out
        })
}

/// The project file the OS asked FLUX to open, kept until the launcher is done.
#[derive(Default)]
pub struct PendingOpen(Mutex<Option<PathBuf>>);

impl PendingOpen {
    /// Reads the file from the command line, where Linux and Windows pass it.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Self {
        Self(Mutex::new(project_arg(args)))
    }

    /// macOS hands the file over in an event instead of the command line.
    #[cfg(target_os = "macos")]
    pub fn set(&self, path: PathBuf) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(path);
    }

    fn take(&self) -> Option<PathBuf> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    fn path(&self) -> Option<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[tauri::command]
pub fn window_pending_path(pending: State<'_, PendingOpen>) -> Option<PathBuf> {
    pending.path()
}

/// The first argument that is an existing `.flx` or legacy `.fxtc` file.
fn project_arg(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter()
        .map(PathBuf::from)
        .find(|path| flux_project::is_project_path(path) && path.is_file())
}

fn editor_url(path: Option<&Path>) -> String {
    match path {
        Some(path) => format!("index.html?open={}", encode_component(path)),
        None => "index.html".to_owned(),
    }
}

/// Replaces the launcher with the welcome window, or straight with the editor when the OS asked
/// Flux to open a file.
#[tauri::command]
pub async fn window_show_welcome(
    app: AppHandle,
    pending: State<'_, PendingOpen>,
    store: State<'_, SharedRecoveries>,
) -> Result<(), String> {
    let dir = store.dir.clone();
    let recoveries =
        tauri::async_runtime::spawn_blocking(move || flux_project::recovery::list(&dir))
            .await
            .map_err(|err| err.to_string())?;
    // Even an unreadable record is offered for discard. Failure to read the directory is nonfatal.
    if recoveries.is_ok_and(|entries| !entries.is_empty()) {
        build(&app, WELCOME, "index.html?view=welcome")?;
        close(&app, LAUNCHER);
        return Ok(());
    }
    match pending.path() {
        Some(path) => build(&app, EDITOR, &editor_url(Some(&path)))?,
        None => build(&app, WELCOME, "index.html?view=welcome")?,
    };
    pending.take();
    close(&app, LAUNCHER);
    Ok(())
}

/// Replaces the welcome window with the editor, which opens `path` or starts a new show.
#[tauri::command]
pub async fn window_open_editor(
    app: AppHandle,
    session: State<'_, SharedSession>,
    output: State<'_, SharedOutput>,
    pending: State<'_, PendingOpen>,
    store: State<'_, SharedRecoveries>,
    path: Option<PathBuf>,
    use_pending: Option<bool>,
) -> Result<(), String> {
    if app.get_webview_window(EDITOR).is_some() {
        return Err("the editor is already open".into());
    }
    // Only a reload of the same editor may restore its document. A new window must honor `path`.
    recovery::transition(&session, &store, |s| {
        s.clear();
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?;
    release(&output).await;
    let requested = pending.path();
    let path = if use_pending == Some(true) {
        requested
    } else {
        path
    };
    build(&app, EDITOR, &editor_url(path.as_deref()))?;
    pending.take();
    close(&app, WELCOME);
    Ok(())
}

/// Replaces the editor with the welcome window after its frontend has resolved unsaved changes.
#[tauri::command]
pub async fn window_close_editor(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
) -> Result<(), String> {
    build(&app, WELCOME, "index.html?view=welcome")?;
    recovery::transition(&session, &store, |s| {
        s.clear();
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?;
    close(&app, EDITOR);
    Ok(())
}

#[tauri::command]
pub async fn window_restore_recovery(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    pending: State<'_, PendingOpen>,
    id: String,
) -> Result<(), String> {
    if app.get_webview_window(EDITOR).is_some() {
        return Err("the editor is already open".into());
    }
    recovery::restore(&session, &store, id)
        .await
        .map_err(|err| err.to_string())?;
    if let Err(err) = build(&app, EDITOR, &editor_url(None)) {
        recovery::abandon(&session, &store)
            .await
            .map_err(|error| error.to_string())?;
        return Err(err);
    }
    pending.take();
    close(&app, WELCOME);
    Ok(())
}

#[tauri::command]
pub async fn window_open_original(
    app: AppHandle,
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
    pending: State<'_, PendingOpen>,
    id: String,
) -> Result<(), String> {
    if app.get_webview_window(EDITOR).is_some() {
        return Err("the editor is already open".into());
    }
    let dir = store.dir.clone();
    let record_id = id.clone();
    let (original, _lease) = tauri::async_runtime::spawn_blocking(move || {
        let lease = flux_project::recovery::claim(&dir, &record_id)?;
        let original = flux_project::recovery::read(&dir, &record_id)?
            .document
            .path
            .ok_or_else(|| {
                flux_project::ProjectError::Invalid("the project has never been saved".into())
            })?;
        Ok::<_, flux_project::ProjectError>((original, lease))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())?;
    let recent = original.clone();
    recovery::transition(&session, &store, move |s| s.open(&original))
        .await
        .map_err(|err| err.to_string())?;
    build(&app, EDITOR, &editor_url(None))?;
    crate::recents::remember(&app, &recent);
    let dir = store.dir.clone();
    tauri::async_runtime::spawn_blocking(move || flux_project::recovery::remove(&dir, &id))
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    pending.take();
    close(&app, WELCOME);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_arg_should_pick_an_existing_project_of_either_extension() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("folder.flx");
        std::fs::create_dir(&folder).unwrap();
        for name in ["Show.flx", "Show.FLX", "Show.fxtc", "Show.FXTC"] {
            let show = dir.path().join(name);
            std::fs::write(&show, b"").unwrap();
            let args = [
                "--flag",
                "notes.txt",
                "missing.flx",
                "missing.fxtc",
                folder.to_str().unwrap(),
                show.to_str().unwrap(),
            ]
            .map(OsString::from);

            assert_eq!(project_arg(args), Some(show));
        }
    }

    #[test]
    fn encode_component_should_escape_what_a_query_string_cannot_carry() {
        let path = Path::new("/home/ana/My Show & Co/día #1.fxtc");

        assert_eq!(
            encode_component(path),
            "%2Fhome%2Fana%2FMy%20Show%20%26%20Co%2Fd%C3%ADa%20%231.fxtc"
        );
    }
}
