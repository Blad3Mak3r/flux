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

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Manager, Monitor, State, Window, WindowEvent};

use crate::commands::SharedSession;
use crate::preferences::{self, Change};
use crate::transport_commands::SharedOutput;
use flux_engine::output::PositionReader;
use flux_engine::transport::TransportState;
use flux_project::Timeline;
use flux_render::pattern::CardSlice;
use flux_render::{Mask, Picture, Quad, Screen};
use flux_video::{Cue, VideoDeck};

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
}

/// The window of one screen.
struct VideoOutput {
    screen: String,
    live: Mutex<Option<Live>>,
    error: Mutex<Option<String>>,
    pattern: Arc<AtomicBool>,
    deck: SharedDeck,
}

/// The one deck every screen's window draws from, so a clip is decoded once however many screens
/// show it. Made when the first window opens.
type SharedDeck = Arc<Mutex<Option<VideoDeck>>>;

/// The windows of every screen, by screen id.
#[derive(Default)]
pub struct VideoOutputs {
    outputs: Mutex<BTreeMap<String, Arc<VideoOutput>>>,
    /// A test pattern is shown where there is no picture, to line the slices up with.
    pattern: Arc<AtomicBool>,
    deck: SharedDeck,
    /// The app is quitting: no window opens again.
    closing: AtomicBool,
    /// One sync at a time, so two never open the same window.
    syncing: Mutex<()>,
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
    /// connected. When it is already open there it only takes the screen's new name.
    fn apply(self: &Arc<Self>, app: &AppHandle, chosen: Option<&str>, name: &str, index: usize) {
        {
            let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(live) = live.as_mut()
                && chosen == Some(live.display.as_str())
            {
                if live.name != name {
                    let _ = live.window.set_title(&title(name));
                    live.name = name.to_owned();
                }
                return;
            }
        }
        self.close();
        let Some(chosen) = chosen else {
            *self.error.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return;
        };
        let Some((display, monitor)) = displays(app).into_iter().find(|(d, _)| d.id == chosen)
        else {
            log::warn!("video output {name}: display {chosen} is not connected");
            return;
        };
        let result = self.open(app, &display, &monitor, name, index);
        if let Err(err) = &result {
            log::warn!("video output {name} could not open on {chosen}: {err}");
        } else {
            log::info!("video output {name} open on {chosen}");
        }
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = result.err();
    }

    fn open(
        self: &Arc<Self>,
        app: &AppHandle,
        display: &Display,
        monitor: &Monitor,
        name: &str,
        index: usize,
    ) -> Result<(), String> {
        let scale = monitor.scale_factor();
        let at = monitor.position();
        let (x, y) = (f64::from(at.x) / scale, f64::from(at.y) / scale);
        // On the editor's own display it is a window that can be moved aside, so a single screen
        // is not covered; on any other it fills the display.
        let label = format!("{LABEL_PREFIX}{}", self.screen);
        let mut builder = Window::builder(app, label)
            .title(title(name))
            .visible(true)
            .focused(false);
        if let Some(icon) = app.default_window_icon().cloned() {
            builder = builder.icon(icon).map_err(|err| err.to_string())?;
        }
        let window = if display.main {
            // Each screen a little further in, so windows on one display do not hide each other.
            let step = 80.0 + 40.0 * index as f64;
            builder
                .inner_size(960.0, 540.0)
                .position(x + step, y + step)
                .build()
        } else {
            builder
                .decorations(false)
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

        let session = Arc::clone(&app.state::<SharedSession>());
        ensure_deck(&self.deck, app)?;
        let deck = Arc::clone(&self.deck);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (width, height) = unpack(size.load(Ordering::Relaxed));
        let transport = Arc::clone(&app.state::<SharedOutput>());
        let grid = Arc::clone(&app.state::<crate::vj_commands::SharedGrid>());
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let render_stop = Arc::clone(&stop);
        let pattern = Arc::clone(&self.pattern);
        let screen_id = self.screen.clone();
        let render = move |screen| Render {
            screen,
            screen_id,
            deck,
            session,
            transport,
            size,
            stop: render_stop,
            pattern,
            timeline: None,
            quads: Vec::new(),
            masks: Vec::new(),
            seen: (u64::MAX, String::new()),
            reader: None,
            card: None,
            card_size: CARD_SIZE,
            card_slices: Vec::new(),
            card_frame: Vec::new(),
            card_id: 0,
            decoding: None,
            grid,
            modulation,
            vj: None,
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
            name: name.to_owned(),
            stop,
            thread: Some(thread),
            gpu_errors,
        });
        Ok(())
    }

