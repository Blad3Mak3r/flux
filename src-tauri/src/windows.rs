//! The windows Flux goes through: the launcher, the welcome window and the editor.
//!
//! Only the launcher is created from `tauri.conf.json`; the others are declared there with
//! `create: false` and built here, one after the other. The next window is always created before
//! the previous one closes, so the app never has zero windows and never exits in between.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

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

/// The `.fxtc` file the OS asked Flux to open, kept until the launcher is done.
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
}

/// The first argument that is an existing `.fxtc` file.
fn project_arg(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    args.into_iter().map(PathBuf::from).find(|path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("fxtc"))
            && path.is_file()
    })
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
) -> Result<(), String> {
    match pending.take() {
        Some(path) => build(&app, EDITOR, &editor_url(Some(&path)))?,
        None => build(&app, WELCOME, "index.html?view=welcome")?,
    };
    close(&app, LAUNCHER);
    Ok(())
}

/// Replaces the welcome window with the editor, which opens `path` or starts a new show.
#[tauri::command]
pub async fn window_open_editor(app: AppHandle, path: Option<PathBuf>) -> Result<(), String> {
    build(&app, EDITOR, &editor_url(path.as_deref()))?;
    close(&app, WELCOME);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_arg_should_pick_the_existing_fxtc_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let show = dir.path().join("Show.FXTC");
        std::fs::write(&show, b"").unwrap();
        let args = [
            "--flag",
            "notes.txt",
            "missing.fxtc",
            show.to_str().unwrap(),
        ]
        .map(OsString::from);

        assert_eq!(project_arg(args), Some(show));
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
