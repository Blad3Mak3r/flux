//! The real-time side of Flux: transport, signal encoders (LTC, MTC), the mixer, the cues of the
//! Live view and the output that opens the audio and MIDI devices. No Tauri in here, so it is
//! testable without a window.

pub mod control;
pub mod control_in;
pub mod cue_routes;
pub mod cues;
pub mod devices;
pub mod fire;
pub mod ltc;
pub mod metronome;
pub mod midi_cue;
pub mod mixer;
pub mod mtc;
pub mod osc;
pub mod output;
pub mod transport;

pub mod preflight;

pub mod audio_routing;

pub mod recording;

pub mod show;

mod show_events;
