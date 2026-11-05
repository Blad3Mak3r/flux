//! Tauri commands of the Live view: the cues of the open project and where the show stands.
//!
//! The cue list is derived from the project (see [`crate::engine::cues`]) and the length of each
//! clip comes from its decoded audio, so these commands run on the blocking pool, where decoding
//! belongs. While an output runs, the same state streams from its live lane
//! (`transport_commands`); these commands serve the time before the first Play, when the playhead
//! is parked in the window and no output reports it.

use std::io;
use std::sync::Arc;

use tauri::State;

use crate::audio::{AudioError, SharedAudio};
use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::engine::cues::{Cue, CueList, LiveState};
use crate::project::Project;

/// The cues of `project`. A clip whose audio cannot be read is a cue without length, never an error.
pub fn cue_list(project: &Project, session: &SharedSession, audio: &SharedAudio) -> CueList {
    CueList::build(project, |asset| {
        let decoded = load(session, audio, asset).ok()?;
        u64::try_from(decoded.frames()).ok()
    })
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AudioError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))
}

/// The cues of the project, in the order the show reaches them.
#[tauri::command]
pub async fn live_cues(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
) -> Result<Vec<Cue>, AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    blocking(move || cue_list(&project, &session, &audio).cues().to_vec()).await
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
    blocking(move || cue_list(&project, &session, &audio).state_at(frame)).await
}
