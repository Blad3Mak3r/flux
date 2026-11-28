//! Real-time output: the audio stream that plays the programme and is the master clock, the LTC
//! that goes out on it or on a stream of its own, and the MIDI thread that sends MTC from that same
//! clock.
//!
//! What can be tested without hardware is kept apart from the device code: [`Renderer`] and
//! [`LtcFollower`] fill a buffer for any sample type, [`pick_config`] chooses a stream format, and
//! [`Snapshot`] turns the last published position into a current one. [`Output`] only wires them to
//! cpal and midir.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use serde::{Serialize, Serializer};
use thiserror::Error;

use super::cue_routes::{CueInfo, CueRoutes};
use super::ltc::{LtcConfig, LtcStream};
use super::metronome::{ClickOut, SharedMetronome};
use super::mixer::{ClipSet, MIX_CHUNK, Programme, SharedLevels};
use super::mtc::MtcScheduler;
use super::transport::{Command, Transport, TransportState};
use crate::devices;
use flux_audio::ENGINE_RATE;
use flux_core::Clock;
use flux_project::{
    DEFAULT_LTC_LEVEL, FrameRate, LtcOutput, MidiClockOutput, MtcOutput, OscDestination,
    OutputConfig, ProjectError, Tempo, Timeline,
};

mod branches;
mod clock;
mod handle;
mod info;
mod ltc_follow;
mod ltc_render;
mod midi_clock;
mod mtc_port;
mod plan;
mod spec;
mod streams;
mod supervisor;

use branches::*;
pub use clock::*;
pub use handle::*;
pub use info::*;
pub use ltc_follow::*;
pub use ltc_render::*;
use midi_clock::run_midi_clock;
pub(crate) use mtc_port::*;
use plan::*;
use spec::*;
use streams::*;
use supervisor::*;

#[cfg(test)]
mod tests;

const MIDI_CLIENT_NAME: &str = "FLUX";
/// The longest the MTC thread sleeps before it looks at the transport again.
pub(crate) const MTC_TICK: Duration = Duration::from_millis(1);
/// The last stretch before a quarter frame that the MTC thread spins instead of sleeping, because a
/// sleep can overshoot by more than this.
pub(crate) const MTC_SPIN: Duration = Duration::from_micros(200);
/// The audio thread keeps its stream alive and wakes up this often to see whether to stop.
const AUDIO_PARK: Duration = Duration::from_millis(50);
/// The published position is one buffer old at most; extrapolating further than this would only
/// hide a stalled stream.
pub(crate) const MAX_EXTRAPOLATION: Duration = Duration::from_millis(100);
/// A metronome on a device of its own starts over from the master's position only when it is this
/// far away (10 ms): a click is a short sound, so it must follow the master closely, but the
/// extrapolated position jitters by a millisecond or two and each start-over is a jump in the click.
const CLICK_SYNC_SAMPLES: u64 = ENGINE_RATE as u64 / 100;

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("audio device not found: {0}")]
    DeviceMissing(String),
    #[error("this system has no default audio output; choose a device in the Output tab")]
    NoDefaultDevice,
    #[error("{device} cannot play {channels} channels at {rate} Hz")]
    Unsupported {
        device: String,
        channels: u16,
        rate: u32,
    },
    #[error("audio stream: {0}")]
    Stream(String),
    #[error("stop microphone recording before playback")]
    Recording,
    #[error("the output is not running")]
    NotRunning,
    #[error(transparent)]
    Project(#[from] ProjectError),
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for OutputError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Self::Stream(code) = self
            && code.starts_with("show.error.")
        {
            return serializer.serialize_str(code);
        }
        if matches!(self, Self::Recording) {
            serializer.serialize_str("voiceRecordingActive")
        } else {
            serializer.collect_str(self)
        }
    }
}

/// What the control side tells the audio callback.
#[derive(Debug)]
pub enum Message {
    Transport(Command),
    Show(crate::show::ShowCommand),
    /// The audio to play from now on. The callback only swaps it in; see [`Output`] for who frees
    /// the old one.
    Clips(Arc<ClipSet>),
}
