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

use std::collections::{BTreeMap, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Manager, Monitor, State, Window, WindowEvent};

use crate::commands::SharedSession;
use crate::preferences::{self, Change};
use crate::transport_commands::SharedOutput;
use flux_engine::output::PositionReader;
use flux_engine::transport::TransportState;
use flux_project::Timeline;
use flux_render::pattern::{CardSlice, Logo};
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
type SharedDeck = Arc<Mutex<Option<VideoDeck>>>;

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
            let wish = *self
                .fullscreen
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(live) = live.as_mut()
                && chosen == Some(live.display.as_str())
                && wish.is_none_or(|full| full == live.fullscreen)
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
        let wish = *self
            .fullscreen
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let fullscreen = wish.unwrap_or(!display.main);
        let window = if !fullscreen {
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
        let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
        ensure_deck(&self.deck, app)?;
        let deck = Arc::clone(&self.deck);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (width, height) = unpack(size.load(Ordering::Relaxed));
        let transport = Arc::clone(&app.state::<SharedOutput>());
        let grid = Arc::clone(&app.state::<crate::vj_commands::SharedGrid>());
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let render_stop = Arc::clone(&stop);
        let pattern = Arc::clone(&self.pattern);
        let controls = Arc::clone(&self.controls);
        let screen_id = self.screen.clone();
        let render = move |screen| Render {
            screen,
            screen_id,
            deck,
            session,
            cache,
            transport,
            size,
            stop: render_stop,
            pattern,
            controls,
            screen_name: String::new(),
            timeline: None,
            quads: Vec::new(),
            masks: Vec::new(),
            seen: (u64::MAX, String::new()),
            reader: None,
            cards: None,
            card_size: CARD_SIZE,
            card_slices: Vec::new(),
            card_caption: String::new(),
            card_frames: Vec::new(),
            card_ids: Vec::new(),
            card_seq: 0,
            decoding: None,
            grid,
            modulation,
            vj: None,
            drawn: None,
            last_frame: None,
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
            fullscreen,
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

/// How a slice is marked on the test card.
fn card_slice(slice: &flux_project::mapping::Slice) -> CardSlice {
    let r = slice.input;
    CardSlice {
        name: slice.name.clone(),
        input: [r.x, r.y, r.width, r.height],
        card: slice.test_card.clone(),
        logo: None,
    }
}

/// The logos read so far, by the id of the project's image; `None` for one that could not be.
static LOGOS: OnceLock<Mutex<HashMap<String, Option<Logo>>>> = OnceLock::new();

/// Reads the image `id` of the project, once: FFmpeg decodes its first picture.
fn logo_for(session: &SharedSession, cache: &Path, id: &str) -> Option<Logo> {
    let known = LOGOS.get_or_init(Default::default);
    if let Some(found) = known.lock().unwrap_or_else(PoisonError::into_inner).get(id) {
        return found.clone();
    }
    let path = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .video_file(id, cache)
        .ok();
    let logo = path.and_then(|path| {
        let frame = flux_video::VideoFile::open(&path).ok()?.frame(0).ok()?;
        Some(Logo {
            id: id.to_owned(),
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.into(),
        })
    });
    known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.to_owned(), logo.clone());
    logo
}

/// Gives each slice that shows a logo its picture.
fn with_logos(slices: &mut [CardSlice], session: &SharedSession, cache: &Path) {
    for slice in slices {
        if slice.card.show_logo && !slice.card.logo.is_empty() {
            slice.logo = logo_for(session, cache, &slice.card.logo);
        }
    }
}

/// What the test card says of a screen: its name and the size of its window in pixels.
fn card_caption(name: &str, width: u32, height: u32) -> String {
    format!("{name}  {width} x {height}")
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
                            controls: Arc::clone(&self.controls),
                            fullscreen: Mutex::default(),
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
    /// Where the project's media is unpacked to be read.
    cache: PathBuf,
    transport: SharedOutput,
    size: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    pattern: Arc<AtomicBool>,
    controls: Arc<Controls>,
    /// The screen's name, for the test card.
    screen_name: String,
    /// The timeline and the slices last copied from the session, and the revision they were
    /// copied at.
    timeline: Option<Timeline>,
    quads: Vec<Quad>,
    masks: Vec<Mask>,
    seen: (u64, String),
    reader: Option<PositionReader>,
    /// The test card of each slice, made when one is shown and again when the slices or their
    /// settings change; each one's frame with its moving part and the id its picture goes by.
    cards: Option<Vec<flux_render::pattern::SliceCard>>,
    card_size: (u32, u32),
    card_slices: Vec<CardSlice>,
    /// What the card says of the screen, made again when it changes.
    card_caption: String,
    card_frames: Vec<Vec<u8>>,
    card_ids: Vec<u64>,
    /// Counts the pictures the cards made, for their ids.
    card_seq: u64,
    /// Where the clips were decoded at the last refresh, written to the log when it changes.
    decoding: Option<flux_video::Decoding>,
    /// What the VJ grid plays, and the grid copied from the session for a VJ project, which
    /// shows its grid instead of a timeline.
    grid: crate::vj_commands::SharedGrid,
    /// Puts the modulated values in the grid it draws.
    modulation: crate::modulation::SharedModulation,
    vj: Option<flux_project::vj::Vj>,
    /// The grid with its modulated values, kept between frames and patched; reset with `vj`.
    drawn: Option<flux_project::vj::Vj>,
    /// When the last frame was drawn, to tell the effects with a history how long it has been.
    last_frame: Option<Instant>,
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
        let at = Instant::now();
        let dt = self
            .last_frame
            .replace(at)
            .map_or(1.0 / 60.0, |last| at.duration_since(last).as_secs_f32());
        let (shown, beats, post) = match &self.vj {
            Some(vj) => {
                let (play, now) = {
                    let mut grid = self.grid.lock().unwrap_or_else(PoisonError::into_inner);
                    let now = grid.tick(vj, bpm);
                    (grid.play.clone(), now)
                };
                let beats = now.beats;
                // What is drawn is the grid with its modulated values; the stored one is not changed.
                let clock = flux_project::vj_modulation::Clock {
                    seconds: now.time.as_secs_f64(),
                    beats,
                };
                let drawn = self.modulation.draw(vj, &mut self.drawn, clock);
                let shown = self
                    .deck
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_mut()
                    .map(|deck| deck.show_grid(drawn, &play, now, self.card_size))
                    .unwrap_or_default();
                let post = composition_post(drawn, clock, dt);
                (shown, beats, post)
            }
            None => (self.timeline_pictures(), 0.0, flux_render::Post::default()),
        };
        if self.screen_id == flux_project::mapping::MAIN_SCREEN {
            self.note_decoding();
        }
        let disabled = self.controls.disabled.load(Ordering::Relaxed);
        let forced = self.controls.card.load(Ordering::Relaxed);
        let mut pictures = if disabled || forced {
            Vec::new()
        } else {
            pictures(&shown, self.card_size, beats)
        };
        // The whole card replaces the picture; a slice that asks for its own is laid over it.
        let everywhere = forced || (pictures.is_empty() && self.pattern.load(Ordering::Relaxed));
        let on = if disabled {
            Vec::new()
        } else {
            self.prepare_cards(everywhere)
        };
        let card = !on.is_empty();
        for &at in &on {
            let [x, y, width, height] = self.cards.as_ref().map_or([0; 4], |cards| cards[at].rect);
            let (cw, ch) = (self.card_size.0 as f32, self.card_size.1 as f32);
            let fixed = self.cards.as_ref().map(|cards| &cards[at].fixed);
            let moving = &self.card_frames[at];
            pictures.push(Picture {
                id: self.card_ids[at],
                width,
                height,
                rgba: if moving.is_empty() {
                    fixed.map_or(&[][..], |f| &f[..])
                } else {
                    moving
                },
                place: [
                    x as f32 / cw,
                    y as f32 / ch,
                    width as f32 / cw,
                    height as f32 / ch,
                ],
                opacity: 1.0,
                blend: flux_render::Blend::Alpha,
                effects: Default::default(),
                nearest: true,
            });
        }
        let drawn = !pictures.is_empty();
        // The test card is what the slices are lined up with: nothing changes it.
        let post = if card || disabled {
            flux_render::Post::default()
        } else {
            post
        };
        // A lost surface draws nothing and is tried again on the next refresh.
        self.screen
            .draw(&pictures, self.card_size, &self.quads, &self.masks, &post)
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

    /// Makes the cards of the slices to show (all of them when `everywhere`, else those that ask
    /// for it) and their moving part for now; the indexes of the cards to draw.
    fn prepare_cards(&mut self, everywhere: bool) -> Vec<usize> {
        let wanted: Vec<usize> = (0..self.card_slices.len())
            .filter(|&at| everywhere || self.card_slices[at].card.show)
            .collect();
        if wanted.is_empty() {
            return wanted;
        }
        let (window_width, window_height) = unpack(self.size.load(Ordering::Relaxed));
        let caption = card_caption(&self.screen_name, window_width, window_height);
        if caption != self.card_caption {
            self.card_caption = caption;
            self.cards = None;
        }
        if self.cards.is_none() {
            let cards =
                flux_render::pattern::layout(&self.card_slices, self.card_size, &self.card_caption);
            self.card_frames = vec![Vec::new(); cards.len()];
            self.card_ids = cards.iter().map(|_| self.next_card_id()).collect();
            self.cards = Some(cards);
        }
        // Timed by the system clock, so the marker crosses every screen together, a minute being a
        // whole number of crossings.
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() % 60_000);
        let seconds = millis as f32 / 1000.0;
        let clock = chrono::Local::now().format("%H:%M:%S").to_string();
        for &at in &wanted {
            let moved = self
                .cards
                .as_ref()
                .is_some_and(|cards| cards[at].frame(&mut self.card_frames[at], seconds, &clock));
            if moved {
                // Each refresh is a new picture.
                self.card_ids[at] = self.next_card_id();
            } else {
                self.card_frames[at].clear();
            }
        }
        wanted
    }

    /// An id for a picture of a card: high, apart from the hashes of video frames.
    fn next_card_id(&mut self) -> u64 {
        self.card_seq += 1;
        u64::MAX - self.card_seq
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
        self.drawn = None;
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
        if let Some(screen) = this {
            self.screen_name.clone_from(&screen.name);
        }
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
        let mut card_slices: Vec<CardSlice> = shown.map(card_slice).collect();
        if card_slices.is_empty() {
            // No slices: the card is of the whole picture, under the screen's name.
            card_slices.push(CardSlice {
                name: self.screen_name.clone(),
                input: [0.0, 0.0, 1.0, 1.0],
                card: Default::default(),
                logo: None,
            });
        }
        drop(session);
        with_logos(&mut card_slices, &self.session, &self.cache);
        let size = composition_size(&self.session);
        if (size, &card_slices) != (self.card_size, &self.card_slices) {
            self.card_size = size;
            self.card_slices = card_slices;
            self.cards = None;
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
            nearest: false,
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

/// What the composition's effects of `vj` come to at `clock`, `dt` seconds after the last frame;
/// none while they are bypassed.
fn composition_post(
    vj: &flux_project::vj::Vj,
    clock: flux_project::vj_modulation::Clock,
    dt: f32,
) -> flux_render::Post {
    if vj.composition_bypass {
        return flux_render::Post::default();
    }
    let time = flux_render::Time {
        seconds: clock.seconds,
        beats: clock.beats,
    };
    flux_render::Post::new(&vj.composition_effects, time, dt)
}

/// The width of the composition the editor's monitor shows; its height keeps the shape.
const MONITOR_WIDTH: u32 = 480;

/// A deck that opens the videos of the open project.
fn new_deck(app: &AppHandle) -> Result<VideoDeck, String> {
    let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
    let session = Arc::clone(&app.state::<SharedSession>());
    Ok(VideoDeck::new(move |asset| {
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_file(asset, &cache)
            .ok()
    }))
}

/// Makes the deck every screen draws from, when no window has yet.
fn ensure_deck(deck: &SharedDeck, app: &AppHandle) -> Result<(), String> {
    let mut deck = deck.lock().unwrap_or_else(PoisonError::into_inner);
    if deck.is_none() {
        *deck = Some(new_deck(app)?);
    }
    Ok(())
}

/// The monitor's picture of `shown` through the composition's effects: its width and height as
/// two little-endian u16, then its RGBA pixels (no encoding to wait for on either side). The
/// output and the preview monitors both come from here, so a clip looks the same in each.
fn monitor_picture(
    shown: &[flux_video::Shown],
    vj: &flux_project::vj::Vj,
    size: (u32, u32),
    width: u32,
    clock: flux_project::vj_modulation::Clock,
    last: &mut Option<Instant>,
    history: &mut flux_render::History,
) -> Vec<u8> {
    let height = (u64::from(width) * u64::from(size.1) / u64::from(size.0.max(1))).max(1) as u32;
    let mut picture = Vec::with_capacity(4 + (width * height * 4) as usize);
    picture.extend_from_slice(&(width as u16).to_le_bytes());
    picture.extend_from_slice(&(height as u16).to_le_bytes());
    let at = Instant::now();
    let dt = last
        .replace(at)
        .map_or(1.0 / 60.0, |last| at.duration_since(last).as_secs_f32());
    let post = composition_post(vj, clock, dt);
    picture.extend(flux_render::composite_post(
        &pictures(shown, size, clock.beats),
        width,
        height,
        &post,
        history,
    ));
    picture
}

/// The grid cut down to `clip`'s layer, with that layer at full strength and its clip alone: what
/// the preview plays, so the clip is drawn as it is placed and not as the layer is mixed.
fn preview_grid(vj: &flux_project::vj::Vj, clip: &str) -> Option<flux_project::vj::Vj> {
    let (layer, _) = vj.find_clip(clip)?;
    let mut layer = layer.clone();
    layer.clips.retain(|c| c.id == clip);
    layer.master = 1.0;
    layer.opacity = 1.0;
    layer.blend = flux_project::vj::Blend::Alpha;
    layer.transition = 0.0;
    layer.crossfade = None;
    layer.bypass = false;
    layer.solo = false;
    layer.modulation.clear();
    let mut vj = vj.clone();
    vj.layers = vec![layer];
    Some(vj)
}

/// How long the preview goes on without being asked for a picture before it lets go of its decoders.
const PREVIEW_IDLE: Duration = Duration::from_secs(1);

/// What the editor's preview monitor plays: the selected clip alone, on a clock and decoders of its
/// own, so previewing never changes what is on air.
#[derive(Default)]
struct Preview {
    clip: Option<String>,
    /// Its own clock and playhead (the grid's type, with only the clip launched).
    grid: crate::vj_commands::Grid,
    deck: Option<VideoDeck>,
    /// The session's revision and the clip the cut-down grid was made for, and the grid.
    cut: Option<(u64, String, String, flux_project::vj::Vj)>,
    drawn: Option<flux_project::vj::Vj>,
    history: flux_render::History,
    last: Option<Instant>,
    polled: Option<Instant>,
    /// A thread is watching for the preview to go unasked.
    watching: bool,
}

impl Preview {
    /// Lets go of the clip and its decoders.
    fn stop(&mut self) {
        *self = Self {
            watching: self.watching,
            ..Self::default()
        };
    }
}

/// Stops `preview` once nobody has asked for a picture for [`PREVIEW_IDLE`]; ends with it.
fn watch(preview: Arc<Mutex<Preview>>) {
    let _ = thread::Builder::new()
        .name("flux-vj-preview".into())
        .spawn(move || {
            loop {
                thread::sleep(PREVIEW_IDLE / 2);
                let mut preview = preview.lock().unwrap_or_else(PoisonError::into_inner);
                if preview.polled.is_none_or(|at| at.elapsed() >= PREVIEW_IDLE) {
                    preview.stop();
                    preview.watching = false;
                    return;
                }
            }
        });
}

impl VideoOutputs {
    /// What a VJ project's grid sends to the screens now, before their mapping, drawn small on
    /// the CPU: its width and height as two little-endian u16, then its RGBA pixels. `None` for a
    /// project that is not VJ. It decodes the playing clips itself,
    /// so it shows them with no screen open too.
    pub fn composition(&self, app: &AppHandle) -> Result<Option<Vec<u8>>, String> {
        self.draw_composition(app, Some(MONITOR_WIDTH))
    }

    /// The composition as [`Self::composition`] draws it, `width` pixels across (its own width
    /// when `None`, from a history of its own so the monitor's is left alone).
    fn draw_composition(
        &self,
        app: &AppHandle,
        width: Option<u32>,
    ) -> Result<Option<Vec<u8>>, String> {
        let session = Arc::clone(&app.state::<SharedSession>());
        // The grid is copied again only when the session moved.
        let mut guard = self.monitor.lock().unwrap_or_else(PoisonError::into_inner);
        let frames = &mut *guard;
        {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let (revision, active) = session.revision();
            if frames
                .grid
                .as_ref()
                .is_none_or(|(r, a, _)| (*r, a.as_str()) != (revision, active))
            {
                frames.grid = Some((revision, active.to_owned(), session.grid_copy()));
                frames.drawn = None;
            }
        }
        let Some(vj) = frames.grid.as_ref().and_then(|(_, _, vj)| vj.as_ref()) else {
            return Ok(None);
        };
        ensure_deck(&self.deck, app)?;
        let bpm = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active_timeline_copy()
            .map_or(120.0, |timeline| timeline.tempo.bpm);
        let (play, now) = {
            let grid = app.state::<crate::vj_commands::SharedGrid>();
            let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
            let now = grid.tick(vj, bpm);
            (grid.play.clone(), now)
        };
        let beats = now.beats;
        let clock = flux_project::vj_modulation::Clock {
            seconds: now.time.as_secs_f64(),
            beats,
        };
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let drawn = modulation.draw(vj, &mut frames.drawn, clock);
        let size = composition_size(&session);
        let shown = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .map(|deck| deck.show_grid(drawn, &play, now, size))
            .unwrap_or_default();
        let mut fresh = flux_render::History::default();
        let history = if width.is_some() {
            &mut frames.history
        } else {
            &mut fresh
        };
        Ok(Some(monitor_picture(
            &shown,
            drawn,
            size,
            width.unwrap_or(size.0),
            clock,
            &mut frames.last,
            history,
        )))
    }

    /// What `clip` alone looks like in the composition (see [`Self::composition`]), as that
    /// picture: its fit, scale, position and effects, at full strength on black. The clip plays on
    /// a clock of its own from its start, whatever is on air. `None` stops the preview, and so
    /// does going unasked for a second. Empty when there is no such clip.
    pub fn preview(&self, app: &AppHandle, clip: Option<&str>) -> Result<Vec<u8>, String> {
        let mut guard = self.preview.lock().unwrap_or_else(PoisonError::into_inner);
        let preview = &mut *guard;
        let Some(clip) = clip else {
            preview.stop();
            return Ok(Vec::new());
        };
        let session = Arc::clone(&app.state::<SharedSession>());
        let bpm = {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let (revision, active) = session.revision();
            let current = preview.cut.as_ref().is_some_and(|(r, a, c, _)| {
                (*r, a.as_str(), c.as_str()) == (revision, active, clip)
            });
            if !current {
                preview.cut = session
                    .grid_ref()
                    .and_then(|vj| preview_grid(vj, clip))
                    .map(|vj| (revision, active.to_owned(), clip.to_owned(), vj));
                preview.drawn = None;
            }
            session.tempo_bpm()
        };
        let Some((_, _, _, vj)) = preview.cut.as_ref() else {
            preview.stop();
            return Ok(Vec::new());
        };
        preview.polled = Some(Instant::now());
        if !preview.watching {
            preview.watching = true;
            watch(Arc::clone(&self.preview));
        }
        let tick = preview.grid.tick(vj, bpm);
        if preview.clip.as_deref() != Some(clip) {
            preview.clip = Some(clip.to_owned());
            // A new deck, so the last clip's decoders close and a file that failed to open is tried again.
            preview.deck = None;
            preview.history = flux_render::History::default();
            preview.grid.play = flux_video::GridPlay::default();
            preview.grid.play.launch(vj, clip, tick);
        }
        if preview.deck.is_none() {
            preview.deck = Some(new_deck(app)?);
        }
        let clock = flux_project::vj_modulation::Clock {
            seconds: tick.time.as_secs_f64(),
            beats: tick.beats,
        };
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let drawn = modulation.draw(vj, &mut preview.drawn, clock);
        let size = composition_size(&session);
        let shown = preview
            .deck
            .as_mut()
            .map(|deck| deck.show_grid(drawn, &preview.grid.play, tick, size))
            .unwrap_or_default();
        Ok(monitor_picture(
            &shown,
            drawn,
            size,
            MONITOR_WIDTH,
            clock,
            &mut preview.last,
            &mut preview.history,
        ))
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

/// What the Output menu shows of the output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputState {
    /// Every screen is black.
    pub disabled: bool,
    /// Every screen shows the test card.
    pub test_card: bool,
    /// How many displays there are to put the output full screen on.
    pub displays: u32,
    /// The display (from 1) the main screen covers; absent while it is a window.
    pub fullscreen: Option<u32>,
}

/// What the Output menu switches.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Control {
    Disable,
    TestCard,
}

/// The snapshot that was saved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// The display `n` counts from 1, in the order the system lists them, as Resolume's
/// Output > Fullscreen > Display N.
fn display_at(displays: &[Display], n: u32) -> Option<&Display> {
    displays.get(usize::try_from(n.checked_sub(1)?).ok()?)
}

impl VideoOutputs {
    fn output_state(&self, app: &AppHandle) -> OutputState {
        let displays: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
        let main = self
            .outputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(flux_project::mapping::MAIN_SCREEN)
            .cloned();
        let on = main.and_then(|output| {
            let live = output.live.lock().unwrap_or_else(PoisonError::into_inner);
            live.as_ref()
                .filter(|live| live.fullscreen && !live.stop.load(Ordering::Relaxed))
                .map(|live| live.display.clone())
        });
        OutputState {
            disabled: self.controls.disabled.load(Ordering::Relaxed),
            test_card: self.controls.card.load(Ordering::Relaxed),
            displays: displays.len() as u32,
            fullscreen: on
                .and_then(|id| displays.iter().position(|d| d.id == id))
                .map(|at| at as u32 + 1),
        }
    }

    /// Puts the main screen full screen on display `n` (from 1), or back in a window on the
    /// display it is on.
    fn set_fullscreen(&self, app: &AppHandle, display: Option<u32>) -> Result<(), String> {
        let main = self
            .outputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(flux_project::mapping::MAIN_SCREEN)
            .cloned()
            .ok_or("the project has no output screen")?;
        let wish = match display {
            Some(n) => {
                let all: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
                let id = display_at(&all, n)
                    .ok_or_else(|| format!("there is no display {n}"))?
                    .id
                    .clone();
                preferences::preferences_change(
                    app.clone(),
                    app.state::<preferences::Store>(),
                    Change::VideoDisplay {
                        screen: flux_project::mapping::MAIN_SCREEN.into(),
                        display: Some(id),
                    },
                )?;
                true
            }
            None => false,
        };
        *main
            .fullscreen
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(wish);
        self.sync(app);
        let (open, error) = main.state();
        match (open, error) {
            (_, Some(error)) => Err(error),
            (false, None) => Err("the output is not on a display".into()),
            _ => Ok(()),
        }
    }

    /// Saves the composition's picture as a PNG in `dir`.
    fn snapshot(&self, app: &AppHandle, dir: &std::path::Path) -> Result<Snapshot, String> {
        let picture = self
            .draw_composition(app, None)?
            .ok_or("a snapshot needs a VJ show")?;
        let size = |at: usize| u32::from(u16::from_le_bytes([picture[at], picture[at + 1]]));
        let (width, height) = (size(0), size(2));
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        let path = crate::snapshot::save(dir, &stamp, &picture[4..], width, height)?;
        Ok(Snapshot {
            path: path.to_string_lossy().into_owned(),
            width,
            height,
        })
    }
}

/// Whether the output is disabled or showing the test card, and where it is full screen.
#[tauri::command]
pub fn video_output_state(app: AppHandle, output: State<'_, SharedVideoOutput>) -> OutputState {
    output.output_state(&app)
}

/// Flips Disable Output or Show Test Card, and answers the state.
#[tauri::command]
pub fn video_output_toggle(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    control: Control,
) -> OutputState {
    let flag = match control {
        Control::Disable => &output.controls.disabled,
        Control::TestCard => &output.controls.card,
    };
    let on = !flag.fetch_xor(true, Ordering::Relaxed);
    log::info!(
        "video output: {control:?} {}",
        if on { "on" } else { "off" }
    );
    output.output_state(&app)
}

/// Puts the output full screen on display `display` (from 1), or in a window when there is none.
#[tauri::command]
pub async fn video_output_fullscreen(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    display: Option<u32>,
) -> Result<OutputState, String> {
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        output.set_fullscreen(&worker_app, display)?;
        Ok(output.output_state(&worker_app))
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Saves a PNG of the composition in the snapshot folder of the preferences.
#[tauri::command]
pub async fn video_output_snapshot(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
) -> Result<Snapshot, String> {
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = preferences::preferences_get(
            worker_app.clone(),
            worker_app.state::<preferences::Store>(),
        )?
        .snapshot_dir;
        let data = worker_app
            .path()
            .app_data_dir()
            .map_err(|err| err.to_string())?;
        let dir = crate::snapshot::folder(chosen.as_deref(), dirs::picture_dir(), &data);
        let saved = output.snapshot(&worker_app, &dir);
        match &saved {
            Ok(shot) => log::info!("video output: snapshot saved to {}", shot.path),
            Err(err) => log::warn!("video output: snapshot failed: {err}"),
        }
        saved
    })
    .await
    .map_err(|err| err.to_string())?
}

/// The test cards of a screen's slices laid out on the composition, small, as the Advanced Output
/// shows them over the picture: the width and height as two little-endian u16, then RGBA, clear
/// where no card is shown. Empty while no card is.
#[tauri::command]
pub async fn video_card_preview(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    screen: String,
) -> Result<tauri::ipc::Response, String> {
    let output = Arc::clone(&output);
    let bytes = tauri::async_runtime::spawn_blocking(move || output.card_preview(&app, &screen))
        .await
        .map_err(|err| err.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

impl VideoOutputs {
    fn card_preview(&self, app: &AppHandle, screen: &str) -> Vec<u8> {
        let session = Arc::clone(&app.state::<SharedSession>());
        let size = composition_size(&session);
        let screens = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let Some(found) = screens.iter().find(|s| s.id == screen) else {
            return Vec::new();
        };
        let everywhere =
            self.controls.card.load(Ordering::Relaxed) || self.pattern.load(Ordering::Relaxed);
        let mut slices: Vec<CardSlice> = found
            .slices
            .iter()
            .filter(|slice| !slice.hidden)
            .map(card_slice)
            .collect();
        if slices.is_empty() {
            slices.push(CardSlice {
                name: found.name.clone(),
                input: [0.0, 0.0, 1.0, 1.0],
                card: Default::default(),
                logo: None,
            });
        }
        if let Ok(cache) = crate::video_commands::video_cache(app) {
            with_logos(&mut slices, &session, &cache);
        }
        let cards = flux_render::pattern::layout(&slices, size, &found.name);
        let shown: Vec<_> = cards
            .into_iter()
            .zip(&slices)
            .filter(|(_, slice)| everywhere || slice.card.show)
            .map(|(card, _)| card)
            .collect();
        if shown.is_empty() {
            return Vec::new();
        }
        let (width, height, rgba) = flux_render::pattern::preview(&shown, size, 640);
        let mut out = Vec::with_capacity(rgba.len() + 4);
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.extend_from_slice(&rgba);
        out
    }
}

/// The colour (`#rrggbb`) the test card of the slice at `index` takes when it names none.
#[tauri::command]
pub fn video_card_base(index: usize) -> String {
    let [r, g, b] = flux_render::palette::default_base(index);
    format!("#{r:02x}{g:02x}{b:02x}")
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
        B::Difference => flux_render::Blend::Difference,
        B::Exclusion => flux_render::Blend::Exclusion,
        B::Overlay => flux_render::Blend::Overlay,
        B::SoftLight => flux_render::Blend::SoftLight,
        B::HardLight => flux_render::Blend::HardLight,
        B::ColorDodge => flux_render::Blend::ColorDodge,
        B::ColorBurn => flux_render::Blend::ColorBurn,
        B::LinearLight => flux_render::Blend::LinearLight,
        B::VividLight => flux_render::Blend::VividLight,
        B::PinLight => flux_render::Blend::PinLight,
        B::Hue => flux_render::Blend::Hue,
        B::Saturation => flux_render::Blend::Saturation,
        B::Color => flux_render::Blend::Color,
        B::Luminosity => flux_render::Blend::Luminosity,
        B::AlphaCut => flux_render::Blend::AlphaCut,
        B::LumaKey => flux_render::Blend::LumaKey,
        B::LumaKeyInverted => flux_render::Blend::LumaKeyInverted,
        B::LumaIsAlpha => flux_render::Blend::LumaIsAlpha,
        B::DifferenceInverted => flux_render::Blend::DifferenceInverted,
        B::Mask50 => flux_render::Blend::Mask50,
        B::Rgb => flux_render::Blend::Rgb,
        B::Displace => flux_render::Blend::Displace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: &str) -> Display {
        Display {
            id: id.into(),
            width: 1920,
            height: 1080,
            main: false,
        }
    }

    #[test]
    fn display_at_should_count_from_one_in_the_order_of_the_system() {
        let list = [display("a"), display("b")];
        assert_eq!(display_at(&list, 1).map(|d| d.id.as_str()), Some("a"));
        assert_eq!(display_at(&list, 2).map(|d| d.id.as_str()), Some("b"));
        assert!(display_at(&list, 0).is_none());
        assert!(display_at(&list, 3).is_none());
    }

    #[test]
    fn controls_should_start_off_and_flip_one_flag_at_a_time() {
        let controls = Controls::default();
        assert!(!controls.disabled.load(Ordering::Relaxed));
        assert!(!controls.card.load(Ordering::Relaxed));
        assert!(!controls.disabled.fetch_xor(true, Ordering::Relaxed));
        assert!(controls.disabled.load(Ordering::Relaxed));
        assert!(!controls.card.load(Ordering::Relaxed));
    }

    #[test]
    fn card_caption_should_name_the_screen_and_the_window_size() {
        assert_eq!(card_caption("Main", 1920, 1080), "Main  1920 x 1080");
    }
    use flux_project::playhead::Tick;
    use flux_project::vj::{Blend, Clip, Layer, Playback, Side, Trigger, Vj};
    use flux_project::{VideoFit, vj::Direction};
    use flux_video::{GridPlay, Rate, Shown};

    fn clip(id: &str) -> Clip {
        Clip {
            id: id.into(),
            column: Some("c1".into()),
            slot: None,
            asset: "v1".into(),
            generator: None,
            name: None,
            playback: Playback::Loop,
            trigger: Trigger::Normal,
            speed: 1.0,
            duration: flux_project::vj::DEFAULT_DURATION,
            direction: Direction::default(),
            tempo: Default::default(),
            region: Default::default(),
            cues: Default::default(),
            fit: VideoFit::Fill,
            scale: 0.5,
            position: [0.1, 0.0],
            effects: Vec::new(),
            modulation: Default::default(),
        }
    }

    fn layer(id: &str, clips: Vec<Clip>) -> Layer {
        Layer {
            id: id.into(),
            name: None,
            global: false,
            master: 0.4,
            opacity: 0.3,
            volume: 1.0,
            blend: Blend::Add,
            transition: 2.0,
            transition_effect: Default::default(),
            autopilot: Default::default(),
            crossfade: Some(Side::A),
            bypass: true,
            solo: false,
            collapsed: false,
            effects: Vec::new(),
            modulation: Default::default(),
            clips,
        }
    }

    fn grid() -> Vj {
        Vj {
            layers: vec![
                layer("l1", vec![clip("a"), clip("b")]),
                layer("l2", vec![clip("c")]),
            ],
            ..Default::default()
        }
    }

    fn shown(vj: &Vj, play: &GridPlay, tick: Tick) -> Vec<Shown> {
        let probe = |_: &str| Some((Rate::new(25, 1), 100));
        play.layers(vj, tick, &probe)
            .into_iter()
            .map(|layer| Shown {
                frame: Some(Arc::new(flux_video::Frame {
                    index: layer.frame,
                    width: 1920,
                    height: 800,
                    rgba: Vec::new(),
                    alpha: false,
                })),
                layer,
            })
            .collect()
    }

    #[test]
    fn preview_grid_should_keep_only_the_clip_at_full_strength() {
        let vj = preview_grid(&grid(), "a").unwrap();
        assert_eq!(vj.layers.len(), 1);
        assert_eq!(vj.layers[0].clips.len(), 1);
        assert!(!vj.layers[0].bypass && vj.layers[0].crossfade.is_none());
        assert!(preview_grid(&grid(), "nope").is_none());
    }

    #[test]
    fn preview_should_place_a_clip_as_the_output_does() {
        let size = (1920, 1080);
        let tick = Tick::default();
        let mut on_air = grid();
        on_air.layers[0] = Layer {
            bypass: false,
            crossfade: None,
            opacity: 1.0,
            master: 1.0,
            transition: 0.0,
            blend: Blend::Alpha,
            ..on_air.layers[0].clone()
        };
        let mut play = GridPlay::default();
        play.launch(&on_air, "a", tick);
        let output = shown(&on_air, &play, tick);

        let cut = preview_grid(&grid(), "a").unwrap();
        let mut preview_play = GridPlay::default();
        preview_play.launch(&cut, "a", tick);
        let preview = shown(&cut, &preview_play, tick);

        assert_eq!(preview.len(), 1, "a bypassed layer still previews");
        let (p, o) = (
            &pictures(&preview, size, 0.0)[0],
            &pictures(&output, size, 0.0)[0],
        );
        assert_eq!(p.place, o.place);
        assert_eq!(p.opacity, 1.0);
    }

    #[test]
    fn preview_should_start_at_the_clip_start_and_again_when_launched_again() {
        let cut = preview_grid(&grid(), "a").unwrap();
        let mut play = GridPlay::default();
        let later = Tick {
            time: Duration::from_secs(1),
            beats: 2.0,
        };
        play.launch(&cut, "a", Tick::default());
        assert!(shown(&cut, &play, later)[0].layer.frame > 0, "it plays");
        play.launch(&cut, "a", later);
        assert_eq!(shown(&cut, &play, later)[0].layer.frame, 0, "it restarts");
    }
}
