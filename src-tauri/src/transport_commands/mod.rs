//! Tauri commands for the real-time output and the transport.
//!
//! `output_start` opens the devices a project names and `output_stop` closes them; the
//! `transport_*` commands only send a message to the running audio callback. While an output runs
//! it reports to the UI over three lanes, each with a thread and a [`Channel`] of its own: the
//! position of the transport, where the show stands among its cues, and the peak level of every
//! track, about 30 times a second each. A slow consumer of the meters never holds up the timecode,
//! and a channel, unlike a broadcast event, is ordered and goes to the caller alone. A reloaded
//! window replaces those channels through `output_attach` without touching the devices.
//!
//! No command runs on the main thread, which belongs to the window: the heavy ones (opening
//! devices, decoding, joining threads) use the blocking pool, the small ones (`transport_*`, a
//! fader) the command thread pool.

use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::diagnostics::{self, LogLane};
use crate::live_commands::{active_timeline, cue_list};
use flux_audio::SharedAudio;
use flux_engine::cues::{CueList, LiveState, SharedCues};
use flux_engine::mixer::{self, Programme, TrackPeak};
use flux_engine::output::{Output, OutputError, OutputInfo, Position, Reconfigured};
use flux_engine::transport::{Command, TransportState};
use flux_project::{OutputConfig, Project, Tempo, TimecodeSettings, Timeline};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, IpcResponse};
use tauri::{AppHandle, Emitter, Manager, State};

mod lanes;
pub(crate) mod playback;
mod running;
pub(crate) mod setup;
mod snapshot;

use self::lanes::*;
use self::playback::*;
pub(crate) use self::playback::{send, show_reader, show_send};
pub use self::setup::output_start;
pub(crate) use self::setup::programme;
pub use self::snapshot::{is_playing, release, routing_snapshot, running_info};
pub(crate) use self::snapshot::{light_snapshot, snapshot};

const LANE_INTERVAL: Duration = Duration::from_millis(33);

/// The event the window hears when the show ended and the next timeline was made active. Its
/// payload is the [`ProjectState`] after the change.
pub const TIMELINE_ADVANCED: &str = "timeline-advanced";

/// The event the window hears when the programme's audio device, lost while the output ran, can be
/// opened again. The window reopens the output, which carries the playhead over, stopped.
pub const OUTPUT_DEVICE_BACK: &str = "output-device-back";

/// How often a lost programme device is looked for.
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputSetup {
    timeline_id: String,
    timecode: TimecodeSettings,
    output: OutputConfig,
    advance: bool,
    audition: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedOutput {
    info: OutputInfo,
    position: Position,
    timeline_id: String,
    start: u64,
    matches: bool,
}

/// A running output and the threads that report it to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRoutingState {
    active: Vec<flux_engine::audio_routing::AudioRouteState>,
    active_tracks: Vec<flux_project::routing::TrackRoute>,
    desired_tracks: Vec<flux_project::routing::TrackRoute>,
    pending: bool,
}

#[derive(Clone)]
struct OutputChannels {
    position: Channel<Position>,
    levels: Channel<Vec<TrackPeak>>,
    live: Channel<LiveState>,
    routing: Channel<AudioRoutingState>,
}

struct PendingAudio {
    config: OutputConfig,
    timeline: Timeline,
    programme: Programme,
    preferences: crate::preferences::Preferences,
    audition: bool,
}

pub struct Running {
    pending_audio: Option<PendingAudio>,
    stationary_requested: bool,
    channels: OutputChannels,
    app: AppHandle,
    advance: Option<Advance>,
    desired_audio: Arc<Mutex<OutputConfig>>,
    on_audio_routing: Sender<Channel<AudioRoutingState>>,
    output: Output,
    setup: OutputSetup,
    preferences: crate::preferences::Preferences,
    cues: SharedCues,
    on_position: Sender<Channel<Position>>,
    on_levels: Sender<Channel<Vec<TrackPeak>>>,
    on_live: Sender<Channel<LiveState>>,
    stop_lanes: Arc<AtomicBool>,
    lanes: Vec<JoinHandle<()>>,
}

pub type SharedOutput = Arc<Mutex<Option<Running>>>;

fn lock(state: &SharedOutput) -> MutexGuard<'_, Option<Running>> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
