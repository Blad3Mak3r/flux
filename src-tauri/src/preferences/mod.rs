//! Per-installation routing and language. Nothing here belongs to an undo history.
use flux_project::{
    AudioOutput, LtcOutput, MetronomeOutput, MidiClockOutput, MtcOutput, OutputConfig, ShowOutputs,
};

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Mutex, PoisonError},
};
use tauri::{AppHandle, Emitter, Manager, State};

mod change;
pub(crate) mod commands;
mod store;

pub use self::change::Change;
pub use self::commands::{analysis_source, preferences_change, preferences_get};
use self::store::*;
pub use flux_project::UNASSIGNED_DEVICE as UNASSIGNED;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub audio_routes: BTreeMap<String, AudioOutput>,
    pub locale: Option<Locale>,
    pub driver: Option<String>,
    pub audio: Option<AudioOutput>,
    pub preview: Option<AudioOutput>,
    pub operator: Option<AudioOutput>,
    pub voice: Option<String>,
    pub microphone: Option<Microphone>,
    /// The audio input and channel external LTC comes in on, for the VJ clock.
    pub ltc_in: Option<Microphone>,
    /// The MIDI input external MTC comes in on, for the VJ clock.
    pub mtc_in: Option<String>,
    /// What the VJ grid's audio modulation listens to; absent is the grid's own sound.
    pub analysis: Option<AnalysisSource>,
    pub metronome: Option<AudioOutput>,
    pub ltc: BTreeMap<u32, LtcOutput>,
    pub mtc: BTreeMap<u32, MtcOutput>,
    /// The MIDI port MIDI clock goes to, for a cue show and a VJ show alike; the show decides
    /// whether it is sent.
    pub midi_clock: Option<MtcOutput>,
    /// The display the main screen of the video output opened on before screens had ids of
    /// their own; read as the main screen's when `video_displays` does not name it.
    #[serde(default)]
    pub video_display: Option<String>,
    /// The display each screen of the video output opens on, by screen id and display name; a
    /// screen not listed stays closed.
    #[serde(default)]
    pub video_displays: BTreeMap<String, String>,
    /// The folder Output > Snapshot saves into; absent is `Pictures/Flux Snapshots`.
    pub snapshot_dir: Option<String>,
    /// A snapshot is also added to the project's media as an image clip.
    pub snapshot_media: bool,
    /// The folder the VJ recording saves into; absent is `Videos/FLUX`.
    pub recordings_dir: Option<String>,
    /// The keys the user changed in a cue show; see `flux_project::keymap`.
    pub shortcuts: flux_project::keymap::Overrides,
    /// The keys every action has now: what the window looks key presses up in. Worked out when sent.
    #[serde(skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub keymap: Vec<flux_project::keymap::ActionView>,
    #[serde(skip_deserializing)]
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Microphone {
    pub driver: Option<String>,
    pub device: String,
    pub channel: u16,
}

/// What the audio analysis of the VJ grid listens to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AnalysisSource {
    /// Nothing: audio modulation stays still.
    Off,
    /// The sound of the grid's own clips.
    #[default]
    Grid,
    /// One channel of an audio input.
    Input {
        driver: Option<String>,
        device: String,
        channel: u16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    Es,
}

#[derive(Default)]
pub struct Store(pub Mutex<Option<Preferences>>);

#[cfg(test)]
mod tests;
