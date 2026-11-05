//! Tauri commands for the real-time output and the transport.
//!
//! `output_start` opens the devices a project names and `output_stop` closes them; the
//! `transport_*` commands only send a message to the running audio callback. While an output runs
//! it reports to the UI over three lanes, each with a thread and a [`Channel`] of its own that
//! `output_start` was given: the position of the transport, where the show stands among its cues,
//! and the peak level of every track, about 30 times a second each. A slow consumer of the meters never holds up the timecode, and a
//! channel, unlike a broadcast event, is ordered, goes to the caller alone and ends with the output.
//!
//! No command runs on the main thread, which belongs to the window: the heavy ones (opening
//! devices, decoding, joining threads) use the blocking pool, the small ones (`transport_*`, a
//! fader) the command thread pool.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tauri::State;
use tauri::ipc::Channel;

use crate::audio::SharedAudio;
use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::engine::cues::{LiveState, SharedCues};
use crate::engine::mixer::{self, Programme, TrackPeak};
use crate::engine::output::{Output, OutputError, OutputInfo, Position};
use crate::engine::transport::Command;
use crate::live_commands::cue_list;
use crate::project::Project;

const LANE_INTERVAL: Duration = Duration::from_millis(33);

/// The position lane: a position that did not move is not sent again.
#[derive(Default)]
struct PositionLane {
    last: Option<Position>,
}

impl PositionLane {
    fn next(&mut self, position: Position) -> Option<Position> {
        (self.last.replace(position) != Some(position)).then_some(position)
    }
}

/// The live lane: a state of the show that did not change is not sent again.
#[derive(Default)]
struct LiveLane {
    last: Option<LiveState>,
}

impl LiveLane {
    fn next(&mut self, state: LiveState) -> Option<LiveState> {
        if self.last.as_ref() == Some(&state) {
            return None;
        }
        self.last = Some(state.clone());
        Some(state)
    }
}

/// The levels lane: silence is sent once, so the meters know to fall, and then not again.
struct LevelsLane {
    /// The previous levels were all silence, which was already sent.
    silent: bool,
}

impl LevelsLane {
    fn new() -> Self {
        Self { silent: true }
    }

    fn next(&mut self, peaks: Vec<TrackPeak>) -> Option<Vec<TrackPeak>> {
        let quiet = peaks.iter().all(|track| track.peak == 0.0);
        let due = !(quiet && self.silent);
        self.silent = quiet;
        due.then_some(peaks)
    }
}

/// A running output and the threads that report it to the UI.
pub struct Running {
    output: Output,
    cues: SharedCues,
    stop_lanes: Arc<AtomicBool>,
    lanes: Vec<JoinHandle<()>>,
}

impl Running {
    fn new(
        output: Output,
        cues: SharedCues,
        on_position: Channel<Position>,
        on_levels: Channel<Vec<TrackPeak>>,
        on_live: Channel<LiveState>,
    ) -> Result<Self, OutputError> {
        // Built before the lanes, so that failing to start one stops the other: `Drop` joins them.
        let mut running = Self {
            output,
            cues,
            stop_lanes: Arc::new(AtomicBool::new(false)),
            lanes: Vec::new(),
        };

        let reader = running.output.reader();
        let mut position = PositionLane::default();
        running.spawn_lane("flux-position", move || {
            if let Some(position) = position.next(reader.position()) {
                // A page that went away (a reload) cannot take it; the next output starts over.
                let _ = on_position.send(position);
            }
        })?;

        let (reader, cues) = (running.output.reader(), running.cues.clone());
        let mut live = LiveLane::default();
        running.spawn_lane("flux-live", move || {
            if let Some(state) = live.next(cues.current().state_at(reader.position().frame)) {
                let _ = on_live.send(state);
            }
        })?;

        let levels = running.output.levels();
        let mut lane = LevelsLane::new();
        running.spawn_lane("flux-levels", move || {
            if let Some(peaks) = lane.next(levels.current().take_peaks()) {
                let _ = on_levels.send(peaks);
            }
        })?;
        Ok(running)
    }

    fn spawn_lane(
        &mut self,
        name: &str,
        mut step: impl FnMut() + Send + 'static,
    ) -> Result<(), OutputError> {
        let stop = Arc::clone(&self.stop_lanes);
        let lane = thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    step();
                    thread::sleep(LANE_INTERVAL);
                }
            })
            .map_err(|err| OutputError::Stream(err.to_string()))?;
        self.lanes.push(lane);
        Ok(())
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop_lanes.store(true, Ordering::Relaxed);
        for lane in self.lanes.drain(..) {
            let _ = lane.join();
        }
    }
}

pub type SharedOutput = Arc<Mutex<Option<Running>>>;

