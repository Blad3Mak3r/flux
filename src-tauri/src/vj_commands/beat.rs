use super::*;

/// What the operator does to the beat clock: runtime state of the grid, never saved.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum BeatOp {
    /// Holds the beats, or lets them go on.
    Pause { on: bool },
    /// Puts the beats on the first of a bar now.
    Resync,
    /// Runs the beats faster (1), slower (-1) or at the tempo (0) while a nudge is held.
    Nudge { direction: i8 },
}

/// Pauses, resyncs or nudges the beat clock, and answers with what the grid plays.
#[tauri::command]
pub fn vj_beat(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    op: BeatOp,
) -> Result<GridState> {
    beat(&app, &session, &grid, op)
}

/// Does `op` to the beat clock, as [`vj_beat`] and a controller mapped to a tempo button do.
pub(crate) fn beat(
    app: &AppHandle,
    session: &SharedSession,
    grid: &SharedGrid,
    op: BeatOp,
) -> Result<GridState> {
    let bpm = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .tempo_bpm();
    {
        let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
        match op {
            BeatOp::Pause { on } => grid.set_paused(on, bpm),
            BeatOp::Resync => grid.resync(bpm),
            BeatOp::Nudge { direction } => grid.nudge(direction, bpm),
        }
    }
    play(app, session, grid, |_, _, _| Ok(()))
}

/// How often the window is told what plays, at most: thirty times a second.
const PLAY_EVERY: Duration = Duration::from_millis(33);

/// The play lane: what the grid plays is sent when it differs from what was sent last. The window
/// shows the beat, not its fraction, so beats count as whole ones.
#[derive(Default)]
pub(super) struct PlayLane {
    last: Option<GridState>,
}

impl PlayLane {
    pub(super) fn next(&mut self, state: GridState) -> Option<GridState> {
        let held = GridState {
            beats: state.beats.floor(),
            ..state.clone()
        };
        if self.last.as_ref() == Some(&held) {
            return None;
        }
        self.last = Some(held);
        Some(state)
    }
}

/// Sends what the grid plays down `on_play` whenever it changes, until [`crate::watch::watch_stop`].
#[tauri::command]
pub fn vj_play_watch(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    watches: State<'_, Watches>,
    on_play: Channel<GridState>,
) {
    let (session, grid) = (Arc::clone(&session), Arc::clone(&grid));
    let mut lane = PlayLane::default();
    watches.start(on_play, PLAY_EVERY, move || {
        play(&app, &session, &grid, |_, _, _| Ok(()))
            .ok()
            .and_then(|state| lane.next(state))
    });
}

/// The colours a pad gets on a controller before the user changes them, and the name of the colour
/// table its velocities pick from, if it has one (see `flux_project::vj_feedback`).
#[derive(serde::Serialize)]
pub struct LightsPreset {
    lights: flux_project::vj_feedback::Lights,
    palette: Option<&'static str>,
}

#[tauri::command]
pub fn vj_lights_preset(port: Option<String>) -> LightsPreset {
    use flux_project::vj_feedback::Lights;
    let port = port.unwrap_or_default();
    LightsPreset {
        lights: Lights::preset(&port),
        palette: Lights::palette(&port),
    }
}
