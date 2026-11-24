//! Playing the VJ grid of a VJ project: launching clips and columns, stopping layers and moving the
//! crossfader. What plays is the running show's and is never stored; the video output's render
//! threads read it on every refresh (see `video_output.rs`).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::State;

use crate::commands::SharedSession;
use crate::video_commands::known_probe;
use flux_project::vj::Vj;
use flux_project::{ProjectError, Result};
use flux_video::{GridPlay, Rate, grid_frame};

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
    /// The column launched last, while nothing was launched or stopped by hand since.
    pub column: Option<String>,
    pub crossfader: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingClip {
    pub layer: String,
    pub clip: String,
    /// Since it was launched.
    pub seconds: f64,
    /// Where it is in its video, 0 to 1, and how long until it reaches the end of it, in seconds;
    /// `None` until the video's length is known, or once a clip played once has ended.
    pub progress: Option<f64>,
    pub remaining: Option<f64>,
    /// How much of the layer shows, 0 to 1 (see [`GridPlay::level`]).
    pub level: f32,
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
    Ok(state(session, &vj, &grid))
}

fn state(session: &SharedSession, vj: &Vj, grid: &Grid) -> GridState {
    let now = grid.now();
    let videos = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .videos_copy();
    let mut playing: Vec<PlayingClip> = grid
        .play
        .playing()
        .iter()
        .map(|(layer, launched)| {
            let elapsed = now.saturating_sub(launched.at);
            let place = vj
                .find_clip(&launched.clip)
                .and_then(|(_, clip)| {
                    let asset = videos.iter().find(|v| v.id == clip.asset)?;
                    Some((clip, known_probe(session, asset)?))
                })
                .and_then(|(clip, probe)| {
                    let rate = Rate::new(probe.rate_num, probe.rate_den);
                    let frame = grid_frame(elapsed, rate, probe.frames, clip.playback)?;
                    let per_frame = f64::from(probe.rate_den) / f64::from(probe.rate_num.max(1));
                    let frames = probe.frames.max(1) as f64;
                    Some((frame as f64 / frames, (frames - frame as f64) * per_frame))
                });
            let level = vj
                .layers
                .iter()
                .find(|l| &l.id == layer)
                .map_or(0.0, |l| grid.play.level(vj, l));
            PlayingClip {
                level,
                layer: layer.clone(),
                clip: launched.clip.clone(),
                seconds: elapsed.as_secs_f64(),
                progress: place.map(|p| p.0),
                remaining: place.map(|p| p.1),
            }
        })
        .collect();
    playing.sort_by(|a, b| a.layer.cmp(&b.layer));
    GridState {
        playing,
        column: grid.play.column().map(str::to_owned),
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

/// Lets go of a clip: one triggered as a piano key stops.
#[tauri::command]
pub fn vj_release(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&session, &grid, |play, vj, _| {
        if play.release(vj, &clip) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Launches every clip of a column; the deck layers without a clip in it stop, global layers play on.
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
