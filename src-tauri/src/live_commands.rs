//! Tauri commands of the Live view: the cues of the open project and where the show stands.
//!
//! The cue list is derived from the project (see [`flux_engine::cues`]) and the length of each
//! clip comes from its decoded audio, so these commands run on the blocking pool, where decoding
//! belongs. While an output runs, the same state streams from its live lane
//! (`transport_commands`); these commands serve the time before the first Play, when the playhead
//! is parked in the window and no output reports it.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};

use tauri::State;

use crate::audio_commands::load;
use crate::commands::SharedSession;
use flux_audio::{AudioError, ENGINE_RATE, SharedAudio};
use flux_engine::cues::{Cue, CueList, LiveState};
use flux_engine::midi_panic::{self, PanicReport};
use flux_project::{AudioAsset, Project, ProjectError, Timeline};

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
    CueList::build_with_video(
        project,
        timeline,
        |asset| samples_of(session, audio, asset),
        |video| {
            crate::video_commands::timeline_frames_of(session, video, timeline.timecode.frame_rate)
        },
    )
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

/// The ids of the cues of the active timeline that match what the operator typed, best first. The
/// window shows the list and goes to the one picked with the seek of a click on its row.
#[tauri::command]
pub async fn live_search(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    query: String,
) -> Result<Vec<String>, AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        let list = cue_list(&project, timeline, &session, &audio);
        Ok(flux_engine::cue_search::search(list.cues(), &query))
    })
    .await?
}

/// Whether the operator has locked the show.
#[tauri::command]
pub fn show_lock_get(session: State<'_, SharedSession>) -> bool {
    show_lock(&session).load(Ordering::Acquire)
}

/// Locks the show or unlocks it; the answer is the state it is in. Locked, the session refuses to
/// change, open or close the project (see `Session::ensure_editable`), so the shortcuts and the
/// remote control are held back too. The lock is session state: it is not saved and starts off.
#[tauri::command]
pub fn show_lock_set(session: State<'_, SharedSession>, locked: bool) -> bool {
    show_lock(&session).store(locked, Ordering::Release);
    log::info!("show {}", if locked { "locked" } else { "unlocked" });
    locked
}

fn show_lock(session: &SharedSession) -> Arc<AtomicBool> {
    Arc::clone(
        &session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .show_lock,
    )
}

/// Silences the MIDI gear of the project: All Sound Off, Reset All Controllers and All Notes Off
/// on every channel of every MIDI port it uses. Works with the output stopped; the ports are opened
/// for the burst.
#[tauri::command]
pub async fn midi_panic(
    app: tauri::AppHandle,
    prefs: State<'_, crate::preferences::Store>,
    project: Project,
) -> Result<PanicReport, String> {
    let prefs = crate::preferences::preferences_get(app, prefs)?;
    tauri::async_runtime::spawn_blocking(move || panic_now(&prefs, &project))
        .await
        .map_err(|err| err.to_string())
}

/// The panic of the open project, for the remote control and the OSC API.
pub(crate) fn panic_from_app(app: &tauri::AppHandle) -> Result<PanicReport, String> {
    use tauri::Manager;

    let project = app
        .state::<SharedSession>()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .document()
        .map(|document| document.project)
        .ok_or_else(|| "noProject".to_string())?;
    let prefs = crate::preferences::preferences_get(app.clone(), app.state())?;
    Ok(panic_now(&prefs, &project))
}

/// Sends the panic to the ports `project` uses and logs how it went. Shared by the window's button
/// and the remote control (MIDI, OSC), which has no window to ask.
pub(crate) fn panic_now(prefs: &crate::preferences::Preferences, project: &Project) -> PanicReport {
    let mtc = prefs.resolve(&project.output, false).mtc;
    let report = midi_panic::send(&midi_panic::ports(project, &mtc));
    log::info!(
        "MIDI panic: {} ports silenced ({} through the output), {} failed",
        report.sent,
        report.via_output,
        report.failed.len()
    );
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_show_lock_should_start_off_and_follow_the_flag_of_the_session() {
        let session = SharedSession::default();
        let lock = show_lock(&session);
        assert!(!lock.load(Ordering::Acquire));
        lock.store(true, Ordering::Release);
        assert!(session.lock().unwrap().ensure_editable().is_err());
        assert!(session.lock().unwrap().ensure_not_running().is_ok());
    }

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
