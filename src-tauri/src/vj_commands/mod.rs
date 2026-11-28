//! Playing the VJ grid of a VJ project: launching clips and columns, stopping layers and moving the
//! crossfader. What plays is the running show's and is never stored; the video output's render
//! threads read it on every refresh (see `video_output/`).

use crate::commands::SharedSession;
use crate::video_commands::known_probe;
use crate::watch::Watches;
use flux_project::playhead::{Playhead, Tick};
use flux_project::vj::{CUES, Playback, Region, Vj};
use flux_project::{ProjectError, Result};
use flux_video::{GridPlay, Rate, video_seconds};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

pub(crate) mod beat;
pub(crate) mod frames;
pub(crate) mod launch;
mod play;

pub use self::beat::BeatOp;
pub(crate) use self::beat::beat;
pub(crate) use self::play::play;

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
    /// The Link session the show is in: the beat count keeps its phase while it runs.
    link: Option<Arc<flux_link::Link>>,
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
            link: None,
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

    /// Joins the Link session `link` (or leaves the one the show is in), counting the beats up to
    /// now at the tempo they had.
    pub fn set_link(&mut self, link: Option<Arc<flux_link::Link>>, bpm: f64) {
        self.beats(bpm);
        self.link = link;
    }

    /// Puts the beat count on the first beat of a bar now, the nearest one. In a Link session the
    /// bar is the session's and no peer is moved, so there is nothing to do.
    pub fn resync(&mut self, bpm: f64) {
        if self.link.is_some() {
            return;
        }
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
        let mut counted = beats + now.saturating_sub(at).as_secs_f64() * bpm / 60.0 * self.rate();
        // In a Link session the count keeps the phase of the session's bar of four, as long as it
        // runs at the tempo: held or nudged beats are the operator's own.
        if let Some(link) = &self.link
            && self.rate() == 1.0
        {
            counted = flux_link::align(counted, link.beat(), flux_link::QUANTUM);
        }
        self.beat = (now.max(at), counted);
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
    /// How many other applications are in the Ableton Link session, while the show is in one.
    pub link_peers: Option<u64>,
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

#[cfg(test)]
mod tests;
