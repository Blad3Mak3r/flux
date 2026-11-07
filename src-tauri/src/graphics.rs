//! Hardware acceleration and graphics API preferences of the app's webview.
//!
//! Flux draws nothing with a graphics API of its own: the system's webview does (WebView2 on
//! Windows, WebKitGTK on Linux, WKWebView on macOS). What can be chosen is how that engine starts,
//! and it is fixed when the first window is created, so a change needs a restart. The choice is the
//! app's, not the project's, and lives in `graphics.json` in the app data folder. A file that is
//! missing or unreadable means the defaults, never an error: a show must not fail to start because
//! of it. `--safe-graphics` (or `FLUX_SAFE_GRAPHICS=1`) ignores the file for one run, for a driver
//! that leaves the window blank.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

const FILE_NAME: &str = "graphics.json";
/// The app's identifier (`tauri.conf.json`), which names its folder in the system's app data.
const IDENTIFIER: &str = "com.fluxtimecode.app";

/// The graphics API the webview draws through, on the systems that let one be chosen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphicsApi {
    /// The system's choice.
    #[default]
    Auto,
    /// Direct3D 11.
    D3d11,
    /// Direct3D 12 (through Direct3D 11 on 12).
    D3d12,
    OpenGl,
    /// Experimental, and only as good as the driver.
    Vulkan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphicsPrefs {
    /// Off draws on the processor, which is slower but works with any driver.
    pub hardware_acceleration: bool,
    pub api: GraphicsApi,
}

impl Default for GraphicsPrefs {
    fn default() -> Self {
        Self {
            hardware_acceleration: true,
            api: GraphicsApi::Auto,
        }
    }
}

/// What the system's webview lets the app choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    /// Whether acceleration can be switched off. macOS always draws with Metal.
    pub fn acceleration_configurable(self) -> bool {
        self != Self::Other
    }

    /// The APIs that can be picked. Only WebView2 takes one; the rest follow the system.
    pub fn apis(self) -> &'static [GraphicsApi] {
        match self {
            Self::Windows => &[
                GraphicsApi::Auto,
                GraphicsApi::D3d11,
                GraphicsApi::D3d12,
                GraphicsApi::OpenGl,
                GraphicsApi::Vulkan,
            ],
            Self::Linux | Self::Other => &[GraphicsApi::Auto],
        }
    }
}

/// `prefs` as this system can honour them: what it cannot choose goes back to the default.
pub fn normalize(prefs: GraphicsPrefs, platform: Platform) -> GraphicsPrefs {
    GraphicsPrefs {
        hardware_acceleration: prefs.hardware_acceleration || !platform.acceleration_configurable(),
        api: if platform.apis().contains(&prefs.api) {
            prefs.api
        } else {
            GraphicsApi::Auto
        },
    }
}

/// The arguments WebView2 (Chromium) is started with.
fn webview2_arguments(prefs: GraphicsPrefs) -> String {
    let mut args = Vec::new();
    if !prefs.hardware_acceleration {
        args.push("--disable-gpu");
    } else if let Some(angle) = match prefs.api {
        GraphicsApi::Auto => None,
        GraphicsApi::D3d11 => Some("--use-angle=d3d11"),
        GraphicsApi::D3d12 => Some("--use-angle=d3d11on12"),
        GraphicsApi::OpenGl => Some("--use-angle=gl"),
        GraphicsApi::Vulkan => Some("--use-angle=vulkan"),
    } {
        args.push(angle);
    }
    args.join(" ")
}

/// The environment variables that set `prefs` up for `platform`, as `(name, value)`.
pub fn environment(prefs: GraphicsPrefs, platform: Platform) -> Vec<(&'static str, String)> {
    match platform {
        Platform::Windows => {
            let args = webview2_arguments(prefs);
            if args.is_empty() {
                Vec::new()
            } else {
                vec![("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", args)]
            }
        }
        Platform::Linux if !prefs.hardware_acceleration => vec![
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1".to_owned()),
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1".to_owned()),
            ("LIBGL_ALWAYS_SOFTWARE", "1".to_owned()),
        ],
        _ => Vec::new(),
    }
}

