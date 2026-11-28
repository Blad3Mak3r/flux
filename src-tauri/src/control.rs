//! Remote control: Flux obeys a MIDI controller (pads, buttons) and OSC messages (a Stream Deck),
//! so a show can be run from a surface without touching the window.
//!
//! Flux stays the master clock: this only moves the transport. The choices are the machine's, not
//! the project's (a pad layout follows the controller, not the show), so they live in
//! `control.json` in the app data folder, next to the graphics preferences. A missing file or a
//! port that is not connected is a state to show, never an error: a show must not fail to start
//! because a controller is unplugged.
//!
//! The inputs live as long as the app, not as long as an output: the output is closed and
//! reopened on every timeline change. The decoding and mapping are in [`flux_engine::control`] and
//! the sockets in [`flux_engine::control_in`]; what is here is the lifecycle, the preferences and
//! the execution. Executing needs the running output, the session and the window, so it is done on
//! a thread of its own (`flux-control`), the only one that logs.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::SharedSession;
use crate::graphics::IDENTIFIER;
use crate::transport_commands::{
    SharedOutput, TIMELINE_ADVANCED, is_playing, light_snapshot, release, snapshot,
};
use crate::vj_commands::SharedGrid;
use crate::vj_control::{SharedLearn, VjRunner};
use flux_engine::control::KeyMsg;
use flux_engine::control::{
    Binding, ControlAction, Feedback, MidiTrigger, action_for, dedup, osc_action, seek_target,
};
use flux_engine::control_in::{ARTNET_UDP_PORT, ArtnetListener, Input, ListenError, MidiListener};
use flux_engine::control_light::{Facts, Lamps, LightPort, by_port};
use flux_engine::devices::{self, MidiPort};
use flux_engine::output::OutputError;
use flux_engine::transport::{Command, TransportState};
use flux_project::vj_feedback::{self, Lights as PadLights, Pad};
use flux_project::vj_mapping::{Command as VjCommand, Target};

const FILE_NAME: &str = "control.json";

/// UDP port the OSC input listens on until the user picks another.
pub const DEFAULT_OSC_PORT: u16 = 9000;

/// The event the window hears for what only it can do: open the output to play, switch the view.
pub const CONTROL_ACTION: &str = "control-action";

/// The event with the trigger the user touched while learning a binding.
pub const CONTROL_LEARNED: &str = "control-learned";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MidiPrefs {
    pub enabled: bool,
    /// The name hint of the input port, as a project keeps those of its outputs.
    pub port_name: Option<String>,
    /// Which of the ports with that name, in list order.
    pub port_index: usize,
    pub bindings: Vec<Binding>,
    /// Light the pads of the controller with the state of the show. Off until the user asks: the
    /// output port of the controller may be in use by something else.
    pub feedback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OscPrefs {
    pub bind_address: String,
    pub tcp_enabled: bool,
    pub enabled: bool,
    pub port: u16,
}

impl Default for OscPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_OSC_PORT,
            bind_address: "0.0.0.0".into(),
            tcp_enabled: true,
        }
    }
}

/// The Art-Net (DMX) input. The port is the protocol's own, 6454.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtnetPrefs {
    pub enabled: bool,
    /// Only this Port-Address (0 to 32767) is heard; absent hears every universe.
    pub universe: Option<u16>,
    /// The interface to listen on; `0.0.0.0` is all of them.
    pub bind_address: String,
}

impl Default for ArtnetPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            universe: None,
            bind_address: "0.0.0.0".into(),
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ControlPrefs {
    pub midi: MidiPrefs,
    pub osc: OscPrefs,
    pub artnet: ArtnetPrefs,
}

/// Whether an action makes sense: a number is a place from 1 and a step moves.
fn valid(action: ControlAction) -> bool {
    match action {
        ControlAction::GotoCue(n) | ControlAction::SelectTimeline(n) => n >= 1,
        ControlAction::StepFrames(n) | ControlAction::StepSeconds(n) => n != 0,
        _ => true,
    }
}

fn valid_trigger(trigger: MidiTrigger) -> bool {
    (1..=16).contains(&trigger.channel) && trigger.number < 128
}

