//! Tauri commands for the real-time output and the transport.
//!
//! `output_start` opens the devices a project names and `output_stop` closes them; the
//! `transport_*` commands only send a message to the running audio callback. While an output runs,
//! the position is pushed to the UI as a `transport-position` event, about 30 times a second,
//! whenever it changes, and the peak level of every track as a `transport-levels` event.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::audio::SharedAudio;
use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::engine::mixer::{self, Programme, SharedLevels, TrackPeak};
use crate::engine::output::{Output, OutputError, OutputInfo, Position, PositionReader};
use crate::engine::transport::Command;
use crate::project::Project;

const POSITION_EVENT: &str = "transport-position";
const POSITION_INTERVAL: Duration = Duration::from_millis(33);
const LEVELS_EVENT: &str = "transport-levels";

/// The peak of each track since the previous event, as linear amplitudes.
#[derive(Clone, Serialize)]
struct Levels {
    tracks: Vec<TrackPeak>,
}

/// A running output and the thread that reports its position to the UI.
pub struct Running {
    output: Output,
    stop_emitter: Arc<AtomicBool>,
    emitter: Option<JoinHandle<()>>,
}

impl Running {
    fn new(output: Output, app: AppHandle) -> Self {
        let stop_emitter = Arc::new(AtomicBool::new(false));
        let emitter = spawn_emitter(
            app,
            output.reader(),
            output.levels(),
            Arc::clone(&stop_emitter),
        );
        Self {
            output,
            stop_emitter,
            emitter: Some(emitter),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop_emitter.store(true, Ordering::Relaxed);
        if let Some(emitter) = self.emitter.take() {
            let _ = emitter.join();
        }
    }
}

pub type SharedOutput = Arc<Mutex<Option<Running>>>;

fn lock(state: &SharedOutput) -> MutexGuard<'_, Option<Running>> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn spawn_emitter(
    app: AppHandle,
    reader: PositionReader,
    levels: SharedLevels,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut last = None;
        let mut silent = true;
        while !stop.load(Ordering::Relaxed) {
            let position = reader.position();
            if last != Some(position) {
                let _ = app.emit(POSITION_EVENT, position);
                last = Some(position);
            }
            // Silence is sent once, so the meters know to fall, and then not again.
            let tracks = levels.current().take_peaks();
            let quiet = tracks.iter().all(|track| track.peak == 0.0);
            if !(quiet && silent) {
                let _ = app.emit(LEVELS_EVENT, Levels { tracks });
            }
            silent = quiet;
            thread::sleep(POSITION_INTERVAL);
        }
    })
}

/// Opening devices and joining threads can block, so both run off the main thread. The previous
/// output is dropped outside the lock, because stopping it waits for threads that read the state.
#[tauri::command]
pub async fn output_start(
    app: AppHandle,
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
) -> Result<OutputInfo, OutputError> {
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        let previous = lock(&state).take();
        drop(previous);

        let output = Output::start(&project, programme(&project, &session, &audio))?;
        let info = output.info().clone();
        *lock(&state) = Some(Running::new(output, app));
        Ok(info)
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?
}

/// The audio of the project's clips, decoding what has not been decoded yet. A file that cannot be
/// read is left out and reported, never an error.
fn programme(project: &Project, session: &SharedSession, audio: &SharedAudio) -> Programme {
    mixer::build(project, |asset| load(session, audio, asset))
}

/// Sends the project's current clips to the running output, after tracks were muted or clips
/// moved. Returns the warnings about files that could not be used.
#[tauri::command]
pub async fn output_set_clips(
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
) -> Result<Vec<String>, OutputError> {
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        let Programme { set, warnings } = programme(&project, &session, &audio);
        match lock(&state).as_ref() {
            Some(running) => running.output.set_clips(set).map(|()| warnings),
            None => Err(OutputError::NotRunning),
        }
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?
}

/// Moves one track's fader on the running output, without rebuilding the programme, so it can
/// follow a drag. The project keeps the value once the edit is committed.
#[tauri::command]
pub fn output_set_track_gain(
    state: State<'_, SharedOutput>,
    track_id: String,
    gain_db: f32,
) -> Result<(), OutputError> {
    match lock(&state).as_ref() {
        Some(running) => {
            running
                .output
                .levels()
                .current()
                .set_gain_db(&track_id, gain_db);
            Ok(())
        }
        None => Err(OutputError::NotRunning),
    }
}

#[tauri::command]
pub async fn output_stop(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || drop(lock(&state).take()))
        .await
        .map_err(|err| OutputError::Stream(err.to_string()))
}

fn send(state: &SharedOutput, command: Command) -> Result<(), OutputError> {
    match lock(state).as_ref() {
        Some(running) => running.output.send(command),
        None => Err(OutputError::NotRunning),
    }
}

#[tauri::command]
pub fn transport_play(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Play)
}

#[tauri::command]
pub fn transport_pause(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Pause)
}

/// Stops and goes back to the start of the timeline.
#[tauri::command]
pub fn transport_stop(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Stop)
}

/// Moves to a timecode given as a frame count.
#[tauri::command]
pub fn transport_seek(state: State<'_, SharedOutput>, frame: u64) -> Result<(), OutputError> {
    send(&state, Command::Seek(frame))
}

/// The current position, or `null` when no output is running.
#[tauri::command]
pub fn transport_position(state: State<'_, SharedOutput>) -> Option<Position> {
    lock(&state)
        .as_ref()
        .map(|running| running.output.reader().position())
}