    /// Whether the window is open, and the last error it met.
    fn state(&self) -> (bool, Option<String>) {
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
        (open, error)
    }
}

fn title(name: &str) -> String {
    format!("FLUX video · {name}")
}

impl VideoOutputs {
    /// Opens, moves, renames or closes a window for every screen of the project, as the
    /// preferences place them.
    pub fn sync(&self, app: &AppHandle) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        let _one = self.syncing.lock().unwrap_or_else(PoisonError::into_inner);
        let screens = app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let prefs =
            preferences::preferences_get(app.clone(), app.state::<preferences::Store>()).ok();
        let (gone, wanted) = {
            let mut outputs = self.outputs.lock().unwrap_or_else(PoisonError::into_inner);
            let gone: Vec<Arc<VideoOutput>> = outputs
                .extract_if(.., |id, _| !screens.iter().any(|screen| &screen.id == id))
                .map(|(_, output)| output)
                .collect();
            let wanted: Vec<_> = screens
                .iter()
                .enumerate()
                .map(|(index, screen)| {
                    let output = outputs.entry(screen.id.clone()).or_insert_with(|| {
                        Arc::new(VideoOutput {
                            screen: screen.id.clone(),
                            live: Mutex::default(),
                            error: Mutex::default(),
                            pattern: Arc::clone(&self.pattern),
                            deck: Arc::clone(&self.deck),
                        })
                    });
                    let display = prefs.as_ref().and_then(|p| p.video_display_for(&screen.id));
                    (Arc::clone(output), display, screen.name.clone(), index)
                })
                .collect();
            (gone, wanted)
        };
        for output in gone {
            output.close();
        }
        for (output, display, name, index) in wanted {
            output.apply(app, display.as_deref(), &name, index);
        }
    }

    fn status(&self, app: &AppHandle) -> VideoOutputStatus {
        let displays: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
        let prefs =
            preferences::preferences_get(app.clone(), app.state::<preferences::Store>()).ok();
        let screens = app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let outputs = self.outputs.lock().unwrap_or_else(PoisonError::into_inner);
        let screens = screens
            .into_iter()
            .map(|screen| {
                let chosen = prefs.as_ref().and_then(|p| p.video_display_for(&screen.id));
                let missing = chosen
                    .as_ref()
                    .is_some_and(|id| !displays.iter().any(|d| &d.id == id));
                let (open, error) = outputs
                    .get(&screen.id)
                    .map_or((false, None), |output| output.state());
                ScreenStatus {
                    id: screen.id,
                    name: screen.name,
                    chosen,
                    open,
                    missing,
                    error,
                }
            })
            .collect();
        let decoding = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(flux_video::VideoDeck::decoding)
            .map(decoding_name);
        VideoOutputStatus {
            displays,
            screens,
            decoding,
        }
    }

    /// Closes every window for good: the app is quitting.
    fn close_all(&self) {
        self.closing.store(true, Ordering::Relaxed);
        let outputs =
            std::mem::take(&mut *self.outputs.lock().unwrap_or_else(PoisonError::into_inner));
        for output in outputs.into_values() {
            output.close();
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
    /// The id of the screen it draws, whose slices it shows.
    screen_id: String,
    deck: SharedDeck,
    session: SharedSession,
    transport: SharedOutput,
    size: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    pattern: Arc<AtomicBool>,
    /// The timeline and the slices last copied from the session, and the revision they were
    /// copied at.
    timeline: Option<Timeline>,
    quads: Vec<Quad>,
    masks: Vec<Mask>,
    seen: (u64, String),
    reader: Option<PositionReader>,
    /// The test card, made when it is shown and again when the slices change, the frame with its
    /// sweep and its id.
    card: Option<Vec<u8>>,
    card_size: (u32, u32),
    card_slices: Vec<CardSlice>,
    card_frame: Vec<u8>,
    card_id: u64,
    /// Where the clips were decoded at the last refresh, written to the log when it changes.
    decoding: Option<flux_video::Decoding>,
    /// What the VJ grid plays, and the grid copied from the session for a VJ project, which
    /// shows its grid instead of a timeline.
    grid: crate::vj_commands::SharedGrid,
    /// Puts the modulated values in the grid it draws.
    modulation: crate::modulation::SharedModulation,
    vj: Option<flux_project::vj::Vj>,
}

/// The size the test card is made at when the project has no video; the GPU scales it to each
/// slice.
const CARD_SIZE: (u32, u32) = (1920, 1080);

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
        self.refresh();
        let bpm = self.timeline.as_ref().map_or(120.0, |t| t.tempo.bpm);
        let (shown, beats) = match &self.vj {
            Some(vj) => {
                let (play, now, beats) = {
                    let mut grid = self.grid.lock().unwrap_or_else(PoisonError::into_inner);
                    (grid.play.clone(), grid.now(), grid.beats(bpm))
                };
                // What is drawn is the grid with its modulated values; the stored one is not changed.
                let clock = flux_project::vj_modulation::Clock {
                    seconds: now.as_secs_f64(),
                    beats,
                };
                let drawn = self.modulation.apply(vj, clock);
                let shown = self
                    .deck
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_mut()
                    .map(|deck| deck.show_grid(&drawn, &play, now))
                    .unwrap_or_default();
                (shown, beats)
            }
            None => (self.timeline_pictures(), 0.0),
        };
        if self.screen_id == flux_project::mapping::MAIN_SCREEN {
            self.note_decoding();
        }
        let mut pictures = pictures(&shown, self.card_size, beats);
        let card = pictures.is_empty() && self.sweep_card();
        if card {
            pictures.push(Picture {
                id: self.card_id,
                width: self.card_size.0,
                height: self.card_size.1,
                rgba: &self.card_frame,
                place: [0.0, 0.0, 1.0, 1.0],
                opacity: 1.0,
                blend: flux_render::Blend::Alpha,
                effects: Default::default(),
            });
        }
        let drawn = !pictures.is_empty();
        // A lost surface draws nothing and is tried again on the next refresh.
        self.screen
            .draw(&pictures, self.card_size, &self.quads, &self.masks)
            .is_ok()
            && drawn
    }

    /// The layers the timeline shows where the transport is, topmost first.
    fn timeline_pictures(&mut self) -> Vec<flux_video::Shown> {
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
                    None => (self.timeline.clone(), cue),
                }
            }
            None => (None, Cue::Black),
        };
        match &timeline {
            // Held only while the deck asks for frames: the pictures are shared, not copied.
            Some(timeline) => self
                .deck
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .map(|deck| deck.show(timeline, cue))
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Writes to the log where the clips are decoded when it changes; the main screen's window
    /// does it for all of them, which share one deck.
    fn note_decoding(&mut self) {
        let now = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(flux_video::VideoDeck::decoding);
        if now != self.decoding {
            if let Some(decoding) = now {
                log::info!("video: decoding on {}", decoding_name(decoding));
            }
            self.decoding = now;
        }
    }

    /// Draws the sweep on the test card where it is now; `false` while the pattern is off.
    fn sweep_card(&mut self) -> bool {
        if !self.pattern.load(Ordering::Relaxed) {
            return false;
        }
        let (width, height) = self.card_size;
        let slices = &self.card_slices;
        let card = self
            .card
            .get_or_insert_with(|| flux_render::pattern::test_card(width, height, slices));
        // Timed by the system clock, so the sweep crosses every screen together, a minute being a
        // whole number of sweeps.
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() % 60_000);
        let seconds = millis as f32 / 1000.0;
        let clock = chrono::Local::now().format("%H:%M:%S").to_string();
        flux_render::pattern::sweep(card, &mut self.card_frame, width, height, seconds, &clock);
        // Each refresh is a new picture: high ids, apart from the hashes of video frames.
        self.card_id = u64::MAX - (seconds * 1000.0) as u64;
        true
    }

    /// Copies the timeline being edited and the slices again when the document changed.
    fn refresh(&mut self) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let (revision, active) = session.revision();
        if (revision, active) == (self.seen.0, self.seen.1.as_str()) {
            return;
        }
        self.seen = (revision, active.to_owned());
        self.timeline = session.active_timeline_copy();
        self.vj = session.grid_copy();
        let screens = session.video_screens();
        let this = screens.iter().find(|screen| screen.id == self.screen_id);
        let slices = this.map_or(&[][..], |screen| &screen.slices);
        self.masks = this
            .map_or(&[][..], |screen| &screen.masks)
            .iter()
            .filter(|mask| !mask.hidden)
            .map(|mask| Mask {
                points: mask.points.iter().map(|p| [p.x, p.y]).collect(),
                invert: mask.invert,
            })
            .collect();
        let shown = slices.iter().filter(|slice| !slice.hidden);
        let rect = |r: &flux_project::mapping::Rect| [r.x, r.y, r.width, r.height];
        self.quads = shown
            .clone()
            .map(|slice| Quad {
                input: rect(&slice.input),
                corners: slice.corners.map(|p| [p.x, p.y]),
                soft: [
                    slice.soft_edge.left,
                    slice.soft_edge.right,
                    slice.soft_edge.top,
                    slice.soft_edge.bottom,
                ],
                gamma: slice.soft_edge.gamma,
            })
            .collect();
        let card_slices: Vec<CardSlice> = shown
            .map(|slice| CardSlice {
                name: slice.name.clone(),
                input: rect(&slice.input),
            })
            .collect();
        drop(session);
        let size = composition_size(&self.session);
        if (size, &card_slices) != (self.card_size, &self.card_slices) {
            self.card_size = size;
            self.card_slices = card_slices;
            self.card = None;
        }
    }
}

