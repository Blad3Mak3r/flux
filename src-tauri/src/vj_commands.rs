//! Playing the VJ grid of a VJ project: launching clips and columns, stopping layers and moving the
//! crossfader. What plays is the running show's and is never stored; the video output's render
//! threads read it on every refresh (see `video_output.rs`).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::SharedSession;
use crate::video_commands::known_probe;
use flux_project::playhead::{Playhead, Tick};
use flux_project::vj::{CUES, Playback, Region, Vj};
use flux_project::{ProjectError, Result};
use flux_video::{GridPlay, Rate, video_seconds};

/// What the grid plays, and the clock its clips play on.
pub struct Grid {
    pub play: GridPlay,
    started: Instant,
    /// Where the beats were counted to, and how many there were by then.
    beat: (Duration, f64),
    /// The beat count holds while it is paused: only the beats stop, the seconds go on.
    paused: bool,
    /// How much faster (or slower) than the tempo the beats run: 1, except while a nudge is held.
    pace: f64,
    /// An autopilot launched a clip since [`Self::take_advanced`]: the sound has to follow.
    advanced: bool,
}

/// How far a nudge moves the pace of the beats, as a fraction of the tempo.
const NUDGE: f64 = 0.1;
/// Beats in a bar, which a resync lands on the first of.
const BAR: f64 = 4.0;

impl Default for Grid {
    fn default() -> Self {
        Self {
            play: GridPlay::default(),
            started: Instant::now(),
            beat: (Duration::ZERO, 0.0),
            paused: false,
            pace: 1.0,
            advanced: false,
        }
    }
}

impl Grid {
    /// The instant the grid's clock started.
    pub fn origin(&self) -> Instant {
        self.started
    }

    /// Where the grid's clock is.
    pub fn now(&self) -> Duration {
        self.started.elapsed()
    }

