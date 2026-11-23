//! Playing the VJ grid of a VJ project: launching clips and columns, stopping layers and moving the
//! crossfader. What plays is the running show's and is never stored; the video output's render
//! threads read it on every refresh (see `video_output.rs`).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::commands::SharedSession;
use flux_project::vj::Vj;
use flux_project::{ProjectError, Result};
use flux_video::GridPlay;

/// What the grid plays, and the clock its clips play on.
pub struct Grid {
    pub play: GridPlay,
    started: Instant,
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            play: GridPlay::default(),
            started: Instant::now(),
        }
    }
}

impl Grid {
    /// Where the grid's clock is.
    pub fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

pub type SharedGrid = Arc<Mutex<Grid>>;

/// What the grid plays, for the window to draw.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridState {
    pub playing: Vec<PlayingClip>,
    pub crossfader: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingClip {
    pub layer: String,
    pub clip: String,
    /// Since it was launched.
    pub seconds: f64,
}

fn grid_of(session: &SharedSession) -> Result<Vj> {
    session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .grid_copy()
        .ok_or_else(|| ProjectError::Invalid("only a VJ project has a grid".into()))
}

/// Applies `change` to what the grid plays, with the grid as the project has it, and answers with
/// the new state.
fn play(
    session: &SharedSession,
    grid: &SharedGrid,
    change: impl FnOnce(&mut GridPlay, &Vj, Duration) -> Result<()>,
) -> Result<GridState> {
    let vj = grid_of(session)?;
    let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
    let now = grid.now();
    grid.play.retain(&vj);
    change(&mut grid.play, &vj, now)?;
    Ok(state(&grid))
}

fn state(grid: &Grid) -> GridState {
    let now = grid.now();
    let mut playing: Vec<PlayingClip> = grid
        .play
        .playing()
        .iter()
        .map(|(layer, launched)| PlayingClip {
            layer: layer.clone(),
            clip: launched.clip.clone(),
            seconds: now.saturating_sub(launched.at).as_secs_f64(),
        })
        .collect();
    playing.sort_by(|a, b| a.layer.cmp(&b.layer));
    GridState {
        playing,
        crossfader: grid.play.crossfader(),
    }
}

/// Plays a clip of the grid on its layer from its first frame; a playing clip starts again.
#[tauri::command]
pub fn vj_launch(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&session, &grid, |play, vj, now| {
        if play.launch(vj, &clip, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Launches every clip of a column; the layers without a clip in it stop.
#[tauri::command]
pub fn vj_launch_column(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    column: String,
) -> Result<GridState> {
    play(&session, &grid, |play, vj, now| {
        if play.launch_column(vj, &column, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no column {column}")))
        }
    })
}

/// Stops a layer, or every layer without one.
#[tauri::command]
pub fn vj_stop(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    layer: Option<String>,
) -> Result<GridState> {
    play(&session, &grid, |play, _, _| {
        play.stop(layer.as_deref());
        Ok(())
    })
}

/// Moves the crossfader: 0 is all side A, 1 all side B.
#[tauri::command]
pub fn vj_crossfader(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    position: f32,
) -> Result<GridState> {
    play(&session, &grid, |play, _, _| {
        play.set_crossfader(position);
        Ok(())
    })
}

/// What the grid plays now.
#[tauri::command]
pub fn vj_play_state(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
) -> Result<GridState> {
    play(&session, &grid, |_, _, _| Ok(()))
}