/// `prefs` as they can be kept: a file edited by hand or by another version may hold anything.
pub fn normalize(mut prefs: ControlPrefs) -> ControlPrefs {
    prefs
        .midi
        .bindings
        .retain(|binding| valid(binding.action) && valid_trigger(binding.trigger));
    for binding in &mut prefs.midi.bindings {
        binding.feedback = binding.feedback.filter(|feedback| feedback.is_valid());
    }
    prefs.midi.bindings = dedup(prefs.midi.bindings);
    prefs.midi.port_name = prefs.midi.port_name.filter(|name| !name.trim().is_empty());
    if prefs.osc.port == 0 {
        prefs.osc.port = DEFAULT_OSC_PORT;
    }
    prefs.artnet.universe = prefs
        .artnet
        .universe
        .filter(|universe| *universe <= flux_project::vj_mapping::MAX_UNIVERSE);
    prefs.artnet.bind_address = prefs.artnet.bind_address.trim().to_owned();
    if prefs.artnet.bind_address.is_empty() {
        prefs.artnet.bind_address = ArtnetPrefs::default().bind_address;
    }
    prefs
}

fn prefs_file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(IDENTIFIER).join(FILE_NAME))
}

/// The preferences in `file`, or the defaults when there are none or they cannot be read.
fn load(file: &Path) -> ControlPrefs {
    let prefs = fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    normalize(prefs)
}

/// Writes through a temporary file, so a failed write leaves the old preferences as they were.
fn save(file: &Path, prefs: &ControlPrefs) -> io::Result<()> {
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

/// Whether an input is listening, and if not, why.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "camelCase")]
pub enum LinkState {
    /// Switched off, or on without a port chosen.
    #[default]
    Off,
    /// The port's name, or the UDP port.
    Listening(String),
    /// The chosen MIDI port is not connected.
    Missing,
    Error(String),
}

/// What the preferences page shows: the choices, how each input is doing and what can be chosen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlView {
    pub prefs: ControlPrefs,
    pub midi: LinkState,
    pub osc: LinkState,
    pub artnet: LinkState,
    /// How many ArtDmx packets arrived since the input was opened.
    pub artnet_packets: u64,
    pub inputs: Vec<MidiPort>,
    /// Why MIDI cannot be listed, on a system without it.
    pub midi_error: Option<String>,
    /// The values a binding without its own lights its pad with, for the chosen controller.
    pub feedback_default: Feedback,
}

/// The inputs that are open and the thread that executes what they receive. Dropping it closes
/// the inputs first, which ends the thread.
#[derive(Default)]
struct Running {
    midi: Option<MidiListener>,
    osc: Option<crate::osc_api::OscServer>,
    artnet: Option<ArtnetListener>,
    /// Where the window's keys go. The worker ends when this and the inputs are dropped.
    keys: Option<mpsc::Sender<Input>>,
    worker: Option<JoinHandle<()>>,
    lights: Option<Lights>,
}