    /// How many beats the count moves for one at the tempo: none while paused.
    fn rate(&self) -> f64 {
        if self.paused { 0.0 } else { self.pace }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Holds the beat count, or lets it go on from where it stopped, so a beat-synced clip keeps its phase.
    pub fn set_paused(&mut self, on: bool, bpm: f64) {
        self.beats(bpm);
        self.paused = on;
    }

    /// Puts the beat count on the first beat of a bar now, the nearest one.
    pub fn resync(&mut self, bpm: f64) {
        let beats = self.beats(bpm);
        self.beat.1 = (beats / BAR).round() * BAR;
    }

    /// Runs the beats a little faster (`direction` above 0), slower (below 0) or at the tempo (0), to
    /// move their phase against the music without changing the tempo.
    pub fn nudge(&mut self, direction: i8, bpm: f64) {
        self.beats(bpm);
        self.pace = 1.0 + NUDGE * f64::from(direction.signum());
    }

    /// The beats the grid's clock has counted, at `bpm` beats a minute now: a count that keeps its
    /// phase when the tempo changes, as the beats before it were counted at the tempo they had.
    /// Every screen asks for it, each a little after the last.
    pub fn beats(&mut self, bpm: f64) -> f64 {
        self.count(self.now(), bpm)
    }

    fn count(&mut self, now: Duration, bpm: f64) -> f64 {
        let (at, beats) = self.beat;
        self.beat = (
            now.max(at),
            beats + now.saturating_sub(at).as_secs_f64() * bpm / 60.0 * self.rate(),
        );
        self.beat.1
    }

    /// The grid's clock now, with the beats it counted, after the playing clips followed the
    /// grid `vj` as it is: a clip whose speed, direction, tempo or region changed goes on from
    /// where it was. Every screen and the sound ask for it before they look at what plays.
    pub fn tick(&mut self, vj: &Vj, bpm: f64) -> Tick {
        let time = self.now();
        let tick = Tick {
            time,
            beats: self.count(time, bpm),
        };
        let length = |asset: &str| {
            let probe = crate::video_commands::probed(asset)?;
            Some((
                video_seconds(Rate::new(probe.rate_num, probe.rate_den), probe.frames),
                probe.still,
            ))
        };
        self.play.follow(vj, tick, &length);
        // A clip ending its pass makes the layer launch the next, whichever screen asked first.
        self.advanced |= self.play.autopilot(vj, tick, &length);
        tick
    }

    /// Whether an autopilot launched a clip since the last call.
    pub fn take_advanced(&mut self) -> bool {
        std::mem::take(&mut self.advanced)
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
    /// The beats the grid's clock has counted, and whether they are held.
    pub beats: f64,
    pub beats_paused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingClip {
    pub layer: String,
    pub clip: String,
    /// Since it was launched.
    pub seconds: f64,
    /// Where the playhead is in the whole clip, 0 to 1; `None` until the video's length is known,
    /// for a generator, or once a clip played once has ended.
    pub position: Option<f64>,
    /// Where it is in its region, 0 to 1, and how long until it reaches the edge it moves towards,
    /// in seconds, at the pace it has now; `None` as `position` is, and while it is paused.
    pub progress: Option<f64>,
    pub remaining: Option<f64>,
    /// The region and the cues of the clip, as fractions of its length.
    pub region: Region,
    pub cues: [Option<f32>; CUES],
    /// How many seconds of the clip play in a second, negative backwards, 0 paused: what the
    /// speed, direction and tempo come to.
    pub rate: f64,
    /// How long one pass through the region lasts, in seconds; `None` while paused.
    pub pass_seconds: Option<f64>,
    /// How much of the clip shows, 0 to 1: its layer's level (see [`GridPlay::level`]) and where
    /// the layer's transition is.
    pub level: f32,
    /// How far the layer's transition is, 0 to 1, while one runs.
    pub transition: Option<f32>,
    /// Whether the layer is fading out to nothing, showing the clip that played.
    pub stopping: bool,
    /// For a text block that shows one line at a time, the line on screen now (from 0), which
    /// `advance` moves on from the line stored in the clip.
    pub line: Option<u32>,
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
    app: &AppHandle,
    session: &SharedSession,
    grid: &SharedGrid,
    change: impl FnOnce(&mut GridPlay, &Vj, Tick) -> Result<()>,
) -> Result<GridState> {
    let vj = grid_of(session)?;
    let bpm = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .tempo_bpm();
    let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
    let before = grid.play.clone();
    grid.play.retain(&vj);
    let now = grid.tick(&vj, bpm);
    grid.play.retime(&vj, now.time);
    change(&mut grid.play, &vj, now)?;
    let answer = state(session, &vj, &grid, now, bpm);
    let advanced = grid.take_advanced();
    let changed = advanced || grid.play != before;
    drop(grid);
    // The window asks for the state all the time; the sound changes only when what plays does.
    if changed {
        crate::vj_audio::refresh(app);
    }
    Ok(answer)
}

/// Where a playing clip is and how it moves, for the window to draw its transport.
#[derive(Debug, PartialEq)]
struct Report {
    position: Option<f64>,
    progress: Option<f64>,
    remaining: Option<f64>,
    rate: f64,
    pass_seconds: Option<f64>,
}

impl Report {
    /// `length` is the video's, in seconds; `None` while it is not known, and for a generator.
    fn of(head: &Playhead, tick: Tick, bpm: f64, length: Option<f64>) -> Self {
        let motion = head.motion();
        let Some(length) = length else {
            return Self {
                position: None,
                progress: None,
                remaining: None,
                rate: motion.direction.sign() * f64::from(motion.speed),
                pass_seconds: None,
            };
        };
        let u = head.phase(tick, length);
        let position = motion.place(u);
        let (start, span) = (
            f64::from(motion.region.start),
            f64::from(motion.region.span()),
        );
        let progress = position.map(|x| (x - start) / span);
        let speed = motion.rate(length, bpm);
        // Which edge of the region it moves towards: a ping-pong on its way back turns.
        let back = motion.playback == Playback::PingPong && u.rem_euclid(2.0) > 1.0;
        let towards = if back { -speed } else { speed };
        let remaining = progress
            .filter(|_| speed != 0.0)
            .map(|inside| if towards > 0.0 { 1.0 - inside } else { inside } / speed.abs());
        Self {
            position,
            progress,
            remaining,
            rate: speed * span * length,
            pass_seconds: motion.pass_seconds(length, bpm),
        }
    }
}

fn state(session: &SharedSession, vj: &Vj, grid: &Grid, tick: Tick, bpm: f64) -> GridState {
    let now = tick.time;
    let mut playing: Vec<PlayingClip> = grid
        .play
        .playing()
        .iter()
        .filter_map(|(layer, launched)| {
            let (shown, at, head) = launched.shown(now)?;
            let elapsed = now.saturating_sub(at);
            let (_, clip) = vj.find_clip(shown)?;
            // Only the videos playing are copied, not the whole list on every poll. A generator
            // and a still image last the clip's duration.
            let duration = f64::from(clip.duration);
            let length = if clip.generator.is_some() {
                Some(duration)
            } else {
                let asset = session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .video_copy(&clip.asset);
                asset
                    .and_then(|asset| known_probe(session, &asset))
                    .map(|probe| {
                        if probe.still {
                            duration
                        } else {
                            video_seconds(Rate::new(probe.rate_num, probe.rate_den), probe.frames)
                        }
                    })
            };
            let report = Report::of(head, tick, bpm, length);
            let transition = launched.fading(now).map(|fade| fade.progress(now));
            let stopping = launched.clip.is_none();
            let fade = transition.map_or(1.0, |t| if stopping { 1.0 - t } else { t });
            let level = vj
                .layers
                .iter()
                .find(|l| &l.id == layer)
                .map_or(0.0, |l| grid.play.level(vj, l));
            Some(PlayingClip {
                level: level * fade,
                layer: layer.clone(),
                clip: shown.to_owned(),
                seconds: elapsed.as_secs_f64(),
                position: report.position,
                progress: report.progress,
                remaining: report.remaining,
                region: clip.region,
                cues: clip.cues,
                rate: report.rate,
                pass_seconds: report.pass_seconds,
                transition,
                stopping,
                line: clip
                    .generator
                    .as_ref()
                    .and_then(|generator| generator.shown_line(tick.beats))
                    .map(|line| line as u32),
            })
        })
        .collect();
    playing.sort_by(|a, b| a.layer.cmp(&b.layer));
    GridState {
        playing,
        column: grid.play.column().map(str::to_owned),
        crossfader: grid.play.crossfader(),
        beats: tick.beats,
        beats_paused: grid.paused(),
    }
}

/// Plays a clip of the grid on its layer from its first frame; a playing clip starts again.
#[tauri::command]
pub fn vj_launch(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.launch(vj, &clip, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Moves the playhead of the clip playing on a layer to `position`, a fraction of the clip's
/// length (brought into its region), as the hand scrubs.
#[tauri::command]
pub fn vj_scrub(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    layer: String,
    position: f64,
) -> Result<GridState> {
    if !position.is_finite() {
        return Err(ProjectError::Invalid("a position is a number".into()));
    }
    play(&app, &session, &grid, |play, _, now| {
        if play.scrub(&layer, position.clamp(0.0, 1.0), now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip plays on {layer}")))
        }
    })
}

/// Jumps to cue number `index` (0 to 7) of a clip: the clip's playhead moves there if it plays,
/// and the clip is launched there if it does not.
#[tauri::command]
pub fn vj_cue(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
    index: usize,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        play.cue(vj, &clip, index, now)
            .ok_or_else(|| ProjectError::Invalid(format!("clip {clip} has no cue {}", index + 1)))
    })
}

/// Lets go of a clip: one triggered as a piano key stops.
#[tauri::command]
pub fn vj_release(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    clip: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        if play.release(vj, &clip, now) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no clip {clip}")))
        }
    })
}

