//! Sends the state of the VJ grid back over OSC to the destinations the show names (see
//! `flux_project::vj_osc_feedback`). One thread looks at the grid about thirty times a second and
//! writes only what changed, at most thirty times a second per address; it never runs in an audio
//! callback and only reads what the grid publishes (the session is only tried, so the window never
//! waits for it).

use std::collections::{HashMap, HashSet};
use std::sync::PoisonError;
use std::thread;
use std::time::{Duration, Instant};

use flux_engine::fire::Sink;
use flux_engine::osc::{OscSink, encode};
use flux_project::vj_osc_feedback::{self as feedback, Live, Message, Throttle, Value};
use flux_project::{OscArg, OscDestination};
use tauri::{AppHandle, Manager};

use crate::commands::SharedSession;
use crate::vj_commands::SharedGrid;

/// How long to wait when no destination is named.
const IDLE_EVERY: Duration = Duration::from_millis(250);

/// How long to wait before trying a destination that failed again.
const RETRY: Duration = Duration::from_secs(2);

/// A destination with its socket and what it was sent.
struct Target {
    sink: Option<OscSink>,
    sent: Throttle,
    retry_at: Instant,
}

/// What the grid shows now, read in one go: the messages of every address, or `None` while the
/// session is busy or has no VJ show.
fn look(app: &AppHandle) -> Option<(Vec<String>, Vec<Message>)> {
    let (playing, column, crossfader, paused) = {
        let grid = app.state::<SharedGrid>();
        let grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
        let playing: HashSet<String> = grid
            .play
            .playing()
            .values()
            .filter_map(|launched| launched.clip.clone())
            .collect();
        (
            playing,
            grid.play.column().map(str::to_owned),
            grid.play.crossfader(),
            grid.paused(),
        )
    };
    let session = app.state::<SharedSession>();
    let session = session.try_lock().ok()?;
    let project = session.vj_project()?;
    if project.vj.osc_feedback.is_empty() {
        return Some((Vec::new(), Vec::new()));
    }
    let live = Live {
        playing: &|id| playing.contains(id),
        column: column.as_deref(),
        crossfader,
        bpm: session.tempo_bpm(),
        paused,
    };
    Some((
        project.vj.osc_feedback.clone(),
        feedback::messages(&project.vj, &live),
    ))
}

fn args(value: Value) -> [OscArg; 1] {
    [match value {
        Value::Int(v) => OscArg::Int(v),
        Value::Float(v) => OscArg::Float(v),
    }]
}

fn connect(destination: &str) -> Result<OscSink, String> {
    let (host, port) = feedback::split_destination(destination)
        .ok_or_else(|| format!("{destination} is not host:port"))?;
    OscSink::connect(&OscDestination {
        id: destination.to_owned(),
        name: destination.to_owned(),
        host: host.to_owned(),
        port,
    })
}

/// Starts the thread, which runs for as long as the app does.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = thread::Builder::new()
        .name("flux-vj-osc-feedback".into())
        .spawn(move || {
            let started = Instant::now();
            let mut targets: HashMap<String, Target> = HashMap::new();
            loop {
                let Some((destinations, messages)) = look(&app) else {
                    thread::sleep(feedback::MIN_INTERVAL);
                    continue;
                };
                targets.retain(|name, _| destinations.contains(name));
                if destinations.is_empty() {
                    thread::sleep(IDLE_EVERY);
                    continue;
                }
                for name in &destinations {
                    let target = targets.entry(name.clone()).or_insert_with(|| Target {
                        sink: None,
                        sent: Throttle::default(),
                        retry_at: Instant::now(),
                    });
                    if target.sink.is_none() && Instant::now() >= target.retry_at {
                        match connect(name) {
                            Ok(sink) => {
                                log::info!("OSC feedback: sending to {name}");
                                target.sent.forget();
                                target.sink = Some(sink);
                            }
                            Err(err) => {
                                log::warn!("OSC feedback: cannot reach {err}");
                                target.retry_at = Instant::now() + RETRY;
                            }
                        }
                    }
                    let Some(sink) = target.sink.as_mut() else {
                        continue;
                    };
                    let due = target.sent.pick(started.elapsed(), messages.clone());
                    let failed = due
                        .iter()
                        .filter(|m| !sink.send(&encode(&m.address, &args(m.value))))
                        .count();
                    if failed > 0 {
                        log::warn!("OSC feedback: {failed} messages to {name} were not sent");
                        target.sink = None;
                        target.retry_at = Instant::now() + RETRY;
                    }
                }
                thread::sleep(feedback::MIN_INTERVAL);
            }
        })
        .inspect_err(|err| log::warn!("OSC feedback: the thread did not start: {err}"));
}