impl Drop for Running {
    fn drop(&mut self) {
        // Out first: the pads are put out while the port is still ours.
        self.lights = None;
        self.midi = None;
        self.osc = None;
        self.artnet = None;
        self.keys = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct Engine {
    prefs: ControlPrefs,
    running: Running,
    midi: LinkState,
    osc: LinkState,
    artnet: LinkState,
}

#[derive(Default)]
pub struct Control {
    engine: Mutex<Engine>,
    /// The next trigger is the one to learn: it is told to the window instead of executed.
    learning: Arc<AtomicBool>,
    /// The target of a VJ mapping the next control touched is to be learned for (see
    /// [`crate::vj_control`]).
    vj_learn: SharedLearn,
}

pub type SharedControl = Arc<Control>;

impl Control {
    /// What the window is waiting to map, while it is in map mode.
    pub fn vj_learn(&self) -> &SharedLearn {
        &self.vj_learn
    }

    /// Sends a key of the window to the thread that runs the mappings.
    pub fn key(&self, message: KeyMsg) {
        let keys = self.lock().running.keys.clone();
        if let Some(keys) = keys {
            // The thread is gone while the inputs are being replaced.
            let _ = keys.send(Input::Key(message));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn view(&self) -> ControlView {
        let (inputs, midi_error) = match devices::list_midi_inputs() {
            Ok(ports) => (ports, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        };
        let engine = self.lock();
        ControlView {
            feedback_default: feedback_default(&engine.prefs),
            prefs: engine.prefs.clone(),
            midi: engine.midi.clone(),
            osc: engine.osc.clone(),
            artnet: engine.artnet.clone(),
            artnet_packets: engine
                .running
                .artnet
                .as_ref()
                .map_or(0, ArtnetListener::packets),
            inputs,
            midi_error,
        }
    }

    /// Makes `prefs` the ones in use: keeps them and opens the inputs they ask for. The previous
    /// inputs are closed first, so that the same UDP port can be bound again.
    fn apply(&self, app: &AppHandle, prefs: ControlPrefs) {
        let prefs = normalize(prefs);
        match prefs_file().map(|file| save(&file, &prefs)) {
            Some(Err(err)) => log::warn!("the remote control preferences were not saved: {err}"),
            None => log::warn!("the remote control preferences were not saved: no data folder"),
            Some(Ok(())) => {}
        }
        self.learning.store(false, Ordering::Relaxed);
        let mut engine = self.lock();
        engine.running = Running::default();
        let (running, midi, osc, artnet) = open(app, &prefs, &self.learning, &self.vj_learn);
        *engine = Engine {
            prefs,
            running,
            midi,
            osc,
            artnet,
        };
    }
}

/// Opens the inputs `prefs` ask for and starts the thread that executes them. The state of each
/// is reported and logged here, on the caller's thread.
fn open(
    app: &AppHandle,
    prefs: &ControlPrefs,
    learning: &Arc<AtomicBool>,
    vj_learn: &SharedLearn,
) -> (Running, LinkState, LinkState, LinkState) {
    let (tx, rx) = mpsc::channel();
    let mut running = Running::default();

    let midi = match (&prefs.midi.enabled, &prefs.midi.port_name) {
        (true, Some(name)) => match MidiListener::open(name, prefs.midi.port_index, tx.clone()) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to the MIDI port {}",
                    listener.name()
                );
                let state = LinkState::Listening(listener.name().to_owned());
                running.midi = Some(listener);
                state
            }
            Err(ListenError::Missing) => {
                log::warn!("remote control: the MIDI port {name} is not connected");
                LinkState::Missing
            }
            Err(err @ ListenError::Unavailable(_)) => {
                log::warn!("remote control: {err}");
                LinkState::Error(err.to_string())
            }
        },
        _ => LinkState::Off,
    };

    let osc = if prefs.osc.enabled {
        match crate::osc_api::OscServer::bind(
            app.clone(),
            &prefs.osc.bind_address,
            prefs.osc.port,
            prefs.osc.tcp_enabled,
            tx.clone(),
        ) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to OSC on UDP port {}",
                    listener.port()
                );
                let state = LinkState::Listening(listener.port().to_string());
                running.osc = Some(listener);
                state
            }
            Err(err) => {
                log::warn!(
                    "remote control: OSC port {} is not available: {err}",
                    prefs.osc.port
                );
                LinkState::Error(err.to_string())
            }
        }
    } else {
        LinkState::Off
    };

    let artnet = if prefs.artnet.enabled {
        match ArtnetListener::bind(
            &prefs.artnet.bind_address,
            ARTNET_UDP_PORT,
            prefs.artnet.universe,
            tx.clone(),
        ) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to Art-Net on {}:{}",
                    prefs.artnet.bind_address,
                    listener.port()
                );
                let state = LinkState::Listening(format!(
                    "{}:{}",
                    prefs.artnet.bind_address,
                    listener.port()
                ));
                running.artnet = Some(listener);
                state
            }
            Err(err) => {
                log::warn!("remote control: Art-Net is not available: {err}");
                LinkState::Error(err.to_string())
            }
        }
    } else {
        LinkState::Off
    };

    if prefs.midi.enabled && prefs.midi.feedback {
        running.lights = prefs.midi.port_name.as_ref().and_then(|name| {
            Lights::start(
                app.clone(),
                name.clone(),
                prefs.midi.port_index,
                prefs.midi.bindings.clone(),
            )
        });
    }

    // The inputs and `running.keys` hold the only senders, so the thread ends when they are
    // dropped. It always runs: the keyboard is an input that needs no preferences.
    running.keys = Some(tx);
    {
        let port = match &midi {
            LinkState::Listening(name) => name.clone(),
            _ => String::new(),
        };
        let dispatcher = Dispatcher {
            app: app.clone(),
            bindings: prefs.midi.bindings.clone(),
            learning: Arc::clone(learning),
            vj: VjRunner::new(app.clone(), Arc::clone(vj_learn), port),
        };
        running.worker = thread::Builder::new()
            .name("flux-control".into())
            .spawn(move || dispatcher.run(&rx))
            .inspect_err(|err| log::warn!("remote control: the thread did not start: {err}"))
            .ok();
    }
    (running, midi, osc, artnet)
}

