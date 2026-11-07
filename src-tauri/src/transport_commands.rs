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
use std::time::{Duration, Instant};

use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::audio_commands::load;
use crate::commands::SharedSession;
use crate::diagnostics::{self, LogLane};
use crate::live_commands::{active_timeline, cue_list};
use flux_audio::SharedAudio;
use flux_engine::cues::{CueList, LiveState, SharedCues};
use flux_engine::mixer::{self, Programme, TrackPeak};
use flux_engine::output::{Output, OutputError, OutputInfo, Position, Reconfigured};
use flux_engine::transport::{Command, TransportState};
use flux_project::{Project, Tempo, Timeline};

const LANE_INTERVAL: Duration = Duration::from_millis(33);

/// The event the window hears when the show ended and the next timeline was made active. Its
/// payload is the [`ProjectState`] after the change.
pub const TIMELINE_ADVANCED: &str = "timeline-advanced";

/// The event the window hears when the programme's audio device, lost while the output ran, can be
/// opened again. The window reopens the output, which carries the playhead over, stopped.
pub const OUTPUT_DEVICE_BACK: &str = "output-device-back";

/// How often a lost programme device is looked for.
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The device lane: says once when the programme's device is back after it was lost.
#[derive(Default)]
struct DeviceLane {
    last_check: Option<Instant>,
    announced: bool,
}

impl DeviceLane {
    /// Whether to tell the window now. `present` looks for the device, which is not cheap, so it
    /// only runs while the device is lost, at most once an interval.
    fn back(&mut self, lost: bool, now: Instant, present: impl FnOnce() -> bool) -> bool {
        if !lost || self.announced {
            return false;
        }
        let due = self
            .last_check
            .is_none_or(|last| now.duration_since(last) >= DEVICE_CHECK_INTERVAL);
        if !due {
            return false;
        }
        self.last_check = Some(now);
        self.announced = present();
        self.announced
    }
}

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

/// The end lane: tells when the show ran to its end, from the count the output keeps.
struct EndLane {
    seen: u64,
}

impl EndLane {
    fn new(seen: u64) -> Self {
        Self { seen }
    }

    /// Whether the show ended since the last call.
    fn ended(&mut self, count: u64) -> bool {
        std::mem::replace(&mut self.seen, count) != count
    }
}

/// What the end lane does when the show ends: the next timeline becomes the active one, stopped at
/// its start, and the window is told.
struct Advance {
    app: AppHandle,
    session: SharedSession,
}