/// The composition: the size the clips are sized into, the slices cut and the card drawn.
fn composition_size(session: &SharedSession) -> (u32, u32) {
    crate::video_commands::picture_size(session)
        .filter(|&(w, h)| {
            let sides = flux_project::mapping::COMPOSITION_SIDES;
            sides.contains(&w) && sides.contains(&h)
        })
        .unwrap_or(CARD_SIZE)
}

/// What to draw of `shown` (topmost first) in a composition of `composition` pixels, the bottom
/// first: from the top down to the first picture that hides everything under it.
fn pictures(shown: &[flux_video::Shown], composition: (u32, u32), beats: f64) -> Vec<Picture<'_>> {
    let mut pictures = Vec::new();
    for shown in shown {
        let Some(frame) = &shown.frame else { continue };
        let layer = &shown.layer;
        let mut hash = DefaultHasher::new();
        (&layer.clip, frame.index).hash(&mut hash);
        let place = flux_render::transformed(
            flux_render::placement(sizing(layer.fit), (frame.width, frame.height), composition),
            layer.scale,
            layer.position,
        );
        let time = flux_render::Time {
            seconds: layer.elapsed.as_secs_f64(),
            beats,
        };
        let effects = flux_render::Effects::new(&layer.effects, time);
        pictures.push(Picture {
            id: hash.finish(),
            width: frame.width,
            height: frame.height,
            rgba: &frame.rgba,
            place,
            opacity: layer.opacity,
            blend: blend(layer.blend),
            effects,
        });
        let alpha = layer.blend == flux_project::vj::Blend::Alpha;
        // Effects can leave parts of the picture clear (a zoom out, a blur's soft edge), so a
        // picture with any hides nothing below it.
        if alpha && layer.opacity >= 1.0 && !frame.alpha && covers(place) && effects.is_identity() {
            break;
        }
    }
    pictures.reverse();
    pictures
}

