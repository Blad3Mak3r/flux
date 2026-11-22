//! The video output: a window on the display chosen in the setup, drawn by the GPU from the engine
//! position. It is black while the transport is stopped or no clip shows.
//!
//! A render thread owns the GPU surface and a [`VideoDeck`]. Each refresh it reads where the
//! transport is, asks the deck for the pictures at that frame and draws the top one; the decoding
//! runs on the deck's own threads. Nothing here logs from the render loop: failures are counted and
//! the status command reports them.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager, Monitor, State, Window, WindowEvent};

use crate::commands::SharedSession;
use crate::preferences::{self, Change};
use crate::transport_commands::SharedOutput;
use flux_engine::output::PositionReader;
use flux_engine::transport::TransportState;
use flux_project::Timeline;
use flux_render::{Picture, Screen};
use flux_video::{Cue, VideoDeck};

const LABEL: &str = "video-output";

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
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The last error the GPU reported while drawing.
    gpu_errors: flux_render::Errors,
}

#[derive(Default)]
pub struct VideoOutput {
    live: Mutex<Option<Live>>,
    error: Mutex<Option<String>>,
}

pub type SharedVideoOutput = Arc<VideoOutput>;

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

impl VideoOutput {
    fn close(&self) {
        let live = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut live) = live {
            live.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = live.thread.take() {
                let _ = thread.join();
            }
            let _ = live.window.destroy();
        }
    }

    /// Opens the window on the chosen display, or closes it when there is none or it is not
    /// connected. Does nothing when it is already open there.
    pub fn apply(self: &Arc<Self>, app: &AppHandle, chosen: Option<&str>) {
        let open_on = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|live| live.display.clone());
        if open_on.as_deref() == chosen && chosen.is_some() {
            return;
        }
        self.close();
        let Some(chosen) = chosen else {
            *self.error.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return;
        };
        let Some((display, monitor)) = displays(app).into_iter().find(|(d, _)| d.id == chosen)
        else {
            log::warn!("video output: display {chosen} is not connected");
            return;
        };
        let result = self.open(app, &display, &monitor);
        if let Err(err) = &result {
            log::warn!("video output could not open on {chosen}: {err}");
        } else {
            log::info!("video output open on {chosen}");
        }
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = result.err();
    }

    fn open(
        self: &Arc<Self>,
        app: &AppHandle,
        display: &Display,
        monitor: &Monitor,
    ) -> Result<(), String> {
        let scale = monitor.scale_factor();
        let at = monitor.position();
        let (x, y) = (f64::from(at.x) / scale, f64::from(at.y) / scale);
        // On the editor's own display it is a window that can be moved aside, so a single screen
        // is not covered; on any other it fills the display.
        let builder = Window::builder(app, LABEL)
            .title("FLUX video")
            .visible(true)
            .focused(false);
        let window = if display.main {
            builder
                .inner_size(960.0, 540.0)
                .position(x + 80.0, y + 80.0)
                .build()
        } else {
            builder
                .decorations(false)
                .skip_taskbar(true)
                .position(x, y)
                .inner_size(
                    f64::from(display.width) / scale,
                    f64::from(display.height) / scale,
                )
                .fullscreen(true)
                .build()
        }
        .map_err(|err| err.to_string())?;

        let size = Arc::new(AtomicU64::new(pack(
            window.inner_size().unwrap_or_default(),
        )));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (size, stop, output) = (Arc::clone(&size), Arc::clone(&stop), Arc::clone(self));
            window.on_window_event(move |event| match event {
                WindowEvent::Resized(new) => size.store(pack(*new), Ordering::Relaxed),
                WindowEvent::Destroyed => {
                    stop.store(true, Ordering::Relaxed);
                    // Closed by the user: it stays closed until the display is chosen again.
                    if let Ok(mut live) = output.live.try_lock()
                        && live
                            .as_ref()
                            .is_some_and(|l| l.stop.load(Ordering::Relaxed))
                    {
                        *live = None;
                    }
                }
                _ => {}
            });
        }

        // The GPU surface is made on the main thread, as macOS requires; the device is started on
        // the render thread, which answers once it can draw.
        let (target_tx, target_rx) = std::sync::mpsc::channel();
        let target = window.clone();
        app.run_on_main_thread(move || {
            let _ = target_tx.send(Screen::target(target));
        })
        .map_err(|err| err.to_string())?;
        let target = match target_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(target)) => target,
            Ok(Err(err)) => {
                let _ = window.destroy();
                return Err(err.to_string());
            }
            Err(_) => {
                let _ = window.destroy();
                return Err("the window did not answer".into());
            }
        };

        let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
        let session = Arc::clone(&app.state::<SharedSession>());
        let resolve_session = Arc::clone(&session);
        let deck = VideoDeck::new(move |asset| {
            resolve_session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .video_file(asset, &cache)
                .ok()
        });
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (width, height) = unpack(size.load(Ordering::Relaxed));
        let transport = Arc::clone(&app.state::<SharedOutput>());
        let render_stop = Arc::clone(&stop);
        let render = move |screen| Render {
            screen,
            deck,
            session,
            transport,
            size,
            stop: render_stop,
            timeline: None,
            seen: (u64::MAX, String::new()),
            reader: None,
        };
        let thread = thread::Builder::new()
            .name("flux-video-output".into())
            .spawn(move || match Screen::new(target, width, height) {
                Ok(screen) => {
                    let _ = ready_tx.send(Ok(screen.errors()));
                    render(screen).run();
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err.to_string()));
                }
            })
            .map_err(|err| err.to_string())?;
        let started = ready_rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| Err("the graphics device did not answer".into()));
        let gpu_errors = match started {
            Ok(errors) => errors,
            Err(err) => {
                stop.store(true, Ordering::Relaxed);
                let _ = window.destroy();
                return Err(err);
            }
        };
        *self.live.lock().unwrap_or_else(PoisonError::into_inner) = Some(Live {
            window,
            display: display.id.clone(),
            stop,
            thread: Some(thread),
            gpu_errors,
        });
        Ok(())
    }

    fn status(&self, app: &AppHandle, chosen: Option<String>) -> VideoOutputStatus {
        let displays: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
        let missing = chosen
            .as_ref()
            .is_some_and(|id| !displays.iter().any(|d| &d.id == id));
        let (open, gpu_error) = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|live| !live.stop.load(Ordering::Relaxed))
            .map_or((false, None), |live| {
                let error = live
                    .gpu_errors
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                (true, error)
            });
        let error = self
            .error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .or(gpu_error);
        VideoOutputStatus {
            displays,
            chosen,
            open,
            missing,
            error,
        }
    }
}

