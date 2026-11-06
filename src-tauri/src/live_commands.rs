//! Tauri commands of the Live view: the cues of the open project and where the show stands.
//!
//! The cue list is derived from the project (see [`crate::engine::cues`]) and the length of each
//! clip comes from its decoded audio, so these commands run on the blocking pool, where decoding
//! belongs. While an output runs, the same state streams from its live lane
//! (`transport_commands`); these commands serve the time before the first Play, when the playhead
//! is parked in the window and no output reports it.

use std::io;
use std::sync::{Arc, PoisonError};

use tauri::State;

use crate::audio::{AudioError, ENGINE_RATE, SharedAudio};
use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::engine::cues::{Cue, CueList, LiveState};
use crate::project::{AudioAsset, Project, ProjectError, Timeline};

/// The samples (at the engine rate) of an asset's audio, found the cheapest way: the audio already
/// decoded, else the length stored with its waveform, and only then by decoding it, which for a song
/// takes seconds.
pub(crate) fn samples_of(
    session: &SharedSession,
    audio: &SharedAudio,
    asset: &AudioAsset,
) -> Option<u64> {
    if let Some(decoded) = audio.get(&asset.id) {
        return u64::try_from(decoded.frames()).ok();
    }
    let stored = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .preview(asset);
    if let Ok(Some(preview)) = stored {
        return Some(samples_of_seconds(preview.duration_seconds));
    }
    let decoded = load(session, audio, asset).ok()?;
    u64::try_from(decoded.frames()).ok()
}

fn samples_of_seconds(seconds: f64) -> u64 {
    // Never negative or NaN: a stored length is only a number from a file.
    (seconds.max(0.0) * f64::from(ENGINE_RATE)).round() as u64
}

/// The cues of `timeline`, one of the timelines of `project`. A clip whose audio cannot be read is a
/// cue without length, never an error.
pub fn cue_list(
    project: &Project,
    timeline: &Timeline,
    session: &SharedSession,
    audio: &SharedAudio,
) -> CueList {
    CueList::build(project, timeline, |asset| samples_of(session, audio, asset))
}

/// The timeline of `project` that the backend has active.
pub fn active_timeline<'a>(
    session: &SharedSession,
    project: &'a Project,
) -> Result<&'a Timeline, ProjectError> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .active_timeline(project)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AudioError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))
}

/// The cues of the active timeline, in the order the show reaches them.
#[tauri::command]
pub async fn live_cues(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
) -> Result<Vec<Cue>, AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        Ok(cue_list(&project, timeline, &session, &audio)
            .cues()
            .to_vec())
    })
    .await?
}

/// Where the show stands with the transport at `frame`, for when no output runs to say so.
#[tauri::command]
pub async fn live_state(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    frame: u64,
) -> Result<LiveState, AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        Ok(cue_list(&project, timeline, &session, &audio).state_at(frame))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_of_seconds_should_convert_a_length_at_the_engine_rate() {
        assert_eq!(samples_of_seconds(2.0), 2 * u64::from(ENGINE_RATE));
    }

    #[test]
    fn samples_of_seconds_should_round_to_the_nearest_sample() {
        assert_eq!(samples_of_seconds(1.0 / 48_000.0 * 2.4), 2);
    }

    #[test]
    fn samples_of_seconds_should_not_go_negative_or_nan() {
        assert_eq!(samples_of_seconds(-3.0), 0);
        assert_eq!(samples_of_seconds(f64::NAN), 0);
    }
}
