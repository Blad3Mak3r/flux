//! The checks the launcher window runs before Flux opens: the audio hosts, the MIDI system and
//! the LTC and MTC encoders.
//!
//! Nothing here is kept: devices are opened when an output starts, and the encoders are built per
//! show. What the launcher gets is the first enumeration, which wakes the audio and MIDI drivers up
//! before the editor needs them, and a clear word for each part. Missing hardware is a normal
//! outcome, never a reason to stop.

use std::io;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use flux_core::Timecode;
use flux_engine::devices;
use flux_engine::ltc::{LtcConfig, LtcStream};
use flux_engine::mtc::{full_frame, quarter_frames};
use flux_project::FrameRate;

const EVENT: &str = "startup-step";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepId {
    Audio,
    Midi,
    Ltc,
    Mtc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepState {
    Running,
    Ready,
    /// Works, but there is no hardware to use it with.
    Missing,
    Fault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: StepId,
    pub state: StepState,
    /// Devices or ports found, for the steps that look for them.
    pub count: Option<usize>,
    /// Why a step is a fault.
    pub message: Option<String>,
}

impl Step {
    fn new(id: StepId, state: StepState) -> Self {
        Self {
            id,
            state,
            count: None,
            message: None,
        }
    }

    fn found(id: StepId, count: usize) -> Self {
        let state = if count == 0 {
            StepState::Missing
        } else {
            StepState::Ready
        };
        Self {
            count: Some(count),
            ..Self::new(id, state)
        }
    }

    fn checked(id: StepId, outcome: Result<(), String>) -> Self {
        match outcome {
            Ok(()) => Self::new(id, StepState::Ready),
            Err(message) => Self {
                message: Some(message),
                ..Self::new(id, StepState::Fault)
            },
        }
    }
}

fn run_step(id: StepId) -> Step {
    match id {
        StepId::Audio => Step::found(id, devices::list_audio(None).len()),
        StepId::Midi => match devices::list_midi() {
            Ok(ports) => Step::found(id, ports.len()),
            Err(err) => Step::checked(id, Err(err.to_string())),
        },
        StepId::Ltc => Step::checked(id, check_ltc()),
        StepId::Mtc => Step::checked(id, check_mtc()),
    }
}

/// Encodes one frame of LTC and checks the signal has the level asked for.
fn check_ltc() -> Result<(), String> {
    let config = LtcConfig::default();
    let mut stream = LtcStream::new(config, FrameRate::Fps25, 0, 0);
    stream.resync(0);
    let mut out = vec![0.0; (config.sample_rate / 25) as usize];
    stream.fill(&mut out);
    let peak = out
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    if (peak - config.level).abs() > 0.01 {
        return Err(format!("LTC peak {peak} instead of {}", config.level));
    }
    Ok(())
}

/// Builds the MTC messages of a known frame and reads the timecode back out of them.
fn check_mtc() -> Result<(), String> {
    let rate = FrameRate::Fps25;
    // 01:02:03:04
    let timecode = Timecode::from_frames(93_079, rate);
    let pieces = quarter_frames(timecode, rate);
    let nibble = |piece: usize| pieces[piece][1] & 0xF;
    let in_order = pieces
        .iter()
        .enumerate()
        .all(|(piece, message)| message[0] == 0xF1 && usize::from(message[1] >> 4) == piece);
    let decoded = (
        (nibble(7) & 1) << 4 | nibble(6),
        nibble(5) << 4 | nibble(4),
        nibble(3) << 4 | nibble(2),
        nibble(1) << 4 | nibble(0),
    );
    let frame = full_frame(timecode, rate);
    if !in_order || decoded != (1, 2, 3, 4) || frame[0] != 0xF0 || frame[9] != 0xF7 {
        return Err("MTC messages do not decode to the timecode they were built from".into());
    }
    Ok(())
}

/// Runs every check, announcing each with a `startup-step` event, and returns the final steps.
#[tauri::command]
pub async fn startup_run(app: AppHandle) -> Result<Vec<Step>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        [StepId::Audio, StepId::Midi, StepId::Ltc, StepId::Mtc]
            .into_iter()
            .map(|id| {
                let _ = app.emit(EVENT, Step::new(id, StepState::Running));
                let step = run_step(id);
                let _ = app.emit(EVENT, &step);
                step
            })
            .collect()
    })
    .await
    .map_err(|err| io::Error::other(err).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ltc_check_should_pass() {
        assert_eq!(check_ltc(), Ok(()));
    }

    #[test]
    fn the_mtc_check_should_pass() {
        assert_eq!(check_mtc(), Ok(()));
    }

    #[test]
    fn found_should_be_missing_when_nothing_was_found() {
        assert_eq!(Step::found(StepId::Audio, 0).state, StepState::Missing);
        assert_eq!(Step::found(StepId::Audio, 2).state, StepState::Ready);
    }

    #[test]
    fn a_failed_check_should_be_a_fault_with_its_reason() {
        let step = Step::checked(StepId::Ltc, Err("boom".into()));
        assert_eq!(step.state, StepState::Fault);
        assert_eq!(step.message.as_deref(), Some("boom"));
    }
}
