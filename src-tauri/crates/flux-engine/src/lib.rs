//! The real-time side of Flux: transport, signal encoders (LTC, MTC), the mixer, the cues of the
//! Live view and the output that opens the audio and MIDI devices. No Tauri in here, so it is
//! testable without a window.

pub mod cues;
pub mod devices;
pub mod ltc;
pub mod metronome;
pub mod mixer;
pub mod mtc;
pub mod output;
pub mod transport;
