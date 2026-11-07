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

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::SharedSession;
use crate::graphics::IDENTIFIER;
use crate::transport_commands::{SharedOutput, TIMELINE_ADVANCED, is_playing, release, snapshot};
use flux_engine::control::{Binding, ControlAction, MidiTrigger, action_for, dedup, seek_target};
use flux_engine::control_in::{Input, ListenError, MidiListener, OscListener};
use flux_engine::devices::{self, MidiPort};
use flux_engine::output::OutputError;
use flux_engine::transport::{Command, TransportState};

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OscPrefs {
    pub enabled: bool,
    pub port: u16,
}

impl Default for OscPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_OSC_PORT,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ControlPrefs {
    pub midi: MidiPrefs,
    pub osc: OscPrefs,
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
    prefs.midi.bindings = dedup(prefs.midi.bindings);
    prefs.midi.port_name = prefs.midi.port_name.filter(|name| !name.trim().is_empty());
    if prefs.osc.port == 0 {
        prefs.osc.port = DEFAULT_OSC_PORT;
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
    pub inputs: Vec<MidiPort>,
    /// Why MIDI cannot be listed, on a system without it.
    pub midi_error: Option<String>,
}

/// The inputs that are open and the thread that executes what they receive. Dropping it closes
/// the inputs first, which ends the thread.
#[derive(Default)]
struct Running {
    midi: Option<MidiListener>,
    osc: Option<OscListener>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.midi = None;
        self.osc = None;
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
}

#[derive(Default)]
pub struct Control {
    engine: Mutex<Engine>,
    /// The next trigger is the one to learn: it is told to the window instead of executed.
    learning: Arc<AtomicBool>,
}

pub type SharedControl = Arc<Control>;

impl Control {
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
            prefs: engine.prefs.clone(),
            midi: engine.midi.clone(),
            osc: engine.osc.clone(),
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
        let (running, midi, osc) = open(app, &prefs, &self.learning);
        *engine = Engine {
            prefs,
            running,
            midi,
            osc,
        };
    }
}

/// Opens the inputs `prefs` ask for and starts the thread that executes them. The state of each
/// is reported and logged here, on the caller's thread.
fn open(
    app: &AppHandle,
    prefs: &ControlPrefs,
    learning: &Arc<AtomicBool>,
) -> (Running, LinkState, LinkState) {
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
        match OscListener::bind(prefs.osc.port, tx.clone()) {
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

    // The inputs hold the only senders, so the thread ends when they are dropped.
    drop(tx);
    if running.midi.is_some() || running.osc.is_some() {
        let dispatcher = Dispatcher {
            app: app.clone(),
            bindings: prefs.midi.bindings.clone(),
            learning: Arc::clone(learning),
        };
        running.worker = thread::Builder::new()
            .name("flux-control".into())
            .spawn(move || dispatcher.run(&rx))
            .inspect_err(|err| log::warn!("remote control: the thread did not start: {err}"))
            .ok();
    }
    (running, midi, osc)
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
}

impl Dispatcher {
    fn run(&self, inputs: &Receiver<Input>) {
        for input in inputs {
            self.handle(input);
        }
    }

    fn handle(&self, input: Input) {
        match input {
            Input::Midi(trigger) => {
                if self.learning.swap(false, Ordering::Relaxed) {
                    log::info!("remote control: learned {trigger:?}");
                    let _ = self.app.emit(CONTROL_LEARNED, trigger);
                } else if let Some(action) = action_for(&self.bindings, trigger) {
                    self.execute(action);
                } else {
                    log::debug!("remote control: {trigger:?} has no action");
                }
            }
            Input::Action(action) => self.execute(action),
        }
    }

    fn execute(&self, action: ControlAction) {
        log::debug!("remote control: {action:?}");
        let output = self.app.state::<SharedOutput>();
        match action {
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
        }
    }

    #[test]
    fn the_defaults_should_listen_to_nothing() {
        let prefs = ControlPrefs::default();
        assert!(!prefs.midi.enabled);
        assert!(!prefs.osc.enabled);
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
        });

        let prefs = normalize(prefs);

        assert_eq!(prefs.midi.port_name, None);
        assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
        assert_eq!(
            prefs.midi.bindings,
            vec![
                binding(36, ControlAction::Play),
                binding(39, ControlAction::GotoCue(2)),
            ]
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
        };

        save(&file, &prefs).unwrap();

        assert_eq!(load(&file), prefs);
        assert!(!file.with_extension("json.tmp").exists());
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