/// The width of the composition the editor's monitor shows; its height keeps the shape.
const MONITOR_WIDTH: u32 = 480;

/// Makes the deck every screen draws from, when no window has yet.
fn ensure_deck(deck: &SharedDeck, app: &AppHandle) -> Result<(), String> {
    let mut deck = deck.lock().unwrap_or_else(PoisonError::into_inner);
    if deck.is_none() {
        let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
        let session = Arc::clone(&app.state::<SharedSession>());
        *deck = Some(VideoDeck::new(move |asset| {
            session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .video_file(asset, &cache)
                .ok()
        }));
    }
    Ok(())
}

impl VideoOutputs {
    /// What a VJ project's grid sends to the screens now, before their mapping, drawn small on
    /// the CPU: its width and height as two little-endian u16, then its RGBA pixels. `None` for a
    /// project that is not VJ. It decodes the playing clips itself,
    /// so it shows them with no screen open too.
    pub fn composition(&self, app: &AppHandle) -> Result<Option<Vec<u8>>, String> {
        let session = Arc::clone(&app.state::<SharedSession>());
        let Some(vj) = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .grid_copy()
        else {
            return Ok(None);
        };
        ensure_deck(&self.deck, app)?;
        let bpm = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active_timeline_copy()
            .map_or(120.0, |timeline| timeline.tempo.bpm);
        let (play, now, beats) = {
            let grid = app.state::<crate::vj_commands::SharedGrid>();
            let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
            (grid.play.clone(), grid.now(), grid.beats(bpm))
        };
        let clock = flux_project::vj_modulation::Clock {
            seconds: now.as_secs_f64(),
            beats,
        };
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let drawn = modulation.apply(&vj, clock);
        let shown = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .map(|deck| deck.show_grid(&drawn, &play, now))
            .unwrap_or_default();
        let size = composition_size(&session);
        let (width, height) = (
            MONITOR_WIDTH,
            (MONITOR_WIDTH * size.1 / size.0.max(1)).max(1),
        );
        // Its width and height as two little-endian u16, then its RGBA pixels: no encoding to
        // wait for on either side.
        let mut picture = Vec::with_capacity(4 + (width * height * 4) as usize);
        picture.extend_from_slice(&(width as u16).to_le_bytes());
        picture.extend_from_slice(&(height as u16).to_le_bytes());
        picture.extend(flux_render::composite(
            &pictures(&shown, size, beats),
            width,
            height,
        ));
        Ok(Some(picture))
    }
}