/// What to set `name` to given what it is already, or `None` to leave it. Arguments for WebView2
/// are added to those a user already set; any other variable the user set is theirs.
fn merged(existing: Option<&str>, name: &str, value: &str) -> Option<String> {
    match existing.filter(|existing| !existing.trim().is_empty()) {
        None => Some(value.to_owned()),
        Some(existing) if name == "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS" => {
            Some(format!("{existing} {value}"))
        }
        Some(_) => None,
    }
}

/// Where the preferences are kept. It is computed from the system's app data folder and the app's
/// identifier, because it is needed before Tauri has an `AppHandle`.
fn prefs_file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(IDENTIFIER).join(FILE_NAME))
}

/// The preferences in `file`, or the defaults when there are none or they cannot be read.
fn load(file: &Path) -> GraphicsPrefs {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes through a temporary file, so a failed write leaves the old preferences as they were.
fn save(file: &Path, prefs: &GraphicsPrefs) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let tmp = file.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, file).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Whether this run ignores the saved preferences.
fn safe_mode(args: impl IntoIterator<Item = OsString>, env: Option<OsString>) -> bool {
    env.is_some_and(|value| !value.is_empty() && value != "0")
        || args.into_iter().any(|arg| arg == "--safe-graphics")
}

/// What this run started the webview with.
#[derive(Debug, Clone, Copy)]
struct Effective {
    prefs: GraphicsPrefs,
    safe_mode: bool,
}

static EFFECTIVE: OnceLock<Effective> = OnceLock::new();

/// Sets the webview up from the saved preferences. Call it first thing in `run`, before any thread
/// or window exists: the environment can only be changed safely then, and the webview reads it
/// once, when it is created.
pub fn apply_early() {
    let platform = Platform::current();
    let safe = safe_mode(
        std::env::args_os().skip(1),
        std::env::var_os("FLUX_SAFE_GRAPHICS"),
    );
    let saved = if safe {
        GraphicsPrefs::default()
    } else {
        prefs_file().map_or_else(GraphicsPrefs::default, |file| load(&file))
    };
    let prefs = normalize(saved, platform);
    for (name, value) in environment(prefs, platform) {
        let existing = std::env::var(name).ok();
        if let Some(next) = merged(existing.as_deref(), name, &value) {
            // SAFETY: called first thing in `run`, before any thread exists.
            unsafe { std::env::set_var(name, next) };
        }
    }
    let _ = EFFECTIVE.set(Effective {
        prefs,
        safe_mode: safe,
    });
}

/// One line for the log: what the webview was started with.
pub fn describe() -> String {
    let effective = EFFECTIVE.get().copied().unwrap_or(Effective {
        prefs: GraphicsPrefs::default(),
        safe_mode: false,
    });
    format!(
        "graphics: acceleration {}, api {:?}{}",
        if effective.prefs.hardware_acceleration {
            "on"
        } else {
            "off"
        },
        effective.prefs.api,
        if effective.safe_mode {
            " (safe mode)"
        } else {
            ""
        }
    )
}

/// The preferences as the window shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsState {
    /// What is saved, which starts the webview the next time.
    pub saved: GraphicsPrefs,
    /// What this run started it with.
    pub effective: GraphicsPrefs,
    /// This run ignored the saved preferences.
    pub safe_mode: bool,
    pub acceleration_configurable: bool,
    pub apis: Vec<GraphicsApi>,
    /// Saved and running differ: a restart is needed to see the change.
    pub restart_needed: bool,
}

fn state_for(saved: GraphicsPrefs, effective: Effective, platform: Platform) -> GraphicsState {
    let saved = normalize(saved, platform);
    GraphicsState {
        saved,
        effective: effective.prefs,
        safe_mode: effective.safe_mode,
        acceleration_configurable: platform.acceleration_configurable(),
        apis: platform.apis().to_vec(),
        restart_needed: saved != effective.prefs,
    }
}

