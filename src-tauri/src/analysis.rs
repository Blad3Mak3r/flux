//! The audio analysis the VJ grid's modulation listens to: a thread reads the source chosen in the
//! preferences (the grid's own sound, an audio input, or nothing) and leaves the low, mid and high
//! bands and the level in atomics for the video frame step (see `modulation.rs`) and the
//! composition bar's meter. The logic is in [`flux_engine::analysis`]; this opens and closes the
//! sources, and retries an input that failed.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use flux_engine::analysis::{self, Analyzer, InputTap, Ring, SharedBands};
use flux_project::vj_modulation::Bands;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::preferences::{self, AnalysisSource};

/// How long to wait before trying again to open an input that failed.
const RETRY: Duration = Duration::from_secs(2);
/// How often the preferences are looked at for a new source.
const LOOK: Duration = Duration::from_millis(250);
/// With no sound arriving for this long, the analysis is fed silence so the bands fall away.
const IDLE: Duration = Duration::from_millis(40);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Off,
    Grid,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    /// Nothing is listened to.
    Off,
    /// The source is open and listening.
    Listening,
    /// The chosen input is not connected.
    Missing,
    /// The input could not be read or stopped delivering.
    Error,
}

const KINDS: [SourceKind; 3] = [SourceKind::Off, SourceKind::Grid, SourceKind::Input];
const STATES: [SourceState; 4] = [
    SourceState::Off,
    SourceState::Listening,
    SourceState::Missing,
    SourceState::Error,
];

/// What the analysis thread leaves for others to read.
#[derive(Default)]
pub struct Analysis {
    pub bands: Arc<SharedBands>,
    kind: AtomicU8,
    state: AtomicU8,
}

pub type SharedAnalysis = Arc<Analysis>;

impl Analysis {
    fn publish(&self, kind: SourceKind, state: SourceState) {
        fn at<T: PartialEq>(list: &[T], wanted: T) -> u8 {
            list.iter().position(|v| *v == wanted).unwrap_or(0) as u8
        }
        self.kind.store(at(&KINDS, kind), Ordering::Relaxed);
        self.state.store(at(&STATES, state), Ordering::Relaxed);
    }

    fn kind(&self) -> SourceKind {
        KINDS[usize::from(self.kind.load(Ordering::Relaxed)).min(KINDS.len() - 1)]
    }

    fn state(&self) -> SourceState {
        STATES[usize::from(self.state.load(Ordering::Relaxed)).min(STATES.len() - 1)]
    }
}

/// What is open for the source chosen.
struct Listening {
    choice: AnalysisSource,
    ring: Option<Arc<Ring>>,
    input: Option<InputTap>,
    state: SourceState,
    /// When a failed input may be tried again.
    retry_at: Option<Instant>,
}

fn open(choice: AnalysisSource) -> Listening {
    match &choice {
        AnalysisSource::Off => {
            analysis::set_grid_tap(false);
            Listening {
                choice,
                ring: None,
                input: None,
                state: SourceState::Off,
                retry_at: None,
            }
        }
        AnalysisSource::Grid => {
            let ring = analysis::grid_tap();
            ring.clear();
            analysis::set_grid_tap(true);
            Listening {
                choice,
                ring: Some(ring),
                input: None,
                state: SourceState::Listening,
                retry_at: None,
            }
        }
        AnalysisSource::Input {
            driver,
            device,
            channel,
        } => {
            analysis::set_grid_tap(false);
            let ring = Arc::new(Ring::new(1 << 16));
            let (input, state, retry_at) =
                match InputTap::open(driver.clone(), device.clone(), *channel, Arc::clone(&ring)) {
                    Ok(tap) => (Some(tap), SourceState::Listening, None),
                    Err(_) => (None, SourceState::Missing, Some(Instant::now() + RETRY)),
                };
            Listening {
                choice,
                ring: Some(ring),
                input,
                state,
                retry_at,
            }
        }
    }
}

fn kind_of(choice: &AnalysisSource) -> SourceKind {
    match choice {
        AnalysisSource::Off => SourceKind::Off,
        AnalysisSource::Grid => SourceKind::Grid,
        AnalysisSource::Input { .. } => SourceKind::Input,
    }
}

fn run(app: &AppHandle, shared: &Analysis) {
    let mut listening: Option<Listening> = None;
    let mut analyzer = Analyzer::new(48_000);
    let mut block = vec![0.0f32; 4096];
    let silence = vec![0.0f32; 4096];
    let (mut looked, mut fed) = (Instant::now() - LOOK, Instant::now());
    loop {
        thread::sleep(Duration::from_millis(5));
        if looked.elapsed() >= LOOK {
            looked = Instant::now();
            let choice = preferences::analysis_source(app);
            let retry = listening
                .as_ref()
                .is_some_and(|l| l.retry_at.is_some_and(|at| Instant::now() >= at));
            if retry || listening.as_ref().is_none_or(|l| l.choice != choice) {
                // The old input is dropped first, so the device is free to open again.
                drop(listening.take());
                listening = Some(open(choice));
            }
            if let Some(l) = &mut listening
                && l.input.as_ref().is_some_and(InputTap::failed)
            {
                l.input = None;
                l.state = SourceState::Error;
                l.retry_at = Some(Instant::now() + RETRY);
            }
            if let Some(l) = &listening {
                shared.publish(kind_of(&l.choice), l.state);
            }
        }
        let Some(ring) = listening.as_ref().and_then(|l| l.ring.as_ref()) else {
            shared.bands.store(Bands::default());
            // Nothing to read: look again for a new source, no faster.
            thread::sleep(LOOK);
            continue;
        };
        if ring.rate() != analyzer.rate() {
            analyzer = Analyzer::new(ring.rate());
        }
        let count = ring.pop(&mut block);
        if count > 0 {
            analyzer.push(&block[..count]);
            fed = Instant::now();
        } else if fed.elapsed() >= IDLE {
            // Nothing is playing: the time that passed was silent.
            let samples = (fed.elapsed().as_secs_f32() * analyzer.rate() as f32) as usize;
            analyzer.push(&silence[..samples.min(silence.len())]);
            fed = Instant::now();
        }
        shared.bands.store(analyzer.bands());
    }
}

/// Starts the thread that listens, for as long as the app runs.
pub fn start(app: &AppHandle) {
    let shared = Arc::clone(&app.state::<SharedAnalysis>());
    let app = app.clone();
    let _ = thread::Builder::new()
        .name("flux-analysis".into())
        .spawn(move || run(&app, &shared));
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Levels {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    pub level: f32,
    pub source: SourceKind,
    pub state: SourceState,
}

/// How loud the bands of the sound are now, 0 to 1, and what is listened to, for the meter of the
/// composition bar.
#[tauri::command]
pub fn vj_analysis_levels(analysis: State<'_, SharedAnalysis>) -> Levels {
    let bands = analysis.bands.load();
    Levels {
        low: bands.low,
        mid: bands.mid,
        high: bands.high,
        level: bands.level,
        source: analysis.kind(),
        state: analysis.state(),
    }
}
