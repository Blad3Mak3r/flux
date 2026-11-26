//! Modulation of the VJ grid while it plays (see `flux_project::vj_modulation`): every frame step
//! and the grid's sound ask for a copy of the grid with its modulated values in place of the
//! stored ones, evaluated on the composition's clock and the analysis' bands.
//!
//! The layer `volume` and `master` reach the grid's sound, which is mixed from a [`GridMix`]
//! built ahead of the audio callback: while any of them is modulated, a timer rebuilds it thirty
//! times a second, and the callback smooths the gain between two builds. Nothing is evaluated in
//! the callback.
//!
//! [`GridMix`]: flux_engine::grid_audio::GridMix

use std::borrow::Cow;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use flux_project::vj::Vj;
use flux_project::vj_modulation::{self, Bands, Clock, Modulator, Values};
use tauri::{AppHandle, Manager, State};

use crate::analysis::SharedAnalysis;
use crate::commands::SharedSession;
use crate::vj_commands::SharedGrid;

/// How often the grid's sound is rebuilt while a layer's volume or master is modulated.
const SOUND_EVERY: Duration = Duration::from_millis(33);

/// The smoothing of the audio sources, shared by every screen and the sound.
pub struct Modulation {
    modulator: Mutex<Modulator>,
    analysis: SharedAnalysis,
}

pub type SharedModulation = Arc<Modulation>;

impl Modulation {
    pub fn new(analysis: SharedAnalysis) -> Self {
        Self {
            modulator: Mutex::default(),
            analysis,
        }
    }

    /// `vj` with its modulated values at `clock`; the grid itself, borrowed, when it has none.
    pub fn apply<'a>(&self, vj: &'a Vj, clock: Clock) -> Cow<'a, Vj> {
        if !vj.is_modulated() {
            return Cow::Borrowed(vj);
        }
        let bands = self.analysis.bands.load();
        let mut modulator = self
            .modulator
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        vj_modulation::modulate(vj, &mut modulator, clock, bands)
    }

    /// What to draw of `vj` at `clock`: `vj` itself when nothing is modulated, else `drawn`, a
    /// copy kept by the caller between frames (it must be reset to `None` whenever `vj`
    /// changes) and patched in place.
    pub fn draw<'a>(&self, vj: &'a Vj, drawn: &'a mut Option<Vj>, clock: Clock) -> &'a Vj {
        if !vj.is_modulated() {
            *drawn = None;
            return vj;
        }
        let bands = self.analysis.bands.load();
        let mut modulator = self
            .modulator
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let copy = drawn.get_or_insert_with(|| vj.clone());
        vj_modulation::modulate_into(vj, copy, &mut modulator, clock, bands);
        copy
    }

    /// The modulated values of `vj` at `clock`.
    pub fn values(&self, vj: &Vj, clock: Clock) -> Values {
        let bands: Bands = self.analysis.bands.load();
        let mut modulator = self
            .modulator
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        vj_modulation::values(vj, &mut modulator, clock, bands)
    }
}

/// The composition's clock now: its seconds and the beats it has counted.
pub fn clock(session: &SharedSession, grid: &SharedGrid) -> Clock {
    let bpm = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .tempo_bpm();
    let mut grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
    let seconds = grid.now().as_secs_f64();
    Clock {
        seconds,
        beats: grid.beats(bpm),
    }
}

/// The modulated values of the grid now, for the inspector to mark on its controls; empty outside
/// a VJ project or when nothing is modulated.
#[tauri::command]
pub fn vj_modulation_values(
    session: State<'_, SharedSession>,
    grid: State<'_, SharedGrid>,
    modulation: State<'_, SharedModulation>,
) -> Values {
    let Some(vj) = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .grid_copy()
    else {
        return Values::default();
    };
    if !vj.is_modulated() {
        return Values::default();
    }
    modulation.values(&vj, clock(&session, &grid))
}

/// Starts the timer that keeps the grid's sound following a modulated volume, for as long as the
/// app runs.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = thread::Builder::new()
        .name("flux-vj-modulation".into())
        .spawn(move || {
            let mut tempo = 0.0;
            loop {
                thread::sleep(SOUND_EVERY);
                let session = app.state::<SharedSession>();
                let session = session.lock().unwrap_or_else(PoisonError::into_inner);
                let modulated = session.grid_ref().is_some_and(Vj::is_sound_modulated);
                // A clip that lasts a number of beats sounds at the speed the tempo makes of it.
                let bpm = session.tempo_bpm();
                let retempo = bpm != tempo && session.grid_ref().is_some_and(Vj::has_beat_clips);
                tempo = bpm;
                drop(session);
                if modulated || retempo {
                    crate::vj_audio::refresh(&app);
                }
            }
        });
}