/// Launches every clip of a column; the deck layers without a clip in it stop, global layers play on.
#[tauri::command]
pub fn vj_launch_column(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    column: String,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
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
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    layer: Option<String>,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, vj, now| {
        play.stop(vj, layer.as_deref(), now);
        Ok(())
    })
}

/// Moves the crossfader: 0 is all side A, 1 all side B.
#[tauri::command]
pub fn vj_crossfader(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    position: f32,
) -> Result<GridState> {
    play(&app, &session, &grid, |play, _, _| {
        play.set_crossfader(position);
        Ok(())
    })
}

/// What the grid sends to the screens now, before their mapping, as a small picture for the
/// editor's output monitor (see [`crate::video_output::VideoOutputs::composition`]); empty outside
/// a VJ project. Sent as bytes, not as JSON, so it is cheap enough to ask for thirty times a second.
#[tauri::command]
pub async fn vj_output_frame(
    app: tauri::AppHandle,
    output: State<'_, crate::video_output::SharedVideoOutput>,
) -> std::result::Result<tauri::ipc::Response, String> {
    let output = std::sync::Arc::clone(&output);
    let picture = tauri::async_runtime::spawn_blocking(move || output.composition(&app))
        .await
        .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(picture.unwrap_or_default()))
}

/// What the selected clip alone looks like in the composition, as a small picture in the format of
/// [`vj_output_frame`] (see [`crate::video_output::VideoOutputs::preview`]); it plays on its own,
/// whatever is on air. `None` stops it; empty when there is no such clip.
#[tauri::command]
pub async fn vj_preview_frame(
    app: tauri::AppHandle,
    output: State<'_, crate::video_output::SharedVideoOutput>,
    clip: Option<String>,
) -> std::result::Result<tauri::ipc::Response, String> {
    let output = std::sync::Arc::clone(&output);
    let picture =
        tauri::async_runtime::spawn_blocking(move || output.preview(&app, clip.as_deref()))
            .await
            .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(picture))
}

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
    play(&app, &session, &grid, |_, _, _| Ok(()))
}

