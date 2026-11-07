//! Tauri commands for the real-time output and the transport.
//!
//! `output_start` opens the devices a project names and `output_stop` closes them; the
//! `transport_*` commands only send a message to the running audio callback. While an output runs
//! it reports to the UI over three lanes, each with a thread and a [`Channel`] of its own: the
//! position of the transport, where the show stands among its cues, and the peak level of every
//! track, about 30 times a second each. A slow consumer of the meters never holds up the timecode,
//! and a channel, unlike a broadcast event, is ordered and goes to the caller alone. A reloaded
//! window replaces those channels through `output_attach` without touching the devices.
//!
//! No command runs on the main thread, which belongs to the window: the heavy ones (opening
//! devices, decoding, joining threads) use the blocking pool, the small ones (`transport_*`, a
//! fader) the command thread pool.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, IpcResponse};
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
use flux_project::{OutputConfig, Project, Tempo, TimecodeSettings, Timeline};

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
#[derive(Clone)]
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

impl Default for LevelsLane {
    fn default() -> Self {
        Self { silent: true }
    }
}

impl LevelsLane {
    fn next(&mut self, peaks: Vec<TrackPeak>) -> Option<Vec<TrackPeak>> {
        let quiet = peaks.iter().all(|track| track.peak == 0.0);
        let due = !(quiet && self.silent);
        self.silent = quiet;
        due.then_some(peaks)
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputSetup {
    timeline_id: String,
    timecode: TimecodeSettings,
    output: OutputConfig,
    advance: bool,
    audition: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachedOutput {
    info: OutputInfo,
    position: Position,
    timeline_id: String,
    start: u64,
    matches: bool,
}

/// A running output and the threads that report it to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRoutingState {
    active: Vec<flux_engine::audio_routing::AudioRouteState>,
    active_tracks: Vec<flux_project::routing::TrackRoute>,
    desired_tracks: Vec<flux_project::routing::TrackRoute>,
    pending: bool,
}
#[derive(Default)]
struct AudioRoutingLane {
    last: Option<AudioRoutingState>,
}
impl AudioRoutingLane {
    fn next(&mut self, state: AudioRoutingState) -> Option<AudioRoutingState> {
        if self.last.as_ref() == Some(&state) {
            return None;
        }
        self.last = Some(state.clone());
        Some(state)
    }
}

#[derive(Clone)]
struct OutputChannels {
    position: Channel<Position>,
    levels: Channel<Vec<TrackPeak>>,
    live: Channel<LiveState>,
    routing: Channel<AudioRoutingState>,
}
struct PendingAudio {
    config: OutputConfig,
    timeline: Timeline,
    programme: Programme,
    preferences: crate::preferences::Preferences,
    audition: bool,
}
pub struct Running {
    pending_audio: Option<PendingAudio>,
    stationary_requested: bool,
    channels: OutputChannels,
    app: AppHandle,
    advance: Option<Advance>,
    desired_audio: Arc<Mutex<OutputConfig>>,
    on_audio_routing: Sender<Channel<AudioRoutingState>>,
    output: Output,
    setup: OutputSetup,
    preferences: crate::preferences::Preferences,
    cues: SharedCues,
    on_position: Sender<Channel<Position>>,
    on_levels: Sender<Channel<Vec<TrackPeak>>>,
    on_live: Sender<Channel<LiveState>>,
    stop_lanes: Arc<AtomicBool>,
    lanes: Vec<JoinHandle<()>>,
}

impl Running {
    // The lanes need every subscriber and the setup they were opened for; they cannot be narrower.
    #[expect(
        clippy::too_many_arguments,
        reason = "the setup, the injected app and the channels of the output"
    )]
    fn new(
        output: Output,
        setup: OutputSetup,
        preferences: crate::preferences::Preferences,
        cues: SharedCues,
        app: AppHandle,
        advance: Option<Advance>,
        on_position: Channel<Position>,
        on_levels: Channel<Vec<TrackPeak>>,
        on_live: Channel<LiveState>,
        on_audio_routing: Channel<AudioRoutingState>,
    ) -> Result<Self, OutputError> {
        let (position_tx, position_rx) = mpsc::channel();
        let (levels_tx, levels_rx) = mpsc::channel();
        let (live_tx, live_rx) = mpsc::channel();
        let (audio_tx, audio_rx) = mpsc::channel();
        // Built before the lanes, so that failing to start one stops the other: `Drop` joins them.
        let mut running = Self {
            pending_audio: None,
            stationary_requested: false,
            channels: OutputChannels {
                position: on_position.clone(),
                levels: on_levels.clone(),
                live: on_live.clone(),
                routing: on_audio_routing.clone(),
            },
            app: app.clone(),
            advance: advance.clone(),
            desired_audio: Arc::new(Mutex::new(setup.output.clone())),
            on_audio_routing: audio_tx,
            output,
            setup,
            preferences,
            cues,
            on_position: position_tx,
            on_levels: levels_tx,
            on_live: live_tx,
            stop_lanes: Arc::new(AtomicBool::new(false)),
            lanes: Vec::new(),
        };

        let reader = running.output.reader();
        let active = running.setup.output.clone();
        let desired = Arc::clone(&running.desired_audio);
        running.spawn_lane(
            "flux-audio-routing",
            report(
                audio_rx,
                on_audio_routing,
                move |lane: &mut AudioRoutingLane| {
                    let desired = desired.lock().unwrap_or_else(PoisonError::into_inner);
                    lane.next(AudioRoutingState {
                        active: reader.audio_routes(),
                        active_tracks: active.track_routes.clone(),
                        desired_tracks: desired.track_routes.clone(),
                        pending: desired.audio_routes != active.audio_routes
                            || desired.track_routes != active.track_routes,
                    })
                },
            ),
        )?;
        let reader = running.output.reader();
        running.spawn_lane(
            "flux-position",
            report(position_rx, on_position, move |lane: &mut PositionLane| {
                lane.next(reader.position())
            }),
        )?;

        let (reader, cues) = (running.output.reader(), running.cues.clone());
        running.spawn_lane(
            "flux-live",
            report(live_rx, on_live, move |lane: &mut LiveLane| {
                lane.next(cues.current().state_at(reader.position().frame))
            }),
        )?;

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
        let mut previous_routes: Vec<flux_engine::audio_routing::AudioRouteState> = Vec::new();
        running.spawn_lane("flux-log", move || {
            for (level, line) in log.next(reader.position(), reader.faults(), Instant::now()) {
                log::log!(level, "{line}");
            }
            let routes = reader.audio_routes();
            for route in &routes {
                if previous_routes.iter().find(|r| r.id == route.id) != Some(route) {
                    log::info!(
                        "audio destination {}: {:?}, device {}, channels {:?}",
                        route.id,
                        route.status,
                        route.device,
                        route.channels
                    );
                }
            }
            previous_routes = routes;
        })?;

        let levels = running.output.levels();
        running.spawn_lane(
            "flux-levels",
            report(levels_rx, on_levels, move |lane: &mut LevelsLane| {
                lane.next(levels.current().take_peaks())
            }),
        )?;
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

/// A new subscriber receives the first reading even when nothing moved since the last tick.
fn report<T: IpcResponse, L: Default>(
    subscriptions: mpsc::Receiver<Channel<T>>,
    mut channel: Channel<T>,
    mut next: impl FnMut(&mut L) -> Option<T>,
) -> impl FnMut() {
    let mut lane = L::default();
    move || {
        if let Some(replacement) = subscriptions.try_iter().last() {
            channel = replacement;
            lane = L::default();
        }
        if let Some(value) = next(&mut lane) {
            let _ = channel.send(value);
        }
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
    prefs: State<'_, crate::preferences::Store>,
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    advance: bool,
    audition: bool,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
    on_audio_routing: Channel<AudioRoutingState>,
) -> Result<OutputInfo, OutputError> {
    project.validate()?;
    let prefs =
        crate::preferences::preferences_get(app.clone(), prefs).map_err(OutputError::Stream)?;
    let audition = audition && prefs.preview.is_some();
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let started = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
        let previous = lock(&state).take();
        let parked = previous.as_ref().map(|r| {
            (
                r.setup.timeline_id.clone(),
                r.setup.timecode.clone(),
                r.output.reader().position(),
            )
        });
        drop(previous);

        let timeline = active_timeline(&session, &project)?;
        let resolved = prefs.resolve_project(&project, timeline, audition);
        let list = cue_list(&project, timeline, &session, &audio);
        let programme = programme(&project, timeline, &session, &audio, &list);
        let output = Output::start(&resolved, timeline, programme, !audition)?;
        if let Some((id, timecode, position)) = parked
            && id == timeline.id
            && timecode == timeline.timecode
        {
            output.send(Command::Restore {
                samples: position.position_samples,
                paused: position.state == TransportState::Paused,
            })?;
        }
        let setup = OutputSetup {
            timeline_id: timeline.id.clone(),
            timecode: timeline.timecode.clone(),
            output: resolved,
            advance,
            audition,
        };
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
            setup,
            prefs,
            cues,
            app,
            advance,
            on_position,
            on_levels,
            on_live,
            on_audio_routing,
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
    audio: State<'_, SharedAudio>,
    app: AppHandle,
    prefs: State<'_, crate::preferences::Store>,
    state: State<'_, SharedOutput>,
    session: State<'_, SharedSession>,
    project: Project,
    audition: bool,
) -> Result<Reconfigured, OutputError> {
    project.validate()?;
    let prefs = crate::preferences::preferences_get(app, prefs).map_err(OutputError::Stream)?;
    let audition = audition && prefs.preview.is_some();
    let (state, session, audio) = (Arc::clone(&state), Arc::clone(&session), Arc::clone(&audio));
    let done = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
        let timeline = active_timeline(&session, &project)?;
        let resolved = prefs.resolve_project(&project, timeline, audition);
        {
            let mut guard = lock(&state);
            if let Some(running) = guard.as_mut()
                && running.setup.output == resolved
                && running.setup.timecode == timeline.timecode
                && running.setup.audition == audition
            {
                running.pending_audio = None;
                running.preferences = prefs.clone();
                *running
                    .desired_audio
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = resolved;
                return Ok(Reconfigured::Applied {
                    info: running.output.info().clone(),
                });
            }
        }
        let list = cue_list(&project, timeline, &session, &audio);
        let programme = programme(&project, timeline, &session, &audio, &list);
        let done = match lock(&state).as_mut() {
            Some(running) => {
                *running
                    .desired_audio
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = resolved.clone();
                let position = running.output.reader().position();
                let needs_restart = audition != running.setup.audition
                    || (running.output.playing_or_pending()
                        && !position.device_lost
                        && !prefs.same_routing(&running.preferences));
                let done = if needs_restart {
                    Reconfigured::NeedsRestart
                } else {
                    running.output.reconfigure(&resolved, timeline)
                };
                if matches!(done, Reconfigured::NeedsRestart) {
                    running.pending_audio = Some(PendingAudio {
                        config: resolved.clone(),
                        timeline: timeline.clone(),
                        programme,
                        preferences: prefs.clone(),
                        audition,
                    });
                } else {
                    running.pending_audio = None;
                }
                // The LTC and MTC outputs now play what the project asks for, so a window that
                // attaches later must find them as requested.
                if matches!(done, Reconfigured::Applied { .. }) {
                    running.setup.output = resolved;
                    running.preferences = prefs;
                }
                Ok(done)
            }
            None => Err(OutputError::NotRunning),
        };
        if matches!(done, Ok(Reconfigured::NeedsRestart)) && !is_playing(&state) {
            apply_pending(&state)?;
            let info = lock(&state)
                .as_ref()
                .ok_or(OutputError::NotRunning)?
                .output
                .info()
                .clone();
            return Ok(Reconfigured::Applied { info });
        }
        done
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

/// Replaces a window's subscriptions without opening devices or sending transport commands.
#[tauri::command(async)]
pub fn output_attach(
    state: State<'_, SharedOutput>,
    setup: OutputSetup,
    on_position: Channel<Position>,
    on_levels: Channel<Vec<TrackPeak>>,
    on_live: Channel<LiveState>,
    on_audio_routing: Channel<AudioRoutingState>,
) -> Option<AttachedOutput> {
    let mut running = lock(&state);
    let running = running.as_mut()?;
    let position = running.output.reader().position();
    running.channels = OutputChannels {
        position: on_position.clone(),
        levels: on_levels.clone(),
        live: on_live.clone(),
        routing: on_audio_routing.clone(),
    };
    let _ = running.on_position.send(on_position);
    let _ = running.on_levels.send(on_levels);
    let _ = running.on_live.send(on_live);
    let _ = running.on_audio_routing.send(on_audio_routing);
    Some(AttachedOutput {
        info: running.output.info().clone(),
        position,
        timeline_id: running.setup.timeline_id.clone(),
        start: running.setup.timecode.start,
        matches: setup == running.setup,
    })
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
        let _transition = transition();
        match lock(&state).as_mut() {
            Some(running) => {
                running.cues.replace(list);
                // The cue tracks follow the edit too (their schedules, and any new port).
                warnings.extend(running.output.set_events(timeline, &project.output.osc));
                if let Some(pending) = &mut running.pending_audio {
                    pending.programme = Programme {
                        set: set.clone(),
                        warnings: warnings.clone(),
                    };
                    pending.timeline = timeline.clone();
                }
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
        .is_some_and(|running| running.output.playing_or_pending())
}

/// Where the transport of the running output is and the cues it plays, for the remote control.
pub(crate) fn snapshot(state: &SharedOutput) -> Option<(Position, Arc<CueList>)> {
    lock(state)
        .as_ref()
        .map(|running| (running.output.reader().position(), running.cues.current()))
}

/// How the running output ended up, for the diagnostics.
pub fn running_info(state: &SharedOutput) -> Option<OutputInfo> {
    lock(state)
        .as_ref()
        .map(|running| running.output.info().clone())
}

pub fn routing_snapshot(state: &SharedOutput) -> Option<AudioRoutingState> {
    let guard = lock(state);
    let running = guard.as_ref()?;
    let desired = running
        .desired_audio
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Some(AudioRoutingState {
        active: running.output.reader().audio_routes(),
        active_tracks: running.setup.output.track_routes.clone(),
        desired_tracks: desired.track_routes.clone(),
        pending: desired.audio_routes != running.setup.output.audio_routes
            || desired.track_routes != running.setup.output.track_routes,
    })
}

/// Closes the running output, if any. Taken first and dropped after: stopping waits for threads,
/// and the lock must be free for the `transport_*` commands, which run on the main thread, in the
/// meantime.
pub async fn release(state: &SharedOutput) {
    let state = Arc::clone(state);
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let _transition = transition();
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

/// Serializes hardware transitions with transport commands, including remote control.
static OUTPUT_TRANSITION: Mutex<()> = Mutex::new(());
fn transition() -> MutexGuard<'static, ()> {
    OUTPUT_TRANSITION
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn apply_pending(state: &SharedOutput) -> Result<(), OutputError> {
    let mut guard = lock(state);
    let Some(running) = guard.as_mut() else {
        return Err(OutputError::NotRunning);
    };
    if running.output.reader().position().state == TransportState::Playing
        || running.pending_audio.is_none()
    {
        return Ok(());
    }
    let pending = running
        .pending_audio
        .take()
        .ok_or(OutputError::NotRunning)?;
    let position = running.output.reader().position();
    let setup = OutputSetup {
        timeline_id: pending.timeline.id.clone(),
        timecode: pending.timeline.timecode.clone(),
        output: pending.config.clone(),
        advance: running.setup.advance,
        audition: pending.audition,
    };
    let channels = running.channels.clone();
    let app = running.app.clone();
    let advance = running.advance.clone();
    let cues = running.cues.clone();
    let old = guard.take();
    drop(guard);
    drop(old);
    let output = Output::start(
        &pending.config,
        &pending.timeline,
        pending.programme,
        !pending.audition,
    )?;
    output.send(Command::Restore {
        samples: position.position_samples,
        paused: position.state == TransportState::Paused,
    })?;
    diagnostics::output_reconfigured(output.info());
    let running = Running::new(
        output,
        setup,
        pending.preferences,
        cues,
        app,
        advance,
        channels.position,
        channels.levels,
        channels.live,
        channels.routing,
    )?;
    *lock(state) = Some(running);
    Ok(())
}

pub(crate) fn send(state: &SharedOutput, command: Command) -> Result<(), OutputError> {
    let _transition = transition();
    if matches!(command, Command::Play) {
        for _ in 0..200 {
            let waiting = lock(state).as_ref().is_some_and(|r| {
                r.stationary_requested
                    && r.pending_audio.is_some()
                    && r.output.reader().position().state == TransportState::Playing
            });
            if !waiting {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        if lock(state).as_ref().is_some_and(|r| {
            r.stationary_requested
                && r.pending_audio.is_some()
                && r.output.reader().position().state == TransportState::Playing
        }) {
            return Err(OutputError::Stream(
                "waiting for the audio device to pause before applying routing".into(),
            ));
        }
        apply_pending(state)?;
    }
    {
        let mut guard = lock(state);
        let running = guard.as_mut().ok_or(OutputError::NotRunning)?;
        running.output.send(command)?;
        if matches!(command, Command::Pause | Command::Stop) {
            running.stationary_requested = true;
        }
        if matches!(command, Command::Play) {
            running.stationary_requested = false;
        }
    }
    if matches!(command, Command::Pause | Command::Stop) {
        let state = Arc::clone(state);
        tauri::async_runtime::spawn_blocking(move || {
            // Wait for the callback to consume Pause/Stop before closing any stream.
            for _ in 0..200 {
                thread::sleep(Duration::from_millis(5));
                let _transition = transition();
                let stationary = lock(&state)
                    .as_ref()
                    .is_none_or(|r| r.output.reader().position().state != TransportState::Playing);
                if stationary {
                    if let Err(error) = apply_pending(&state) {
                        log::warn!("pending audio routing could not apply: {error}");
                    }
                    break;
                }
            }
        });
    }
    Ok(())
}

#[tauri::command(async)]
pub fn transport_play(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Play)
}

#[tauri::command(async)]
pub fn transport_pause(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Pause)
}

/// Stops and goes back to the start of the timeline.
#[tauri::command(async)]
pub fn transport_stop(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Stop)
}

/// Moves to a timecode given as a frame count.
#[tauri::command(async)]
pub fn transport_seek(
    state: State<'_, SharedOutput>,
    voice: State<'_, Arc<crate::voice_commands::VoiceState>>,
    frame: u64,
) -> Result<(), OutputError> {
    crate::voice_commands::send_transport(&voice, &state, Command::Seek(frame))
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
            operator_lost: false,
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

    #[test]
    fn reattached_position_lane_should_send_an_unchanged_position_to_the_new_page() {
        let (subscriptions, pending) = mpsc::channel();
        let (old_tx, old_rx) = mpsc::channel();
        let (new_tx, new_rx) = mpsc::channel();
        let channel = |tx: Sender<u64>| {
            Channel::<Position>::new(move |body| {
                let json = body.deserialize::<serde_json::Value>().unwrap();
                tx.send(json["frame"].as_u64().unwrap()).unwrap();
                Ok(())
            })
        };
        let mut step = report(pending, channel(old_tx), |lane: &mut PositionLane| {
            lane.next(at(17))
        });
        step();
        assert_eq!(old_rx.try_recv(), Ok(17));
        step();
        assert!(old_rx.try_recv().is_err());

        subscriptions.send(channel(new_tx)).unwrap();
        step();
        assert_eq!(new_rx.try_recv(), Ok(17));
        assert!(old_rx.try_recv().is_err());
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
        let mut lane = LevelsLane::default();
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
        let mut lane = LevelsLane::default();
        lane.next(peak(0.5));
        assert_eq!(lane.next(peak(0.0)), Some(peak(0.0)));
        assert_eq!(lane.next(peak(0.0)), None);
    }

    #[test]
    fn levels_lane_should_stay_quiet_from_the_start_while_the_tracks_are_silent() {
        assert_eq!(LevelsLane::default().next(peak(0.0)), None);
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