/// `cpu`, or the GPU device's name.
fn decoding_name(decoding: flux_video::Decoding) -> String {
    match decoding {
        flux_video::Decoding::Software => "cpu".to_owned(),
        flux_video::Decoding::Hardware(device) => device.to_owned(),
    }
}

/// Whether a picture at `place` fills the whole composition, hiding what is under it.
fn covers(place: [f32; 4]) -> bool {
    const SLACK: f32 = 1e-4;
    let [x, y, w, h] = place;
    x <= SLACK && y <= SLACK && x + w >= 1.0 - SLACK && y + h >= 1.0 - SLACK
}

/// The renderer's sizing for a clip's fit.
fn sizing(fit: flux_project::VideoFit) -> flux_render::Sizing {
    use flux_project::VideoFit;
    match fit {
        VideoFit::Fit => flux_render::Sizing::Fit,
        VideoFit::Fill => flux_render::Sizing::Fill,
        VideoFit::Stretch => flux_render::Sizing::Stretch,
        VideoFit::None => flux_render::Sizing::None,
    }
}

/// Closes the windows when the last window of the editor goes, so the app quits with it: a window
/// without a webview would otherwise keep it running with nothing to bring the editor back.
pub fn window_gone(app: &AppHandle, label: &str) {
    if !label.starts_with(LABEL_PREFIX) && app.webview_windows().is_empty() {
        app.state::<SharedVideoOutput>().close_all();
        app.exit(0);
    }
}

/// Opens the screens on the displays saved in the preferences, then keeps following the project:
/// a screen added, removed or renamed, or another project opened, opens or closes its window.
/// Called at start, off the main thread.
pub fn restore(app: &AppHandle) {
    let outputs = Arc::clone(&app.state::<SharedVideoOutput>());
    let session = Arc::clone(&app.state::<SharedSession>());
    let mut seen = Vec::new();
    while !outputs.closing.load(Ordering::Relaxed) {
        let screens: Vec<(String, String)> = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens()
            .into_iter()
            .map(|screen| (screen.id, screen.name))
            .collect();
        if screens != seen {
            seen = screens;
            outputs.sync(app);
        }
        thread::sleep(Duration::from_millis(400));
    }
}

/// The displays and, for each screen, the display chosen and whether its window is open there.
#[tauri::command]
pub fn video_output_status(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
) -> VideoOutputStatus {
    output.status(&app)
}

/// Chooses the display a screen opens on (`None` closes it), keeps the choice in the local
/// preferences and opens or closes its window.
#[tauri::command]
pub async fn video_output_choose(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    screen: String,
    display: Option<String>,
) -> Result<VideoOutputStatus, String> {
    preferences::preferences_change(
        app.clone(),
        app.state::<preferences::Store>(),
        Change::VideoDisplay { screen, display },
    )?;
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        output.sync(&worker_app);
        output.status(&worker_app)
    })
    .await
    .map_err(|err| err.to_string())
}

/// Shows or hides the test pattern where there is no picture.
#[tauri::command]
pub fn video_output_pattern(output: State<'_, SharedVideoOutput>, on: bool) {
    output.pattern.store(on, Ordering::Relaxed);
}

/// The render's blend for a layer's.
fn blend(blend: flux_project::vj::Blend) -> flux_render::Blend {
    use flux_project::vj::Blend as B;
    match blend {
        B::Alpha => flux_render::Blend::Alpha,
        B::Add => flux_render::Blend::Add,
        B::Subtract => flux_render::Blend::Subtract,
        B::Multiply => flux_render::Blend::Multiply,
        B::Screen => flux_render::Blend::Screen,
        B::Lighten => flux_render::Blend::Lighten,
        B::Darken => flux_render::Blend::Darken,
    }
}
