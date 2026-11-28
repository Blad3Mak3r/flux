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

use crate::commands::SharedSession;
use crate::graphics::IDENTIFIER;
use crate::transport_commands::{
    SharedOutput, TIMELINE_ADVANCED, is_playing, light_snapshot, release, snapshot,
};
use crate::vj_commands::SharedGrid;
use crate::vj_control::{SharedLearn, VjRunner};
use flux_engine::control::KeyMsg;
use flux_engine::control::{
    Binding, ControlAction, Feedback, MidiTrigger, action_for, dedup, marker_target, osc_action,
    seek_target,
};
use flux_engine::control_in::{ARTNET_UDP_PORT, ArtnetListener, Input, ListenError, MidiListener};
use flux_engine::control_light::{Facts, Lamps, LightPort, by_port};
use flux_engine::devices::{self, MidiPort};
use flux_engine::output::OutputError;
use flux_engine::transport::{Command, TransportState};
use flux_project::vj_feedback::{self, Lights as PadLights, Pad};
use flux_project::vj_mapping::{Command as VjCommand, Target};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

pub(crate) mod commands;
mod dispatch;
mod lights;
mod open;
mod prefs;

use self::dispatch::*;
use self::lights::*;
pub use self::prefs::normalize;
use self::prefs::*;

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

/// Opens the inputs the saved preferences ask for. Called once the app is up.
pub fn start(app: &AppHandle) {
    let prefs = prefs_file().map_or_else(ControlPrefs::default, |file| load(&file));
    let control = Arc::clone(&app.state::<SharedControl>());
    control.apply(app, prefs);
}

#[cfg(test)]
mod tests;