fn lock(state: &SharedOutput) -> MutexGuard<'_, Option<Running>> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Opening devices and joining threads can block, so both run off the main thread. The previous
/// output is dropped outside the lock, because stopping it waits for threads that read the state.
#[tauri::command]
pub async fn output_start(
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
) -> Result<OutputInfo, OutputError> {
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    tauri::async_runtime::spawn_blocking(move || {
        let previous = lock(&state).take();
        drop(previous);

        let output = Output::start(&project, programme(&project, &session, &audio))?;
        let info = output.info().clone();
        let cues = SharedCues::new(cue_list(&project, &session, &audio));
        *lock(&state) = Some(Running::new(output, cues, on_position, on_levels, on_live)?);
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
            Some(running) => {
                running.cues.replace(cue_list(&project, &session, &audio));
                running.output.set_clips(set).map(|()| warnings)
            }
            None => Err(OutputError::NotRunning),
        }
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?
}

/// Moves one track's fader on the running output, without rebuilding the programme, so it can
/// follow a drag. The project keeps the value once the edit is committed.
#[tauri::command(async)]
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
    // Taken first and dropped after: stopping waits for threads, and the lock must be free for the
    // `transport_*` commands, which run on the main thread, in the meantime.
    tauri::async_runtime::spawn_blocking(move || {
        let previous = lock(&state).take();
        drop(previous);
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))
}

fn send(state: &SharedOutput, command: Command) -> Result<(), OutputError> {
    match lock(state).as_ref() {
        Some(running) => running.output.send(command),
        None => Err(OutputError::NotRunning),
    }
}

#[tauri::command(async)]
pub fn transport_play(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Play)
}

#[tauri::command(async)]
pub fn transport_pause(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Pause)
}

/// Stops and goes back to the start of the timeline.
#[tauri::command(async)]
pub fn transport_stop(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    send(&state, Command::Stop)
}

/// Moves to a timecode given as a frame count.
#[tauri::command(async)]
pub fn transport_seek(state: State<'_, SharedOutput>, frame: u64) -> Result<(), OutputError> {
    send(&state, Command::Seek(frame))
}

/// The current position, or `null` when no output is running.
#[tauri::command(async)]
pub fn transport_position(state: State<'_, SharedOutput>) -> Option<Position> {
    lock(&state)
        .as_ref()
        .map(|running| running.output.reader().position())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::cues::Countdown;
    use crate::engine::transport::TransportState;

    fn at(frame: u64) -> Position {
        Position {
            state: TransportState::Playing,
            frame,
            position_samples: frame * 1_920,
            device_lost: false,
            ltc_lost: false,
            clock_unpaced: false,
        }
    }

    fn peak(level: f32) -> Vec<TrackPeak> {
        vec![TrackPeak {
            id: "t1".into(),
            peak: level,
        }]
    }

    #[test]
    fn position_lane_should_send_the_first_position() {
        assert_eq!(PositionLane::default().next(at(1)), Some(at(1)));
    }

    #[test]
    fn position_lane_should_not_repeat_a_position_that_did_not_move() {
        let mut lane = PositionLane::default();
        lane.next(at(1));
        assert_eq!(lane.next(at(1)), None);
        assert_eq!(lane.next(at(2)), Some(at(2)));
    }

    fn state(next: &str) -> LiveState {
        LiveState {
            statuses: Vec::new(),
            current: None,
            next: Some(Countdown {
                cue_id: next.into(),
                remaining: 1,
                text: String::new(),
            }),
            running: Vec::new(),
        }
    }

    #[test]
    fn live_lane_should_send_the_first_state() {
        assert_eq!(LiveLane::default().next(state("a")), Some(state("a")));
    }

    #[test]
    fn live_lane_should_not_repeat_a_state_that_did_not_change() {
        let mut lane = LiveLane::default();
        lane.next(state("a"));
        assert_eq!(lane.next(state("a")), None);
        assert_eq!(lane.next(state("b")), Some(state("b")));
    }

    #[test]
    fn levels_lane_should_send_sound() {
        let mut lane = LevelsLane::new();
        assert_eq!(lane.next(peak(0.5)), Some(peak(0.5)));
        assert_eq!(lane.next(peak(0.25)), Some(peak(0.25)));
    }

    #[test]
    fn levels_lane_should_send_silence_once_and_then_nothing() {
        let mut lane = LevelsLane::new();
        lane.next(peak(0.5));
        assert_eq!(lane.next(peak(0.0)), Some(peak(0.0)));
        assert_eq!(lane.next(peak(0.0)), None);
    }

    #[test]
    fn levels_lane_should_stay_quiet_from_the_start_while_the_tracks_are_silent() {
        assert_eq!(LevelsLane::new().next(peak(0.0)), None);
    }

    #[test]
    fn lanes_should_serialise_to_the_shape_the_ui_reads() {
        let position = serde_json::to_value(at(3)).unwrap();
        assert_eq!(position["frame"], 3);
        assert_eq!(position["positionSamples"], 3 * 1_920);
        let levels = serde_json::to_value(peak(0.5)).unwrap();
        assert_eq!(levels[0]["id"], "t1");
    }
}
