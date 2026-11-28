//! The video output: a window for each screen of the project, on the display the setup chose for
//! it, drawn by the GPU from the engine position. It is black while the transport is stopped or no
//! clip shows. A VJ project shows its grid instead: the clips launched by hand, on the grid's own
//! clock.
//!
//! Each window has a render thread that owns its GPU surface; every window shares one
//! [`VideoDeck`], so a clip is decoded once however many screens show it. Each refresh a render
//! thread reads where the transport is, asks the deck for the pictures at that frame and draws the
//! top one; the decoding runs on the deck's own threads. Nothing here logs from the render loop: failures are counted and
//! the status command reports them.

use crate::commands::SharedSession;
use crate::preferences::{self, Change};
use crate::transport_commands::SharedOutput;
use flux_engine::output::PositionReader;
use flux_engine::transport::TransportState;
use flux_project::Timeline;
use flux_render::pattern::{CardSlice, Logo};
use flux_render::{Mask, Picture, Quad, Screen};
use flux_video::{Cue, VideoDeck};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Manager, Monitor, State, Window, WindowEvent};

pub(crate) mod card;
mod grid;
pub(crate) mod menu;
pub(crate) mod outputs;
mod preview;
mod render;
mod window;

use self::card::*;
use self::grid::*;
pub(crate) use self::grid::{GridSources, composition_size, grid_frame, items};
pub use self::outputs::{restore, window_gone};
pub(crate) use self::preview::ensure_deck;
use self::preview::*;
use self::render::*;

/// Every output window's label starts with it; the screen's id follows.
const LABEL_PREFIX: &str = "video-output-";

/// A display the output can open on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// How the preference names it.
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// A window of FLUX is on it: the output opens there as a window, not full screen.
    pub main: bool,
}

/// What the setup page shows about the output.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOutputStatus {
    pub displays: Vec<Display>,
    /// The screens of the project, the main one first.
    pub screens: Vec<ScreenStatus>,
    /// Where the open clips are decoded: `cpu`, or the GPU device's name (`d3d11va`,
    /// `videotoolbox`, `vaapi`...); `None` with no clip open.
    pub decoding: Option<String>,
}

/// What the setup page shows about one screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStatus {
    pub id: String,
    pub name: String,
    /// The display chosen in the preferences.
    pub chosen: Option<String>,
    /// The window is open and drawing.
    pub open: bool,
    /// The chosen display is not connected.
    pub missing: bool,
    /// Why the window could not draw, when it could not.
    pub error: Option<String>,
}

struct Live {
    window: Window,
    display: String,
    name: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The last error the GPU reported while drawing.
    gpu_errors: flux_render::Errors,
    /// The window covers its display.
    fullscreen: bool,
}

/// The window of one screen.
struct VideoOutput {
    screen: String,
    live: Mutex<Option<Live>>,
    error: Mutex<Option<String>>,
    pattern: Arc<AtomicBool>,
    controls: Arc<Controls>,
    /// `Some(true)` puts the window full screen even on the editor's display, `Some(false)` makes
    /// it a window even on another; absent, it is full screen anywhere but the editor's display.
    fullscreen: Mutex<Option<bool>>,
    deck: SharedDeck,
}

/// What Output > Disable Output and Show Test Card set, for every screen at once.
#[derive(Default)]
struct Controls {
    /// Every screen is black.
    disabled: AtomicBool,
    /// Every screen shows the test card instead of the picture.
    card: AtomicBool,
}

/// The one deck every screen's window draws from, so a clip is decoded once however many screens
/// show it. Made when the first window opens.
pub(crate) type SharedDeck = Arc<Mutex<Option<VideoDeck>>>;

/// The windows of every screen, by screen id.
#[derive(Default)]
pub struct VideoOutputs {
    outputs: Mutex<BTreeMap<String, Arc<VideoOutput>>>,
    /// A test pattern is shown where there is no picture, to line the slices up with.
    pattern: Arc<AtomicBool>,
    controls: Arc<Controls>,
    deck: SharedDeck,
    /// The app is quitting: no window opens again.
    closing: AtomicBool,
    /// One sync at a time, so two never open the same window.
    syncing: Mutex<()>,
    /// What the editor's monitor remembers of its last frame, for the composition's feedback.
    monitor: Mutex<MonitorFrames>,
    /// The editor's preview of the selected clip: its own clock, playhead and decoders.
    preview: Arc<Mutex<Preview>>,
}

/// The editor's monitor draws on the CPU, and keeps its own history of the frames before.
#[derive(Default)]
struct MonitorFrames {
    history: flux_render::History,
    last: Option<Instant>,
    /// The grid copied from the session, the revision it was copied at, and its modulated copy.
    grid: Option<(u64, String, Option<flux_project::vj::Vj>)>,
    drawn: Option<flux_project::vj::Vj>,
    /// The last picture drawn per scope (see [`scope_key`]): when, from which revision, and it.
    pictures: HashMap<String, (Instant, u64, Vec<u8>)>,
    /// The size of the display each screen is set to, by screen id, and when it was looked up.
    display_sizes: HashMap<String, (Instant, Option<(u32, u32)>)>,
}

/// How long a screen's chosen display size is kept before it is looked up again.
const DISPLAY_SIZE_KEPT: Duration = Duration::from_secs(1);

/// How long a drawn monitor picture is shared by the monitors asking for it. It is just under the
/// monitors' ~33 ms poll (`OUTPUT_EVERY` in `src/vj/store.ts`): each monitor's timer has its own
/// phase, so a shorter window would let two monitors both draw in one period.
const FRESH: Duration = Duration::from_millis(30);

/// Whether a picture drawn `age` ago at `drawn_at` is still the one to show at `revision`.
fn picture_is_fresh(age: Duration, drawn_at: u64, revision: u64) -> bool {
    age < FRESH && drawn_at == revision
}

fn scope_key(scope: Option<&Scope>) -> String {
    match scope {
        None => String::new(),
        Some(Scope::Layer(id)) => format!("l:{id}"),
        Some(Scope::Group(id)) => format!("g:{id}"),
    }
}

/// What a monitor of one layer or one group of the grid shows, by id.
pub enum Scope {
    Layer(String),
    Group(String),
}

pub type SharedVideoOutput = Arc<VideoOutputs>;

fn display_id(monitor: &Monitor) -> String {
    monitor.name().cloned().unwrap_or_else(|| {
        let at = monitor.position();
        format!("{},{}", at.x, at.y)
    })
}

fn displays(app: &AppHandle) -> Vec<(Display, Monitor)> {
    // The displays FLUX's own windows (the editor, the welcome window) are on.
    let main: Vec<String> = app
        .webview_windows()
        .values()
        .filter_map(|window| window.current_monitor().ok().flatten())
        .map(|monitor| display_id(&monitor))
        .collect();
    app.available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| {
            let id = display_id(&monitor);
            let display = Display {
                main: main.contains(&id),
                width: monitor.size().width,
                height: monitor.size().height,
                id,
            };
            (display, monitor)
        })
        .collect()
}

/// Whether the display named `id` is connected.
pub fn connected(app: &AppHandle, id: &str) -> bool {
    displays(app).iter().any(|(display, _)| display.id == id)
}

#[cfg(test)]
mod tests;