/// What the grid plays now.
#[tauri::command]
pub fn vj_play_state(
    app: AppHandle,
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
) -> Result<GridState> {
    play(&app, &session, &grid, |_, _, _| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(seconds: f64) -> Duration {
        Duration::from_secs_f64(seconds)
    }

    #[test]
    fn beats_should_hold_while_paused_and_go_on_from_there() {
        let mut grid = Grid::default();
        assert_eq!(grid.count(secs(1.0), 120.0), 2.0);
        grid.paused = true;
        assert_eq!(grid.count(secs(5.0), 120.0), 2.0, "held");
        grid.paused = false;
        assert_eq!(
            grid.count(secs(5.0), 120.0),
            2.0,
            "from the time it resumed"
        );
        assert_eq!(grid.count(secs(6.0), 120.0), 4.0);
    }

    fn running(seconds: u64) -> Grid {
        Grid {
            started: Instant::now() - Duration::from_secs(seconds),
            ..Grid::default()
        }
    }

    #[test]
    fn set_paused_should_hold_the_count_of_the_clock_that_runs() {
        let mut grid = running(1);
        grid.set_paused(true, 120.0);
        let held = grid.beats(120.0);
        assert!((2.0..2.1).contains(&held));
        assert_eq!(grid.beats(120.0), held);
    }

    #[test]
    fn resync_should_land_on_the_nearest_bar() {
        let mut grid = running(3);
        grid.resync(120.0);
        assert_eq!(
            grid.beat.1, 8.0,
            "six beats and a bit is nearer eight than four"
        );
    }

    #[test]
    fn nudge_should_change_the_pace_only_while_held() {
        let mut grid = Grid {
            pace: 1.0 + NUDGE,
            ..Grid::default()
        };
        assert!((grid.count(secs(1.0), 120.0) - 2.2).abs() < 1e-9);
        grid.pace = 1.0;
        assert!((grid.count(secs(2.0), 120.0) - 4.2).abs() < 1e-9);
        grid.nudge(-1, 120.0);
        assert_eq!(grid.pace, 1.0 - NUDGE);
        grid.nudge(0, 120.0);
        assert_eq!(grid.pace, 1.0);
    }
    use flux_project::playhead::Motion;
    use flux_project::vj::{Direction, Tempo};

    fn head(motion: Motion) -> Playhead {
        Playhead::launch(motion, Tick::default())
    }

    fn motion() -> Motion {
        Motion {
            speed: 1.0,
            direction: Direction::Forward,
            tempo: Tempo::Free,
            region: Region::default(),
            playback: Playback::Loop,
        }
    }

    fn at(seconds: f64, beats: f64) -> Tick {
        Tick {
            time: Duration::from_secs_f64(seconds),
            beats,
        }
    }

    #[test]
    fn the_report_should_say_where_the_playhead_is_in_the_clip_and_in_its_region() {
        let region = Region {
            start: 0.5,
            end: 1.0,
        };
        let moving = head(Motion { region, ..motion() });
        // A video of 10 s: the region lasts 5.
        let report = Report::of(&moving, at(1.0, 0.0), 120.0, Some(10.0));
        assert!((report.position.unwrap() - 0.6).abs() < 1e-9);
        assert!((report.progress.unwrap() - 0.2).abs() < 1e-9);
        assert!((report.remaining.unwrap() - 4.0).abs() < 1e-9);
        assert!((report.rate - 1.0).abs() < 1e-9);
        assert!((report.pass_seconds.unwrap() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn the_report_should_count_down_to_the_edge_it_moves_towards_at_the_pace_it_has() {
        let reverse = head(Motion {
            direction: Direction::Reverse,
            speed: 2.0,
            ..motion()
        });
        let report = Report::of(&reverse, at(1.0, 0.0), 120.0, Some(10.0));
        assert!((report.progress.unwrap() - 0.8).abs() < 1e-9);
        assert!(
            (report.remaining.unwrap() - 4.0).abs() < 1e-9,
            "to the start, twice as fast"
        );
        assert!((report.rate + 2.0).abs() < 1e-9);
        // A ping-pong on its way back goes to the start although it plays forwards.
        let bounce = head(Motion {
            playback: Playback::PingPong,
            ..motion()
        });
        let report = Report::of(&bounce, at(12.5, 0.0), 120.0, Some(10.0));
        assert!((report.progress.unwrap() - 0.75).abs() < 1e-9);
        assert!((report.remaining.unwrap() - 7.5).abs() < 1e-9);
    }

    #[test]
    fn the_report_should_follow_the_beats_and_be_still_while_paused_or_ended() {
        let beats = head(Motion {
            tempo: Tempo::Beats { beats: 8 },
            speed: 4.0,
            ..motion()
        });
        let report = Report::of(&beats, at(1.0, 2.0), 120.0, Some(10.0));
        assert!((report.pass_seconds.unwrap() - 4.0).abs() < 1e-9);
        assert!((report.progress.unwrap() - 0.25).abs() < 1e-9);
        assert!((report.remaining.unwrap() - 3.0).abs() < 1e-9);
        assert!((report.rate - 2.5).abs() < 1e-9, "10 s of video in 4 s");
        let paused = head(Motion {
            direction: Direction::Pause,
            ..motion()
        });
        let report = Report::of(&paused, at(5.0, 0.0), 120.0, Some(10.0));
        assert_eq!(
            (report.remaining, report.pass_seconds, report.rate),
            (None, None, 0.0)
        );
        assert_eq!(report.position, Some(0.0));
        let once = head(Motion {
            playback: Playback::Once,
            ..motion()
        });
        let report = Report::of(&once, at(11.0, 0.0), 120.0, Some(10.0));
        assert_eq!(
            (report.position, report.progress, report.remaining),
            (None, None, None)
        );
        let unknown = Report::of(&once, at(1.0, 0.0), 120.0, None);
        assert_eq!(
            (unknown.position, unknown.pass_seconds, unknown.rate),
            (None, None, 1.0)
        );
    }
}