fn pack(size: tauri::PhysicalSize<u32>) -> u64 {
    (u64::from(size.width) << 32) | u64::from(size.height)
}

fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// The render thread's state.
struct Render {
    screen: Screen,
    deck: VideoDeck,
    session: SharedSession,
    transport: SharedOutput,
    size: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    /// The timeline last copied from the session, and the revision it was copied at.
    timeline: Option<Timeline>,
    seen: (u64, String),
    reader: Option<PositionReader>,
}

impl Render {
    fn run(mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            let (width, height) = unpack(self.size.load(Ordering::Relaxed));
            self.screen.resize(width, height);
            let drawn = self.step();
            if !drawn {
                // Nothing on show, or a window the system is not showing: no need for every refresh.
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Draws one refresh; `false` when it drew black or nothing.
    fn step(&mut self) -> bool {
        // The transport is locked only while it changes: then the last reader is used.
        if let Ok(output) = self.transport.try_lock() {
            self.reader = output.as_ref().map(|running| running.reader());
        }
        let (timeline, cue) = match &self.reader {
            Some(reader) => {
                let (state, frame) = reader.timeline_frame();
                let cue = if state == TransportState::Stopped {
                    Cue::Black
                } else {
                    Cue::At(frame)
                };
                match reader.show() {
                    Some((plan, tick)) => (Some(plan.entries[tick.entry].timeline.clone()), cue),
                    None => (self.edited_timeline(), cue),
                }
            }
            None => (None, Cue::Black),
        };
        let shown = match &timeline {
            Some(timeline) => self.deck.show(timeline, cue),
            None => Vec::new(),
        };
        let top = shown
            .iter()
            .find_map(|shown| shown.frame.as_ref().map(|frame| (&shown.layer.clip, frame)));
        let picture = top.map(|(clip, frame)| {
            let mut hash = DefaultHasher::new();
            (clip, frame.index).hash(&mut hash);
            Picture {
                id: hash.finish(),
                width: frame.width,
                height: frame.height,
                rgba: &frame.rgba,
            }
        });
        let drawn = picture.is_some();
        // A lost surface draws nothing and is tried again on the next refresh.
        self.screen.draw(picture).is_ok() && drawn
    }

    /// The timeline being edited, copied again only when the document changed.
    fn edited_timeline(&mut self) -> Option<Timeline> {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let (revision, active) = session.revision();
        if (revision, active) != (self.seen.0, self.seen.1.as_str()) {
            self.seen = (revision, active.to_owned());
            self.timeline = session.active_timeline_copy();
        }
        self.timeline.clone()
    }
}

fn chosen_display(app: &AppHandle) -> Option<String> {
    preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
        .ok()
        .and_then(|prefs| prefs.video_display)
}

/// Opens the output on the display saved in the preferences, when there is one. Called at start.
pub fn restore(app: &AppHandle) {
    let output = Arc::clone(&app.state::<SharedVideoOutput>());
    output.apply(app, chosen_display(app).as_deref());
}

/// The displays, the one chosen, and whether the window is open there.
#[tauri::command]
pub fn video_output_status(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
) -> VideoOutputStatus {
    output.status(&app, chosen_display(&app))
}

/// Chooses the display the output opens on (`None` closes it), keeps the choice in the local
/// preferences and opens or closes the window.
#[tauri::command]
pub async fn video_output_choose(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    display: Option<String>,
) -> Result<VideoOutputStatus, String> {
    preferences::preferences_change(
        app.clone(),
        app.state::<preferences::Store>(),
        Change::VideoDisplay {
            display: display.clone(),
        },
    )?;
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        output.apply(&worker_app, display.as_deref());
        output.status(&worker_app, display)
    })
    .await
    .map_err(|err| err.to_string())
}