/// The values a binding without its own lights its pad with: those of the controller's preset.
fn feedback_default(prefs: &ControlPrefs) -> Feedback {
    prefs
        .midi
        .port_name
        .as_deref()
        .map_or(Feedback::GENERIC, Feedback::preset)
}

/// How often the state of the show is looked at. Only a change is sent to the controller; the
/// look itself is a few atomic reads.
const LIGHTS_INTERVAL: Duration = Duration::from_millis(50);

/// How long a lit pad that blinks stays in each half of the blink.
const BLINK_HALF: Duration = Duration::from_millis(500);

/// How long to wait before trying to open the controller's output port again.
const LIGHTS_RETRY: Duration = Duration::from_secs(2);

/// The thread that lights the pads of the controller. Dropping it stops the thread, which puts
/// the pads out first.
struct Lights {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Lights {
    fn start(app: AppHandle, port: String, index: usize, bindings: Vec<Binding>) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("flux-control-lights".into())
            .spawn({
                let stop = Arc::clone(&stop);
                move || run_lights(&app, &port, index, &bindings, &stop)
            })
            .inspect_err(|err| log::warn!("remote control: the lights thread did not start: {err}"))
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Lights {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A port the pads of the VJ mappings are lit through, found by the name of the input the control
/// was learned on.
struct PadPort {
    port: Option<LightPort>,
    lamps: Lamps,
    retry_at: Instant,
}

/// The pads of the VJ mappings with what they show now, and the colours of the grid. Empty with no
/// VJ project open, which puts every pad out. The playing clips are read first and the session
/// is only tried, so the window never waits for the lights.
fn vj_pads(app: &AppHandle) -> (Vec<Pad>, Option<PadLights>) {
    let (playing, paused) = {
        let grid = app.state::<SharedGrid>();
        let grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
        let clips: HashSet<String> = grid
            .play
            .playing()
            .values()
            .filter_map(|launched| launched.clip.clone())
            .collect();
        (clips, grid.paused())
    };
    let session = app.state::<SharedSession>();
    let Ok(session) = session.try_lock() else {
        return (Vec::new(), None);
    };
    session.vj_project().map_or((Vec::new(), None), |project| {
        (
            vj_feedback::pads(&project.vj, &|id| playing.contains(id), paused),
            project.vj.lights,
        )
    })
}

/// Lights the pads of the VJ mappings on every port they were learned on, writing only what
/// changed. A port is opened when a pad needs it and closed, its pads put out, when none does; one
/// that fails is reopened after [`LIGHTS_RETRY`] and gets the whole state again.
fn light_vj_pads(
    app: &AppHandle,
    default_port: &str,
    ports: &mut HashMap<String, PadPort>,
    failed_sends: &mut u64,
) {
    let (pads, grid_lights) = vj_pads(app);
    let groups = by_port(&pads, default_port);
    for (hint, group) in &groups {
        let entry = ports.entry(hint.clone()).or_insert_with(|| PadPort {
            port: None,
            lamps: Lamps::default(),
            retry_at: Instant::now(),
        });
        if entry.port.is_none() && Instant::now() >= entry.retry_at {
            match LightPort::open(hint, 0) {
                Ok(opened) => {
                    log::info!(
                        "remote control: lighting the VJ pads through {}",
                        opened.name()
                    );
                    entry.lamps.forget();
                    entry.port = Some(opened);
                }
                Err(err) => {
                    log::warn!("remote control: the VJ pads cannot be lit on {hint}: {err}");
                    entry.retry_at = Instant::now() + LIGHTS_RETRY;
                }
            }
        }
        let Some(open) = entry.port.as_mut() else {
            continue;
        };
        let base = grid_lights.unwrap_or_else(|| PadLights::preset(hint));
        let failed = entry.lamps.update_pads(group, base, open);
        if failed > 0 {
            *failed_sends += failed as u64;
            log::warn!(
                "remote control: {failed} VJ lights could not be sent ({failed_sends} so far), reopening {hint}"
            );
            entry.port = None;
            entry.retry_at = Instant::now() + LIGHTS_RETRY;
        }
    }
    ports.retain(|hint, entry| {
        if groups.contains_key(hint) {
            return true;
        }
        if let Some(open) = entry.port.as_mut() {
            entry.lamps.clear(open);
        }
        false
    });
}

/// Looks at the state of the show, from the same sources as the Live view, and writes to the
/// controller the pads that changed. It never touches the clock: it only reads the output's
/// published state, and a port that fails is counted, logged here (off the output path, and not
/// more than once per failure) and reopened.
fn run_lights(app: &AppHandle, name: &str, index: usize, bindings: &[Binding], stop: &AtomicBool) {
    let preset = Feedback::preset(name);
    let mut lamps = Lamps::default();
    let mut port: Option<LightPort> = None;
    let mut retry_at = Instant::now();
    let mut failed_sends = 0_u64;
    let mut vj_ports: HashMap<String, PadPort> = HashMap::new();
    let started = Instant::now();
    // The session is only tried: if the window holds it, the last known values do.
    let mut timeline = None;
    let mut timecode = None;

    while !stop.load(Ordering::Relaxed) {
        if port.is_none() && Instant::now() >= retry_at {
            match LightPort::open(name, index) {
                Ok(opened) => {
                    log::info!(
                        "remote control: lighting the pads through {}",
                        opened.name()
                    );
                    lamps.forget();
                    port = Some(opened);
                }
                Err(err) => {
                    log::warn!("remote control: the lights cannot be sent: {err}");
                    retry_at = Instant::now() + LIGHTS_RETRY;
                }
            }
        }
        if let Some(open) = port.as_mut() {
            if let Ok(session) = app.state::<SharedSession>().try_lock() {
                timeline = session.timeline_place();
                timecode = session.active_timecode();
            }
            let snapshot = light_snapshot(&app.state::<SharedOutput>());
            let (position, cues, show) = match &snapshot {
                Some((position, cues, show)) => (Some(position), cues.cues(), *show),
                None => (None, &[][..], None),
            };
            let facts = Facts {
                transport: position.map(|position| position.state),
                position: position.map_or(0, |position| position.frame),
                cues,
                start: timecode.as_ref().map_or(0, |timecode| timecode.start),
                fps: timecode
                    .as_ref()
                    .map_or(0, |timecode| timecode.frame_rate.nominal_fps()),
                show,
                timeline,
            };
            let blink_on =
                (started.elapsed().as_millis() / BLINK_HALF.as_millis()).is_multiple_of(2);
            let failed = lamps.update(bindings, preset, &facts, blink_on, open);
            if failed > 0 {
                failed_sends += failed as u64;
                log::warn!(
                    "remote control: {failed} lights could not be sent ({failed_sends} so far), reopening the port"
                );
                port = None;
                retry_at = Instant::now() + LIGHTS_RETRY;
            }
        }
        light_vj_pads(app, name, &mut vj_ports, &mut failed_sends);
        thread::sleep(LIGHTS_INTERVAL);
    }

    if let Some(open) = port.as_mut() {
        lamps.clear(open);
    }
    for entry in vj_ports.values_mut() {
        if let Some(open) = entry.port.as_mut() {
            entry.lamps.clear(open);
        }
    }
}

/// What only the window can do, told to it with [`CONTROL_ACTION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum WindowAction {
    /// Play with no output open: the window opens it, as its own play button does.
    Play,
    ViewEdit,
    ViewLive,
}

/// How a timeline action picks the timeline.
#[derive(Debug, Clone, Copy)]
enum Pick {
    Step(isize),
    Place(usize),
}

struct Dispatcher {
    app: AppHandle,
    bindings: Vec<Binding>,
    learning: Arc<AtomicBool>,
    vj: VjRunner,
}

impl Dispatcher {
    /// Handles what arrives until the inputs are closed. Messages that come together (a knob
    /// turned fast) are taken as one batch, so only where a control ended is applied; while a
    /// moving control is being followed the wait is cut short to send its state on time.
    fn run(mut self, inputs: &Receiver<Input>) {
        loop {
            let first = match self.vj.wait() {
                Some(wait) => match inputs.recv_timeout(wait) {
                    Ok(input) => Some(input),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match inputs.recv() {
                    Ok(input) => Some(input),
                    Err(_) => break,
                },
            };
            if let Some(first) = first {
                let mut commands = Vec::new();
                self.handle(first, &mut commands);
                while let Ok(input) = inputs.try_recv() {
                    self.handle(input, &mut commands);
                }
                self.vj.run(commands);
            }
            self.vj.tick();
        }
        // The gesture the last message opened is still the user's.
        self.vj.finish();
    }

    /// A VJ show takes what its mappings are the source of; anything else goes to the cue
    /// bindings and the fixed `/flux/...` addresses, as before.
    fn handle(&mut self, input: Input, commands: &mut Vec<(Target, VjCommand)>) {
        match input {
            Input::Midi(message) => {
                if self.vj.midi(&message, commands) {
                    return;
                }
                let Some(trigger) = message.trigger() else {
                    return;
                };
                if self.learning.swap(false, Ordering::Relaxed) {
                    log::info!("remote control: learned {trigger:?}");
                    let _ = self.app.emit(CONTROL_LEARNED, trigger);
                } else if let Some(action) = action_for(&self.bindings, trigger) {
                    self.execute(action);
                } else {
                    log::debug!("remote control: {trigger:?} has no action");
                }
            }
            Input::Dmx(frame) => self.vj.dmx(&frame, commands),
            Input::Key(message) => self.vj.key(&message, commands),
            Input::Osc(message) => {
                if self.vj.osc(&message, commands) {
                    return;
                }
                if let Some(action) = osc_action(&message) {
                    self.execute(action);
                }
            }
        }
    }

    fn execute(&self, action: ControlAction) {
        log::debug!("remote control: {action:?}");
        let output = self.app.state::<SharedOutput>();
        match action {
            ControlAction::Go | ControlAction::Devamp | ControlAction::CancelDevamp => {
                let action = match action {
                    ControlAction::Go => crate::show_commands::Action::Go,
                    ControlAction::Devamp => crate::show_commands::Action::Devamp,
                    _ => crate::show_commands::Action::CancelDevamp,
                };
                if let Err(e) = crate::show_commands::execute(&self.app, &output, action) {
                    log::warn!("show control failed: {e}");
                }
            }
            ControlAction::Play => self.transport(&output, Command::Play, true),
            ControlAction::Pause => self.transport(&output, Command::Pause, false),
            ControlAction::Stop => self.transport(&output, Command::Stop, false),
            ControlAction::Toggle => {
                let playing = snapshot(&output)
                    .is_some_and(|(position, _)| position.state == TransportState::Playing);
                if playing {
                    self.transport(&output, Command::Pause, false);
                } else {
                    self.transport(&output, Command::Play, true);
                }
            }
            ControlAction::NextCue
            | ControlAction::PrevCue
            | ControlAction::GotoCue(_)
            | ControlAction::StepFrames(_)
            | ControlAction::StepSeconds(_) => self.seek(&output, action),
            ControlAction::NextTimeline => self.timeline(&output, Pick::Step(1)),
            ControlAction::PrevTimeline => self.timeline(&output, Pick::Step(-1)),
            ControlAction::SelectTimeline(place) => {
                self.timeline(&output, Pick::Place(place as usize));
            }
            ControlAction::Panic => {
                self.transport(&output, Command::Stop, false);
                if let Err(err) = crate::live_commands::panic_from_app(&self.app) {
                    log::warn!("remote control: panic failed: {err}");
                }
            }
            ControlAction::ViewEdit => self.tell(WindowAction::ViewEdit),
            ControlAction::ViewLive => self.tell(WindowAction::ViewLive),
        }
    }

    fn tell(&self, action: WindowAction) {
        if let Err(err) = self.app.emit(CONTROL_ACTION, action) {
            log::warn!("remote control: the window could not be told: {err}");
        }
    }

    /// Sends `command` to the running output. With none running, only play does something: the
    /// window opens the output, which it owns, and plays.
    fn transport(&self, output: &SharedOutput, command: Command, open: bool) {
        match crate::voice_commands::send_transport(
            &self.app.state::<Arc<crate::voice_commands::VoiceState>>(),
            output,
            command,
        ) {
            Ok(()) => {}
            Err(OutputError::NotRunning) if open => self.tell(WindowAction::Play),
            Err(OutputError::NotRunning) => log::debug!("remote control: no output is open"),
            Err(err) => log::warn!("remote control: {command:?} failed: {err}"),
        }
    }

    fn seek(&self, output: &SharedOutput, action: ControlAction) {
        let Some((position, cues)) = snapshot(output) else {
            log::debug!("remote control: no output is open");
            return;
        };
        let timecode = self
            .app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active_timecode();
        let Some(timecode) = timecode else {
            log::debug!("remote control: no project is open");
            return;
        };
        let fps = timecode.frame_rate.nominal_fps();
        match seek_target(action, position.frame, cues.cues(), timecode.start, fps) {
            Some(frame) => {
                if let Err(err) = crate::voice_commands::send_transport(
                    &self.app.state::<Arc<crate::voice_commands::VoiceState>>(),
                    output,
                    Command::Seek(frame),
                ) {
                    log::warn!("remote control: seek failed: {err}");
                }
            }
            None => log::info!("remote control: {action:?} has nowhere to go"),
        }
    }

    /// Changes the active timeline as the window's own command does: not while the show plays, and
    /// the output is released because it was opened with the old timecode. The window is told with
    /// the event it already hears when a show ends and the next timeline takes over.
    fn timeline(&self, output: &SharedOutput, pick: Pick) {
        if is_playing(output) {
            log::info!("remote control: the timelines cannot change while the show plays");
            return;
        }
        let changed = {
            let session = self.app.state::<SharedSession>();
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            match pick {
                Pick::Step(delta) => session.step_timeline(delta),
                Pick::Place(place) => session.select_timeline_at(place),
            }
        };
        match changed {
            Ok(Some(state)) => {
                tauri::async_runtime::block_on(release(output));
                let _ = self.app.emit(TIMELINE_ADVANCED, state);
            }
            Ok(None) => log::info!("remote control: there is no such timeline"),
            Err(err) => log::warn!("remote control: the timeline could not be changed: {err}"),
        }
    }
}

/// Opens the inputs the saved preferences ask for. Called once the app is up.
pub fn start(app: &AppHandle) {
    let prefs = prefs_file().map_or_else(ControlPrefs::default, |file| load(&file));
    let control = Arc::clone(&app.state::<SharedControl>());
    control.apply(app, prefs);
}

/// Opening a MIDI port can block, so the commands run off the main thread.
async fn blocking<T: Send + 'static>(op: impl FnOnce() -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(op)
        .await
        .expect("the remote control task panicked")
}

#[tauri::command]
pub async fn control_get(control: State<'_, SharedControl>) -> Result<ControlView, String> {
    let control = Arc::clone(&control);
    Ok(blocking(move || control.view()).await)
}

/// Sends the view down `on_view` whenever it changes (the Art-Net packet counter moves by itself),
/// until [`crate::watch::watch_stop`].
#[tauri::command]
pub fn control_watch(
    control: State<'_, SharedControl>,
    watches: State<'_, crate::watch::Watches>,
    on_view: tauri::ipc::Channel<ControlView>,
) {
    let control = Arc::clone(&control);
    let mut changes = crate::watch::Changes::default();
    watches.start(on_view, Duration::from_secs(1), move || {
        changes.next(control.view())
    });
}

/// Keeps `prefs`, opens the inputs they ask for and answers with how that went. Sending the same
/// preferences again retries a port that was not connected.
#[tauri::command]
pub async fn control_set(
    app: AppHandle,
    control: State<'_, SharedControl>,
    prefs: ControlPrefs,
) -> Result<ControlView, String> {
    let control = Arc::clone(&control);
    Ok(blocking(move || {
        control.apply(&app, prefs);
        control.view()
    })
    .await)
}

/// A key the window heard in a VJ show, pressed or let go, for the mappings (or to learn).
#[tauri::command]
pub fn control_key(control: State<'_, SharedControl>, key: KeyMsg) {
    control.key(key);
}

/// Starts or cancels waiting for a trigger to learn: the next one is sent to the window with
/// [`CONTROL_LEARNED`] and not executed.
#[tauri::command]
pub fn control_learn(control: State<'_, SharedControl>, on: bool) {
    control.learning.store(on, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_engine::control::TriggerKind;
    use tempfile::TempDir;

    fn binding(number: u8, action: ControlAction) -> Binding {
        Binding {
            trigger: MidiTrigger {
                kind: TriggerKind::Note,
                channel: 1,
                number,
            },
            action,
            feedback: None,
        }
    }

    #[test]
    fn the_defaults_should_listen_to_nothing() {
        let prefs = ControlPrefs::default();
        assert!(!prefs.midi.enabled);
        assert!(!prefs.osc.enabled);
        assert!(!prefs.midi.feedback, "the lights are off until asked for");
        assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
        assert!(prefs.midi.bindings.is_empty());
    }

    #[test]
    fn normalize_should_drop_what_cannot_work() {
        let mut prefs = ControlPrefs::default();
        prefs.midi.port_name = Some("  ".into());
        prefs.osc.port = 0;
        prefs.midi.bindings = vec![
            binding(36, ControlAction::Play),
            binding(36, ControlAction::Stop),
            binding(37, ControlAction::GotoCue(0)),
            binding(38, ControlAction::StepFrames(0)),
            binding(39, ControlAction::GotoCue(2)),
            binding(200, ControlAction::Stop),
        ];
        prefs.midi.bindings[5].trigger.number = 200;
        prefs.midi.bindings.push(Binding {
            trigger: MidiTrigger {
                kind: TriggerKind::Cc,
                channel: 17,
                number: 1,
            },
            action: ControlAction::Stop,
            feedback: None,
        });
        prefs.midi.bindings[4].feedback = Some(Feedback { on: 200, off: 0 });
        prefs.midi.bindings[0].feedback = Some(Feedback { on: 5, off: 1 });

        let prefs = normalize(prefs);

        assert_eq!(prefs.midi.port_name, None);
        assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
        assert_eq!(
            prefs.midi.bindings,
            vec![
                Binding {
                    feedback: Some(Feedback { on: 5, off: 1 }),
                    ..binding(36, ControlAction::Play)
                },
                binding(39, ControlAction::GotoCue(2)),
            ],
            "a value that is not a data byte is dropped, the others are kept"
        );
    }

    #[test]
    fn preferences_should_survive_a_save_and_a_load() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("flux").join(FILE_NAME);
        let mut prefs = ControlPrefs::default();
        prefs.midi.enabled = true;
        prefs.midi.port_name = Some("APC40 mkII".into());
        prefs.midi.port_index = 1;
        prefs.midi.bindings = vec![binding(0, ControlAction::GotoCue(1))];
        prefs.osc = OscPrefs {
            enabled: true,
            port: 9100,
            ..OscPrefs::default()
        };

        save(&file, &prefs).unwrap();

        assert_eq!(load(&file), prefs);
        assert!(!file.with_extension("json.tmp").exists());
    }

    #[test]
    fn art_net_should_be_off_by_default_hear_every_universe_and_survive_a_save() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join(FILE_NAME);
        assert!(!ControlPrefs::default().artnet.enabled);
        assert_eq!(ControlPrefs::default().artnet.universe, None);

        let prefs = ControlPrefs {
            artnet: ArtnetPrefs {
                enabled: true,
                universe: Some(3),
                bind_address: "192.168.1.5".into(),
            },
            ..ControlPrefs::default()
        };
        save(&file, &prefs).unwrap();

        assert_eq!(load(&file), prefs);
    }

