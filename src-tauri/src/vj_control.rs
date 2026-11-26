//! Running the mappings of a VJ show: what a MIDI controller or an OSC message does to the grid.
//!
//! The decoding, the mappings, the scaling and the plan of each command are pure and live in
//! `flux_project::vj_mapping` (tested without hardware); what is here carries them out on the
//! `flux-control` thread, through the same backend paths as the window: an edit of the project
//! goes through the session as `vj_edit` does, a launch or a stop through [`crate::vj_commands`].
//!
//! **Undo.** A discrete edit (a bypass, a deck, a tempo button, a learned mapping) is sent to the
//! window at once with `commit`, so it is one entry of the undo history. A value moved by a
//! controller (a fader, a knob) is applied in place, as a fader drag is, and sent to the window at
//! most [`EMIT_EVERY`] apart without `commit`; once the control has been still for
//! [`COMMIT_AFTER`] the last state is sent again with `commit`, which closes the gesture. So a
//! sweep of a knob is one undo step, not hundreds, and the output follows the hand at once.
//!
//! **Locks.** The session is locked only to read the mappings, plan and apply the edit, which is
//! all CPU; the window is told and the grid played with no lock held. Nothing here logs from the
//! MIDI callback or the socket threads: they only decode and send, and this thread reports.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::SharedSession;
use crate::vj_commands::{BeatOp, SharedGrid, beat, play};
use flux_engine::artnet::{self, DmxFrame, DmxLearn, DmxState};
use flux_engine::control::{KeyMsg, MidiMsg, OscMessage};
use flux_project::vj_mapping::{
    Command, Heard, Plan, PlayAction, Source, Target, coalesce, osc_command, osc_level, plan, react,
};
use flux_project::{Project, ProjectError, VjOp};

/// The event with the project after a mapping changed it.
pub const VJ_CONTROL: &str = "vj-control";

/// The event with the mapping a learn just made.
pub const VJ_LEARNED: &str = "vj-learned";

/// The event with a key the learn refused because the app uses it (the source that was heard).
pub const VJ_REFUSED: &str = "vj-learn-refused";

/// The least time between two states sent while a control moves.
const EMIT_EVERY: Duration = Duration::from_millis(33);

/// How long a control must be still for its gesture to be closed.
const COMMIT_AFTER: Duration = Duration::from_millis(500);

/// How many taps the tempo is read from.
const TAPS: usize = 16;

/// The target the next control touched is to be mapped to, while the window is in map mode.
pub type SharedLearn = Arc<Mutex<Option<Target>>>;

/// What the window hears with [`VJ_CONTROL`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Controlled {
    project: Project,
    active: String,
    /// The gesture is over: this state is the one for the undo history.
    commit: bool,
}

/// What a gesture has to do at an instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Send the state, the gesture goes on.
    Send,
    /// Send the state and close the gesture.
    Commit,
    /// Nothing yet; look again after this long.
    Wait(Duration),
    /// Nothing to do until a value moves.
    Idle,
}

/// When the states of a moving control are sent: throttled while it moves, closed when it rests.
#[derive(Debug, Default)]
struct Gesture {
    /// A value moved since the last state was sent.
    dirty: bool,
    /// States were sent that no commit closed.
    open: bool,
    last_sent: Option<Instant>,
    last_moved: Option<Instant>,
}

impl Gesture {
    fn moved(&mut self, now: Instant) {
        self.dirty = true;
        self.open = true;
        self.last_moved = Some(now);
    }

    /// What to do at `now`; the caller does it and then calls [`Self::sent`] or [`Self::closed`].
    fn step(&self, now: Instant) -> Step {
        let since =
            |at: Option<Instant>| at.map_or(Duration::MAX, |at| now.saturating_duration_since(at));
        if self.dirty {
            let wait = EMIT_EVERY.saturating_sub(since(self.last_sent));
            return if wait.is_zero() {
                Step::Send
            } else {
                Step::Wait(wait)
            };
        }
        if self.open {
            let wait = COMMIT_AFTER.saturating_sub(since(self.last_moved));
            return if wait.is_zero() {
                Step::Commit
            } else {
                Step::Wait(wait)
            };
        }
        Step::Idle
    }

    fn sent(&mut self, now: Instant) {
        self.dirty = false;
        self.last_sent = Some(now);
    }

    fn closed(&mut self) {
        self.dirty = false;
        self.open = false;
    }
}