pub fn state() -> GraphicsState {
    let platform = Platform::current();
    let effective = EFFECTIVE.get().copied().unwrap_or(Effective {
        prefs: GraphicsPrefs::default(),
        safe_mode: false,
    });
    let saved = prefs_file().map_or_else(GraphicsPrefs::default, |file| load(&file));
    state_for(saved, effective, platform)
}

#[tauri::command]
pub fn graphics_get() -> GraphicsState {
    state()
}

/// Saves the preferences. They take effect when the app is restarted.
#[tauri::command]
pub fn graphics_set(prefs: GraphicsPrefs) -> Result<GraphicsState, String> {
    let file = prefs_file().ok_or("the app data folder is not known")?;
    save(&file, &normalize(prefs, Platform::current())).map_err(|err| err.to_string())?;
    log::info!("graphics: preferences saved, they apply after a restart");
    Ok(state())
}

/// Restarts the app, so the webview starts with the saved preferences.
#[tauri::command]
pub async fn graphics_restart(
    app: AppHandle,
    session: tauri::State<'_, crate::commands::SharedSession>,
    store: tauri::State<'_, crate::recovery::SharedRecoveries>,
) -> Result<(), String> {
    crate::recovery::transition(&session, &store, |s| {
        s.clear();
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?;
    log::info!("graphics: restarting to apply the preferences");
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(hardware_acceleration: bool, api: GraphicsApi) -> GraphicsPrefs {
        GraphicsPrefs {
            hardware_acceleration,
            api,
        }
    }

    #[test]
    fn defaults_should_be_accelerated_and_automatic() {
        assert_eq!(GraphicsPrefs::default(), prefs(true, GraphicsApi::Auto));
        assert!(environment(GraphicsPrefs::default(), Platform::Windows).is_empty());
        assert!(environment(GraphicsPrefs::default(), Platform::Linux).is_empty());
    }

    #[test]
    fn windows_should_pass_the_api_to_webview2() {
        let args = |api| environment(prefs(true, api), Platform::Windows);
        assert!(args(GraphicsApi::Auto).is_empty());
        let one = |api| args(api).remove(0).1;
        assert_eq!(one(GraphicsApi::D3d11), "--use-angle=d3d11");
        assert_eq!(one(GraphicsApi::D3d12), "--use-angle=d3d11on12");
        assert_eq!(one(GraphicsApi::OpenGl), "--use-angle=gl");
        assert_eq!(one(GraphicsApi::Vulkan), "--use-angle=vulkan");
        assert_eq!(
            args(GraphicsApi::D3d11)[0].0,
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"
        );
    }

    #[test]
    fn windows_should_turn_the_gpu_off_and_ignore_the_api_then() {
        let set = environment(prefs(false, GraphicsApi::Vulkan), Platform::Windows);
        assert_eq!(
            set,
            [(
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                "--disable-gpu".to_owned()
            )]
        );
    }

    #[test]
    fn linux_should_only_switch_acceleration_off() {
        let off = environment(prefs(false, GraphicsApi::Auto), Platform::Linux);
        let names: Vec<_> = off.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "WEBKIT_DISABLE_COMPOSITING_MODE",
                "WEBKIT_DISABLE_DMABUF_RENDERER",
                "LIBGL_ALWAYS_SOFTWARE"
            ]
        );
        assert!(environment(prefs(true, GraphicsApi::Vulkan), Platform::Linux).is_empty());
    }

    #[test]
    fn macos_should_set_nothing() {
        assert!(environment(prefs(false, GraphicsApi::D3d11), Platform::Other).is_empty());
    }

    #[test]
    fn normalize_should_drop_what_the_system_cannot_choose() {
        assert_eq!(
            normalize(prefs(false, GraphicsApi::Vulkan), Platform::Linux),
            prefs(false, GraphicsApi::Auto)
        );
        assert_eq!(
            normalize(prefs(false, GraphicsApi::D3d11), Platform::Other),
            prefs(true, GraphicsApi::Auto)
        );
        assert_eq!(
            normalize(prefs(true, GraphicsApi::Vulkan), Platform::Windows),
            prefs(true, GraphicsApi::Vulkan)
        );
    }

    #[test]
    fn webview2_arguments_should_add_to_a_users_and_other_variables_should_be_left_alone() {
        let name = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
        assert_eq!(
            merged(None, name, "--disable-gpu"),
            Some("--disable-gpu".into())
        );
        assert_eq!(
            merged(Some("  "), name, "--disable-gpu"),
            Some("--disable-gpu".into())
        );
        assert_eq!(
            merged(Some("--foo"), name, "--disable-gpu"),
            Some("--foo --disable-gpu".into())
        );
        assert_eq!(merged(None, "LIBGL_ALWAYS_SOFTWARE", "1"), Some("1".into()));
        assert_eq!(merged(Some("0"), "LIBGL_ALWAYS_SOFTWARE", "1"), None);
    }

    #[test]
    fn preferences_should_round_trip_in_camel_case_and_fill_what_is_missing() {
        let json = serde_json::to_value(prefs(false, GraphicsApi::D3d12)).unwrap();
        assert_eq!(json["hardwareAcceleration"], false);
        assert_eq!(json["api"], "d3d12");
        assert_eq!(
            serde_json::from_value::<GraphicsPrefs>(json).unwrap(),
            prefs(false, GraphicsApi::D3d12)
        );
        let partial: GraphicsPrefs = serde_json::from_str(r#"{"api":"openGl"}"#).unwrap();
        assert_eq!(partial, prefs(true, GraphicsApi::OpenGl));
    }

    #[test]
    fn load_should_give_the_defaults_for_a_missing_or_unreadable_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&dir.path().join("nothing.json")),
            GraphicsPrefs::default()
        );
        let broken = dir.path().join("broken.json");
        fs::write(&broken, "{ not json").unwrap();
        assert_eq!(load(&broken), GraphicsPrefs::default());
        let unknown = dir.path().join("unknown.json");
        fs::write(&unknown, r#"{"api":"metal"}"#).unwrap();
        assert_eq!(load(&unknown), GraphicsPrefs::default());
    }

    #[test]
    fn save_should_write_what_load_reads_and_create_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("app").join(FILE_NAME);
        save(&file, &prefs(false, GraphicsApi::OpenGl)).unwrap();
        assert_eq!(load(&file), prefs(false, GraphicsApi::OpenGl));
        assert!(!file.with_extension("json.tmp").exists());
    }

    #[test]
    fn safe_mode_should_come_from_the_flag_or_the_variable() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(!safe_mode(args(&["show.fxtc"]), None));
        assert!(safe_mode(args(&["--safe-graphics"]), None));
        assert!(safe_mode(args(&[]), Some("1".into())));
        assert!(!safe_mode(args(&[]), Some("0".into())));
        assert!(!safe_mode(args(&[]), Some("".into())));
    }

    #[test]
    fn state_should_ask_for_a_restart_only_when_saved_and_running_differ() {
        let running = Effective {
            prefs: prefs(true, GraphicsApi::Auto),
            safe_mode: false,
        };
        assert!(
            !state_for(prefs(true, GraphicsApi::Auto), running, Platform::Windows).restart_needed
        );
        assert!(
            state_for(prefs(false, GraphicsApi::Auto), running, Platform::Windows).restart_needed
        );
        // What the system cannot choose is not a difference.
        assert!(
            !state_for(prefs(true, GraphicsApi::Vulkan), running, Platform::Linux).restart_needed
        );
    }

    #[test]
    fn prefs_file_should_be_in_a_folder_named_after_the_app() {
        if let Some(file) = prefs_file() {
            assert!(file.ends_with(Path::new(IDENTIFIER).join(FILE_NAME)));
        }
    }
}
