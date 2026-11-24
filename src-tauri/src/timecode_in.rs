//! The external timecode the VJ clock shows: LTC from an audio input or MTC from a MIDI input,
//! as the preferences choose. A reader is opened when the clock first asks for its source and
//! stays open until the clock goes back to Flux's own.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flux_engine::chase::{Latest, LtcReader, MtcReader, Reading};
use flux_engine::control_in::ListenError;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::preferences::{self, Microphone};

/// How long to wait before trying again to open an input that failed.
const RETRY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Ltc,
    Mtc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputState {
    /// No input is chosen in the preferences.
    Unset,
    /// The chosen input is not connected.
    Missing,
    /// The input is open and listening.
    Listening,
    /// The input could not be opened or stopped delivering.
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputStatus {
    pub state: InputState,
    /// What came in last, once something has.
    pub reading: Option<Reading>,
}

/// One open input, with the choice it was opened for.
struct Open<K, R> {
    choice: K,
    reader: Result<R, (InputState, Instant)>,
    latest: Arc<Latest>,
}

#[derive(Default)]
pub struct TimecodeIn {
    ltc: Option<Open<Microphone, LtcReader>>,
    mtc: Option<Open<String, MtcReader>>,
}

pub type SharedTimecodeIn = Mutex<TimecodeIn>;

/// Keeps `slot` open on `choice`: opens it when the choice changed, or retries a failed one.
fn keep_open<'a, K: PartialEq + Clone, R>(
    slot: &'a mut Option<Open<K, R>>,
    choice: Option<&K>,
    open: impl FnOnce(&K, Arc<Latest>) -> Result<R, InputState>,
) -> Option<&'a Open<K, R>> {
    let Some(choice) = choice else {
        *slot = None;
        return None;
    };
    let stale = match slot {
        Some(current) if current.choice == *choice => {
            matches!(current.reader, Err((_, at)) if at.elapsed() >= RETRY)
        }
        _ => true,
    };
    if stale {
        // Drop the old reader first, so the device is free to open again.
        *slot = None;
        let latest = Arc::new(Latest::default());
        let reader = open(choice, Arc::clone(&latest)).map_err(|state| (state, Instant::now()));
        *slot = Some(Open {
            choice: choice.clone(),
            reader,
            latest,
        });
    }
    slot.as_ref()
}

fn status<K, R>(open: Option<&Open<K, R>>, failed: impl Fn(&R) -> bool) -> InputStatus {
    let Some(open) = open else {
        return InputStatus {
            state: InputState::Unset,
            reading: None,
        };
    };
    let state = match &open.reader {
        Ok(reader) if failed(reader) => InputState::Error,
        Ok(_) => InputState::Listening,
        Err((state, _)) => *state,
    };
    InputStatus {
        state,
        reading: open.latest.read(),
    }
}

/// Where the external timecode of `source` is, opening its input first if it is not open yet.
#[tauri::command]
pub fn timecode_in_status(
    app: AppHandle,
    prefs: State<'_, preferences::Store>,
    inputs: State<'_, SharedTimecodeIn>,
    source: Source,
) -> Result<InputStatus, String> {
    let prefs = preferences::preferences_get(app, prefs)?;
    let mut inputs = inputs.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(match source {
        Source::Ltc => {
            let open = keep_open(&mut inputs.ltc, prefs.ltc_in.as_ref(), |input, latest| {
                LtcReader::open(
                    input.driver.clone(),
                    input.device.clone(),
                    input.channel,
                    latest,
                )
                .map_err(|_| InputState::Missing)
            });
            status(open, LtcReader::failed)
        }
        Source::Mtc => {
            let open = keep_open(&mut inputs.mtc, prefs.mtc_in.as_ref(), |port, latest| {
                MtcReader::open(port, latest).map_err(|error| match error {
                    ListenError::Missing => InputState::Missing,
                    ListenError::Unavailable(_) => InputState::Error,
                })
            });
            status(open, |_| false)
        }
    })
}

/// Closes both inputs, when the clock goes back to Flux's own or the VJ screen closes.
#[tauri::command]
pub fn timecode_in_close(inputs: State<'_, SharedTimecodeIn>) {
    *inputs.lock().unwrap_or_else(PoisonError::into_inner) = TimecodeIn::default();
}

/// The MIDI inputs MTC can come in on.
#[tauri::command]
pub fn timecode_midi_inputs() -> Result<Vec<String>, String> {
    flux_engine::devices::list_midi_inputs()
        .map(|ports| ports.into_iter().map(|port| port.name).collect())
        .map_err(|error| error.to_string())
}
