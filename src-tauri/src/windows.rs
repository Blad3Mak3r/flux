//! The windows Flux goes through: the launcher, the welcome window and the editor.
//!
//! Only the launcher is created from `tauri.conf.json`; the others are declared there with
//! `create: false` and built here, one after the other. The next window is always created before
//! the previous one closes, so the app never has zero windows and never exits in between.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

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

/// Replaces the launcher with the welcome window.
#[tauri::command]
pub async fn window_show_welcome(app: AppHandle) -> Result<(), String> {
    build(&app, WELCOME, "index.html?view=welcome")?;
    close(&app, LAUNCHER);
    Ok(())
}

/// Replaces the welcome window with the editor, which opens `path` or starts a new show.
#[tauri::command]
pub async fn window_open_editor(app: AppHandle, path: Option<PathBuf>) -> Result<(), String> {
    let url = match path {
        Some(path) => format!("index.html?open={}", encode_component(&path)),
        None => "index.html".to_owned(),
    };
    build(&app, EDITOR, &url)?;
    close(&app, WELCOME);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_component_should_escape_what_a_query_string_cannot_carry() {
        let path = Path::new("/home/ana/My Show & Co/día #1.fxtc");

        assert_eq!(
            encode_component(path),
            "%2Fhome%2Fana%2FMy%20Show%20%26%20Co%2Fd%C3%ADa%20%231.fxtc"
        );
    }
}
