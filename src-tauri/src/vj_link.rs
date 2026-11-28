//! The VJ show's place in an Ableton Link session: joined while the show asks for it, with its
//! tempo shared both ways.
//!
//! The beat phase is the grid's (see `Grid::count` in `vj_commands/`); what this thread does is
//! join and leave the session as `vj.link` says, and carry the tempo across: the operator's tempo
//! goes to the session, a peer's comes into the show as a change of the project that the window
//! is told of, like a controller's. Which way a tempo goes is decided by [`flux_link::sync_tempo`].

use std::sync::{Arc, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use flux_link::{Link, Tempo, sync_tempo};
use flux_project::VjOp;
use tauri::{AppHandle, Manager};

use crate::commands::SharedSession;
use crate::vj_commands::SharedGrid;

/// How often the session is looked at.
const POLL: Duration = Duration::from_millis(20);
/// The least time between two changes of the project made by a peer dragging the tempo.
const APPLY_EVERY: Duration = Duration::from_millis(100);
/// How long a peer's tempo must be still for the change to be one step of the undo history.
const COMMIT_AFTER: Duration = Duration::from_millis(500);

/// A session that was joined, and the tempo both sides had last.
struct Joined {
    link: Arc<Link>,
    known: f64,
    /// When the project last took a peer's tempo, and whether that change is still open.
    applied: Option<Instant>,
    open: bool,
}

/// Starts the thread that follows `vj.link` of the open project for as long as the app runs.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = thread::Builder::new()
        .name("flux-link".into())
        .spawn(move || run(&app));
}

fn run(app: &AppHandle) {
    let mut joined: Option<Joined> = None;
    loop {
        thread::sleep(POLL);
        let (wanted, bpm) = {
            let session = app.state::<SharedSession>();
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            (
                session.grid_ref().is_some_and(|vj| vj.link),
                session.tempo_bpm(),
            )
        };
        let grid = app.state::<SharedGrid>();
        match (wanted, joined.as_mut()) {
            (true, None) => {
                let link = Arc::new(Link::new(bpm));
                link.set_enabled(true);
                grid.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .set_link(Some(Arc::clone(&link)), bpm);
                log::info!("joined the Ableton Link session at {bpm} BPM");
                joined = Some(Joined {
                    link,
                    known: bpm,
                    applied: None,
                    open: false,
                });
            }
            (false, Some(_)) => {
                if let Some(left) = joined.take() {
                    grid.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .set_link(None, bpm);
                    left.link.set_enabled(false);
                    log::info!("left the Ableton Link session");
                    if left.open {
                        crate::vj_control::tell(app, true);
                    }
                }
            }
            (true, Some(session)) => follow(app, session, bpm),
            (false, None) => {}
        }
    }
}

/// Carries the tempo across to the side that did not change it, and closes the change a peer made
/// once it has been still.
fn follow(app: &AppHandle, session: &mut Joined, bpm: f64) {
    let now = Instant::now();
    match sync_tempo(session.known, session.link.tempo(), bpm) {
        Tempo::ToLink(tempo) => {
            session.link.set_tempo(tempo);
            session.known = tempo;
        }
        Tempo::ToShow(tempo) => {
            if session
                .applied
                .is_some_and(|at| now.duration_since(at) < APPLY_EVERY)
            {
                return;
            }
            let edited = app
                .state::<SharedSession>()
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .edit_vj(VjOp::SetBpm { bpm: tempo });
            match edited {
                Ok(_) => {
                    session.known = tempo;
                    session.applied = Some(now);
                    session.open = true;
                    crate::vj_control::tell(app, false);
                }
                Err(err) => log::warn!("the Link tempo {tempo} was refused: {err}"),
            }
        }
        Tempo::Same => {
            // The show has the session's tempo to the two decimals it keeps.
            if (session.link.tempo() - bpm).abs() <= flux_link::TEMPO_EPSILON {
                session.known = bpm;
            }
        }
    }
    if session.open
        && session
            .applied
            .is_some_and(|at| now.duration_since(at) >= COMMIT_AFTER)
    {
        session.open = false;
        crate::vj_control::tell(app, true);
    }
}