impl Advance {
    fn run(&self) {
        let next = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next_timeline();
        match next {
            Ok(Some(state)) => {
                log::info!("show ended, the next timeline is the active one");
                let _ = self.app.emit(TIMELINE_ADVANCED, state);
            }
            Ok(None) => log::info!("show ended"),
            Err(err) => log::warn!("show ended, the next timeline could not be selected: {err}"),
        }
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
        app: AppHandle,
        advance: Option<Advance>,
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

        if let Some(advance) = advance {
            let reader = running.output.reader();
            let mut end = EndLane::new(reader.shows_ended());
            running.spawn_lane("flux-advance", move || {
                if end.ended(reader.shows_ended()) {
                    advance.run();
                }
            })?;
        }

        let (reader, device) = (running.output.reader(), running.output.programme_device());
        let mut back = DeviceLane::default();
        running.spawn_lane("flux-device", move || {
            let lost = reader.position().device_lost;
            if back.back(lost, Instant::now(), || device.is_present()) {
                log::info!("audio device is back");
                let _ = app.emit(OUTPUT_DEVICE_BACK, ());
            }
        })?;

        let reader = running.output.reader();
        let mut log = LogLane::default();
        running.spawn_lane("flux-log", move || {
            for (level, line) in log.next(reader.position(), reader.faults(), Instant::now()) {
                log::log!(level, "{line}");
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
// A Tauri command takes the managed state it uses and each channel the window gives it as an
// argument of its own, so it cannot be narrower.
#[expect(
    clippy::too_many_arguments,
    reason = "injected state and channels of a command"
)]
#[tauri::command]
pub async fn output_start(
    app: AppHandle,
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    advance: bool,
    audition: bool,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
) -> Result<OutputInfo, OutputError> {
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let started = tauri::async_runtime::spawn_blocking(move || {
        let previous = lock(&state).take();
        drop(previous);

        let timeline = active_timeline(&session, &project)?;
        let list = cue_list(&project, timeline, &session, &audio);
        let programme = programme(&project, timeline, &session, &audio, &list);
        let output = Output::start(&project, timeline, programme, !audition)?;
        let info = output.info().clone();
        diagnostics::output_started(&info);
        let cues = SharedCues::new(list);
        // With `advance`, the show running to its end makes the next timeline the active one.
        let advance = advance.then(|| Advance {
            app: app.clone(),
            session: Arc::clone(&session),
        });
        *lock(&state) = Some(Running::new(
            output,
            cues,
            app,
            advance,
            on_position,
            on_levels,
            on_live,
        )?);
        Ok(info)
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    if let Err(err) = &started {
        log::error!("output could not start: {err}");
    }
    started
}

/// Brings the running output's LTC and MTC outputs in line with the project's, without touching the
/// programme's stream, so the change applies while playing. `needsRestart` says the change reaches
/// the programme's stream (its device, channels, the LTC on it, the metronome, the frame rate or
/// start): nothing was touched and the output has to be started again.
#[tauri::command]
pub async fn output_reconfigure(
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    project: Project,
) -> Result<Reconfigured, OutputError> {
    let (state, session) = (Arc::clone(&state), Arc::clone(&session));
    let done = tauri::async_runtime::spawn_blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        match lock(&state).as_mut() {
            Some(running) => Ok(running.output.reconfigure(&project, timeline)),
            None => Err(OutputError::NotRunning),
        }
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    match &done {
        Ok(Reconfigured::Applied { info }) => diagnostics::output_reconfigured(info),
        Ok(Reconfigured::NeedsRestart) => log::info!("output settings changed, restart needed"),
        Err(err) => log::warn!("the output could not be reconfigured: {err}"),
    }
    done
}

/// The audio of the timeline's clips, decoding what has not been decoded yet, and where the show
/// ends according to `cues`. A file that cannot be read is left out and reported, never an error.
fn programme(
    project: &Project,
    timeline: &Timeline,
    session: &SharedSession,
    audio: &SharedAudio,
    cues: &CueList,
) -> Programme {
    let mut programme = mixer::build(project, timeline, |asset| load(session, audio, asset));
    programme.set = programme.set.with_end(Some(cues.end_samples()));
    programme
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
    let updated = tauri::async_runtime::spawn_blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        let list = cue_list(&project, timeline, &session, &audio);
        let Programme { set, mut warnings } =
            programme(&project, timeline, &session, &audio, &list);
        match lock(&state).as_mut() {
            Some(running) => {
                running.cues.replace(list);
                // The cue tracks follow the edit too (their schedules, and any new port).
                warnings.extend(running.output.set_events(timeline, &project.output.osc));
                running.output.set_clips(set).map(|()| warnings)
            }
            None => Err(OutputError::NotRunning),
        }
    })
    .await
    .map_err(|err| OutputError::Stream(err.to_string()))?;
    match &updated {
        Ok(warnings) => diagnostics::log_warnings(warnings),
        Err(err) => log::warn!("the clips could not be sent to the output: {err}"),
    }
    updated
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

/// Moves the metronome to the active timeline's tempo or click switch on the running output, so an
/// edit is heard at once. No output, or none with a metronome, is not an error: the next output
/// reads the tempo from the project.
#[tauri::command(async)]
pub fn output_set_tempo(state: State<'_, SharedOutput>, tempo: Tempo) {
    if let Some(running) = lock(&state).as_ref() {
        running.output.set_tempo(tempo);
    }
}

/// Whether the transport of the running output is playing.
pub fn is_playing(state: &SharedOutput) -> bool {
    lock(state)
        .as_ref()
        .is_some_and(|running| running.output.reader().position().state == TransportState::Playing)
}

/// How the running output ended up, for the diagnostics.
pub fn running_info(state: &SharedOutput) -> Option<OutputInfo> {
    lock(state)
        .as_ref()
        .map(|running| running.output.info().clone())
}

/// Closes the running output, if any. Taken first and dropped after: stopping waits for threads,
/// and the lock must be free for the `transport_*` commands, which run on the main thread, in the
/// meantime.
pub async fn release(state: &SharedOutput) {
    let state = Arc::clone(state);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let previous = lock(&state).take();
        let running = previous.is_some();
        drop(previous);
        if running {
            log::info!("output stopped");
        }
    })
    .await;
}

#[tauri::command]
pub async fn output_stop(state: State<'_, SharedOutput>) -> Result<(), OutputError> {
    release(&state).await;
    Ok(())
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
    use flux_engine::cues::Countdown;

    fn at(frame: u64) -> Position {
        Position {
            state: TransportState::Playing,
            frame,
            position_samples: frame * 1_920,
            device_lost: false,
            ltc_lost: 0,
            mtc_lost: 0,
            metronome_lost: false,
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
            notes: Vec::new(),
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
    fn end_lane_should_say_so_once_per_end() {
        let mut lane = EndLane::new(0);

        assert!(!lane.ended(0));
        assert!(lane.ended(1));
        assert!(!lane.ended(1));
        assert!(lane.ended(2));
    }

    #[test]
    fn end_lane_should_ignore_the_ends_before_the_output_opened() {
        let mut lane = EndLane::new(3);

        assert!(!lane.ended(3));
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
    fn device_lane_should_not_look_for_a_device_that_is_not_lost() {
        let mut lane = DeviceLane::default();

        assert!(!lane.back(false, Instant::now(), || panic!("looked")));
    }

    #[test]
    fn device_lane_should_look_once_a_second_and_announce_once() {
        let mut lane = DeviceLane::default();
        let start = Instant::now();

        assert!(!lane.back(true, start, || false));
        assert!(
            !lane.back(true, start + Duration::from_millis(500), || panic!(
                "too soon"
            ))
        );
        assert!(lane.back(true, start + DEVICE_CHECK_INTERVAL, || true));
        assert!(
            !lane.back(true, start + 3 * DEVICE_CHECK_INTERVAL, || panic!(
                "announced"
            ))
        );
    }

    #[test]
    fn lanes_should_serialise_to_the_shape_the_ui_reads() {
        let position = serde_json::to_value(at(3)).unwrap();
        assert_eq!(position["frame"], 3);
        assert_eq!(position["positionSamples"], 3 * 1_920);
        assert_eq!(position["ltcLost"], 0);
        assert_eq!(position["mtcLost"], 0);
        assert_eq!(position["metronomeLost"], false);
        let levels = serde_json::to_value(peak(0.5)).unwrap();
        assert_eq!(levels[0]["id"], "t1");
    }
}