/// What the `flux-control` thread keeps to run the mappings.
pub struct VjRunner {
    app: AppHandle,
    learn: SharedLearn,
    /// The name of the MIDI port the controller is on.
    port: String,
    /// The mappings whose button is down.
    held: HashSet<String>,
    /// When each tap of the tempo was made, in milliseconds since this started.
    taps: Vec<f64>,
    started: Instant,
    gesture: Gesture,
    /// The last frame of each Art-Net universe, to tell what changed.
    dmx: DmxState,
    /// What the channels did since a DMX learn began.
    dmx_learn: DmxLearn,
}

impl VjRunner {
    pub fn new(app: AppHandle, learn: SharedLearn, port: String) -> Self {
        Self {
            app,
            learn,
            port,
            held: HashSet::new(),
            taps: Vec::new(),
            started: Instant::now(),
            gesture: Gesture::default(),
            dmx: DmxState::default(),
            dmx_learn: DmxLearn::default(),
        }
    }

    /// How long the thread may wait for the next message before it has to look at the gesture
    /// or decide a DMX learn.
    pub fn wait(&self) -> Option<Duration> {
        let now = Instant::now();
        let gesture = match self.gesture.step(now) {
            Step::Send | Step::Commit => Some(Duration::ZERO),
            Step::Wait(wait) => Some(wait),
            Step::Idle => None,
        };
        match (gesture, self.dmx_learn.due_in(now)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The inputs were closed: what is still open is closed, so the window keeps what was done.
    pub fn finish(&mut self) {
        if self.gesture.open {
            self.tell(true);
            self.gesture.closed();
        }
    }

    /// Sends or closes the gesture if its time has come.
    pub fn tick(&mut self) {
        let now = Instant::now();
        if let Some((universe, channel)) = self.dmx_learn.take(now) {
            self.learn_source(Source::Artnet {
                universe,
                channel,
                fine: None,
            });
        }
        match self.gesture.step(now) {
            Step::Send => {
                self.tell(false);
                self.gesture.sent(now);
            }
            Step::Commit => {
                self.tell(true);
                self.gesture.closed();
            }
            Step::Wait(_) | Step::Idle => {}
        }
    }

    /// Tells the window the project as it is. A gesture still open is closed by a discrete edit,
    /// which is a state of its own.
    fn tell(&self, commit: bool) {
        let state = self
            .app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .project_state();
        if let Some(state) = state {
            let controlled = Controlled {
                project: state.project,
                active: state.active,
                commit,
            };
            if let Err(err) = self.app.emit(VJ_CONTROL, controlled) {
                log::warn!("vj control: the window could not be told: {err}");
            }
        }
    }

    /// Whether the open project is a VJ show, the only one that has mappings.
    fn is_vj(&self) -> bool {
        self.app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_vj()
    }

    /// Takes a MIDI message for the VJ show: to learn, or to run the mappings it is the source of.
    /// `true` when it was taken, so the cue bindings do not also run it.
    pub fn midi(&mut self, message: &MidiMsg, commands: &mut Vec<(Target, Command)>) -> bool {
        let Some(heard) = message.heard(&self.port) else {
            return false;
        };
        self.hear(heard, commands)
    }

    /// Takes an OSC message for the VJ show: to learn, to run the mappings of its address, or the
    /// fixed addresses. `true` when it was taken.
    pub fn osc(&mut self, message: &OscMessage, commands: &mut Vec<(Target, Command)>) -> bool {
        // The cue addresses and the versioned API are not for mapping.
        if message.address.starts_with("/flux/") {
            return false;
        }
        let level = osc_level(&message.args);
        let heard = Heard {
            source: Source::Osc {
                address: message.address.clone(),
            },
            level: level.unwrap_or(1.0).clamp(0.0, 1.0),
            // A message with no value is a press; a value of 0 is a release.
            down: level.is_none_or(|level| level > 0.0),
        };
        if self.learn_from(&heard) {
            return true;
        }
        if self.resolve(&heard, commands) {
            return true;
        }
        let session = self.app.state::<SharedSession>();
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        let fixed = session
            .vj_project()
            .and_then(|project| osc_command(&project.vj, &message.address, &message.args));
        let taken = fixed.is_some();
        commands.extend(fixed);
        taken
    }

    /// Takes the values of an Art-Net universe: to learn the channel that moved, or to run the
    /// mappings of the channels that changed. Only a change is news (see [`artnet`]).
    pub fn dmx(&mut self, frame: &DmxFrame, commands: &mut Vec<(Target, Command)>) {
        let changes = self.dmx.update(frame);
        if changes.is_empty() {
            return;
        }
        let waiting = self
            .learn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        if waiting {
            self.dmx_learn
                .observe(frame.universe, &changes, Instant::now());
            return;
        }
        if !self.dmx_learn.is_idle() {
            self.dmx_learn.reset();
        }
        let sources: Vec<Source> = {
            let session = self.app.state::<SharedSession>();
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            session.vj_project().map_or_else(Vec::new, |project| {
                project
                    .vj
                    .mappings
                    .iter()
                    .map(|m| m.source.clone())
                    .collect()
            })
        };
        for heard in artnet::heard(&self.dmx, frame.universe, &changes, &sources) {
            self.resolve(&heard, commands);
        }
    }

    /// Takes a key of the keyboard: to learn, or to run the mappings of it.
    pub fn key(&mut self, message: &KeyMsg, commands: &mut Vec<(Target, Command)>) {
        self.hear(message.heard(), commands);
    }

    /// Learns `heard` if the window is waiting for a control; else runs its mappings.
    fn hear(&mut self, heard: Heard, commands: &mut Vec<(Target, Command)>) -> bool {
        self.learn_from(&heard) || self.resolve(&heard, commands)
    }

    /// Adds the commands of the mappings `heard` is the source of; `true` when there are any
    /// mappings of it, even when a release asks nothing.
    fn resolve(&mut self, heard: &Heard, commands: &mut Vec<(Target, Command)>) -> bool {
        let session = self.app.state::<SharedSession>();
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(project) = session.vj_project() else {
            return false;
        };
        commands.extend(react(&project.vj, heard, &mut self.held));
        project
            .vj
            .mappings
            .iter()
            .any(|m| m.source.matches(&heard.source))
    }

    /// Makes `heard` the source of the target the window waits to map, if there is one. A note
    /// that is let go does not count: the press came first.
    fn learn_from(&mut self, heard: &Heard) -> bool {
        let released_button = matches!(
            heard.source,
            Source::Midi {
                kind: flux_project::vj_mapping::MidiKind::Note,
                ..
            } | Source::Key { .. }
        ) && !heard.down;
        if released_button || !self.is_vj() {
            return false;
        }
        self.learn_source(heard.source.clone())
    }

    /// Makes `source` the source of the target the window waits to map. A key the app uses
    /// itself is refused (the window is told and the wait goes on); `true` when it was taken.
    fn learn_source(&mut self, source: Source) -> bool {
        if !self.is_vj() {
            return false;
        }
        if source.is_app_shortcut()
            && self
                .learn
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        {
            log::info!("vj control: {source:?} is used by the app, not learned");
            let _ = self.app.emit(VJ_REFUSED, &source);
            return true;
        }
        let Some(target) = self
            .learn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return false;
        };
        let op = VjOp::AddMapping {
            source: source.clone(),
            target,
        };
        match self.edit(op) {
            Ok(()) => {
                log::info!("vj control: learned {source:?}");
                let _ = self.app.emit(VJ_LEARNED, &source);
            }
            Err(err) => log::warn!("vj control: the control was not learned: {err}"),
        }
        true
    }

    /// Runs the commands of one batch of messages, the last value of each target of a burst only.
    pub fn run(&mut self, commands: Vec<(Target, Command)>) {
        for (target, command) in coalesce(commands) {
            let planned = {
                let session = self.app.state::<SharedSession>();
                let session = session.lock().unwrap_or_else(PoisonError::into_inner);
                session
                    .vj_project()
                    .and_then(|project| plan(&project.vj, &target, command))
            };
            let Some(planned) = planned else {
                log::debug!("vj control: {command:?} asks nothing of {target:?}");
                continue;
            };
            let result = match planned {
                Plan::Edit(op) => self.edit(op),
                Plan::Play(action) => self.play(action),
                Plan::Tap => self.tap(),
            };
            if let Err(err) = result {
                log::debug!("vj control: {target:?} {command:?} failed: {err}");
            }
        }
    }

    /// Applies an edit of the project as the window's own would be: a value in place, anything
    /// else through the session's check, and the window told.
    fn edit(&mut self, op: VjOp) -> Result<(), ProjectError> {
        let preview = op.is_preview();
        {
            let session = self.app.state::<SharedSession>();
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            if preview {
                session.preview_vj(op)?;
            } else {
                session.edit_vj(op)?;
            }
        }
        // A level, a side, a bypass or a solo changes how loud the playing clips are.
        crate::modulation::wake();
        crate::vj_audio::refresh(&self.app);
        if preview {
            self.gesture.moved(Instant::now());
        } else {
            // Closes a gesture that was open, and is a step of its own.
            self.tell(true);
            self.gesture.closed();
        }
        Ok(())
    }

    fn tap(&mut self) -> Result<(), ProjectError> {
        self.taps
            .push(self.started.elapsed().as_secs_f64() * 1000.0);
        if self.taps.len() > TAPS {
            self.taps.remove(0);
        }
        self.edit(VjOp::TapBpm {
            taps: self.taps.clone(),
        })
    }

    fn play(&self, action: PlayAction) -> Result<(), ProjectError> {
        let app = &self.app;
        let session = app.state::<SharedSession>();
        let grid = app.state::<SharedGrid>();
        let missing = |what: &str| ProjectError::Invalid(format!("no {what}"));
        match action {
            PlayAction::Launch(clip) => play(app, &session, &grid, |play, vj, now| {
                play.launch(vj, &clip, now)
                    .then_some(())
                    .ok_or_else(|| missing("clip"))
            }),
            PlayAction::Release(clip) => play(app, &session, &grid, |play, vj, now| {
                play.release(vj, &clip, now)
                    .then_some(())
                    .ok_or_else(|| missing("clip"))
            }),
            PlayAction::LaunchColumn(column) => play(app, &session, &grid, |play, vj, now| {
                play.launch_column(vj, &column, now)
                    .then_some(())
                    .ok_or_else(|| missing("column"))
            }),
            PlayAction::Stop(layer) => play(app, &session, &grid, |play, vj, now| {
                play.stop(vj, layer.as_deref(), now);
                Ok(())
            }),
            PlayAction::Crossfader(position) => play(app, &session, &grid, |play, _, _| {
                play.set_crossfader(position);
                Ok(())
            }),
            PlayAction::BeatPause(on) => {
                let on = on.unwrap_or_else(|| {
                    !grid.lock().unwrap_or_else(PoisonError::into_inner).paused()
                });
                beat(app, &session, &grid, BeatOp::Pause { on })
            }
            PlayAction::Resync => beat(app, &session, &grid, BeatOp::Resync),
            PlayAction::Nudge(direction) => beat(app, &session, &grid, BeatOp::Nudge { direction }),
        }
        .map(|_| ())
    }
}

/// Waits for the next control the user touches, or the next OSC message, and makes it the source
/// of `target`; `None` stops waiting. The window asks while it is in map mode.
#[tauri::command]
pub fn vj_learn(control: tauri::State<'_, crate::control::SharedControl>, target: Option<Target>) {
    *control
        .vj_learn()
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = target;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        // One fixed origin: instants are only compared with each other.
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *ORIGIN.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn a_resting_gesture_should_do_nothing() {
        assert_eq!(Gesture::default().step(at(0)), Step::Idle);
    }

    #[test]
    fn a_moving_control_should_send_at_once_then_at_most_every_33_ms() {
        let mut gesture = Gesture::default();
        gesture.moved(at(0));
        assert_eq!(gesture.step(at(0)), Step::Send);
        gesture.sent(at(0));
        gesture.moved(at(10));
        assert_eq!(gesture.step(at(10)), Step::Wait(Duration::from_millis(23)));
        assert_eq!(gesture.step(at(40)), Step::Send);
    }

    #[test]
    fn a_control_at_rest_should_close_its_gesture_once() {
        let mut gesture = Gesture::default();
        gesture.moved(at(0));
        gesture.sent(at(0));
        assert_eq!(
            gesture.step(at(100)),
            Step::Wait(Duration::from_millis(400))
        );
        assert_eq!(gesture.step(at(500)), Step::Commit);
        gesture.closed();
        assert_eq!(gesture.step(at(900)), Step::Idle);
    }

    #[test]
    fn a_value_that_moves_again_should_keep_the_gesture_open() {
        let mut gesture = Gesture::default();
        gesture.moved(at(0));
        gesture.sent(at(0));
        gesture.moved(at(400));
        gesture.sent(at(400));
        assert_eq!(
            gesture.step(at(600)),
            Step::Wait(Duration::from_millis(300)),
            "the 500 ms count from the last move"
        );
    }
}