    #[test]
    fn normalize_should_drop_a_universe_that_is_not_a_port_address_and_an_empty_interface() {
        let mut prefs = ControlPrefs::default();
        prefs.artnet.universe = Some(40_000);
        prefs.artnet.bind_address = "  ".into();

        let prefs = normalize(prefs);

        assert_eq!(prefs.artnet.universe, None);
        assert_eq!(prefs.artnet.bind_address, "0.0.0.0");
    }

    #[test]
    fn a_file_from_before_art_net_should_load_with_it_off() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(
            &file,
            r#"{"midi":{"enabled":false},"osc":{"enabled":true,"port":9100}}"#,
        )
        .unwrap();

        let prefs = load(&file);

        assert_eq!(prefs.osc.port, 9100);
        assert_eq!(prefs.artnet, ArtnetPrefs::default());
    }

    #[test]
    fn a_missing_or_broken_file_should_give_the_defaults() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join(FILE_NAME);
        assert_eq!(load(&file), ControlPrefs::default());
        fs::write(&file, "{ not json").unwrap();
        assert_eq!(load(&file), ControlPrefs::default());
    }

    #[test]
    fn a_file_with_only_some_fields_should_fill_in_the_rest() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join(FILE_NAME);
        fs::write(&file, r#"{"osc":{"enabled":true}}"#).unwrap();

        let prefs = load(&file);

        assert!(prefs.osc.enabled);
        assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
        assert!(!prefs.midi.enabled);
    }

    #[test]
    fn a_link_state_should_serialize_for_the_window() {
        let json = |state: LinkState| serde_json::to_string(&state).unwrap();
        assert_eq!(json(LinkState::Off), r#"{"state":"off"}"#);
        assert_eq!(json(LinkState::Missing), r#"{"state":"missing"}"#);
        assert_eq!(
            json(LinkState::Listening("9000".into())),
            r#"{"state":"listening","detail":"9000"}"#
        );
        assert_eq!(
            json(LinkState::Error("in use".into())),
            r#"{"state":"error","detail":"in use"}"#
        );
    }
}
