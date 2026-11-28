//! The Ableton Link session of the VJ beat clock.
//!
//! Link shares a tempo and a beat phase between the applications on a network. Flux joins as a
//! peer: its tempo is the show's tempo both ways, and the beats its grid counts keep the phase of
//! the Link session in bars of [`QUANTUM`] beats. The Link engine is native code (see
//! `rusty_link`); what is decided here, the mapping of phases and the direction of a tempo change,
//! is pure and tested without a network.

use std::sync::Mutex;

use rusty_link::{AblLink, SessionState};

/// Beats in the phase cycle every peer agrees on: one bar of four.
pub const QUANTUM: f64 = 4.0;

/// A tempo that differs from another by less than this (beats a minute) is the same: the show
/// keeps its tempo to two decimals and Link's may be any real.
pub const TEMPO_EPSILON: f64 = 0.006;

/// A Link session this app can join and leave.
pub struct Link {
    link: AblLink,
    /// Capturing the application thread's session state is not thread-safe, so one is kept and
    /// every use of it is serialised.
    state: Mutex<SessionState>,
}

impl Link {
    /// A session that starts at `bpm` if no peer is there to give it theirs. Not joined yet.
    pub fn new(bpm: f64) -> Self {
        Self {
            link: AblLink::new(bpm),
            state: Mutex::new(SessionState::new()),
        }
    }

    /// Joins the session (and finds peers) or leaves it.
    pub fn set_enabled(&self, on: bool) {
        if self.link.is_enabled() != on {
            self.link.enable(on);
        }
    }

    pub fn enabled(&self) -> bool {
        self.link.is_enabled()
    }

    /// How many other applications are in the session.
    pub fn peers(&self) -> u64 {
        self.link.num_peers()
    }

    fn with_state<T>(&self, use_state: impl FnOnce(&mut SessionState, i64) -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        self.link.capture_app_session_state(&mut state);
        use_state(&mut state, self.link.clock_micros())
    }

    /// The session's tempo, in beats a minute.
    pub fn tempo(&self) -> f64 {
        self.with_state(|state, _| state.tempo())
    }

    /// Sets the session's tempo, which every peer follows.
    pub fn set_tempo(&self, bpm: f64) {
        self.with_state(|state, now| {
            state.set_tempo(bpm, now);
            self.link.commit_app_session_state(state);
        });
    }

    /// The beat of the session's timeline now. Its phase in [`QUANTUM`] beats is shared by every
    /// peer: the same instant has the same phase everywhere.
    pub fn beat(&self) -> f64 {
        self.with_state(|state, now| state.beat_at_time(now, QUANTUM))
    }
}

/// `beats`, moved by the least that gives it the phase of `target` in a cycle of `quantum` beats:
/// the grid's own count with the bars it has counted, on Link's phase. It never moves a count by
/// more than half a cycle, so a count that is already near stays where it is.
pub fn align(beats: f64, target: f64, quantum: f64) -> f64 {
    let diff = (target - beats).rem_euclid(quantum);
    if diff > quantum / 2.0 {
        beats + diff - quantum
    } else {
        beats + diff
    }
}

/// Which side a tempo has to be copied to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tempo {
    /// Nothing to copy.
    Same,
    /// A peer changed the session's tempo: the show takes it.
    ToShow(f64),
    /// The operator changed the show's tempo: the session takes it.
    ToLink(f64),
}

/// Decides which way a tempo moves, given the one both sides had last (`known`), the session's and
/// the show's now. A peer's change wins when both moved in the same instant.
pub fn sync_tempo(known: f64, link: f64, show: f64) -> Tempo {
    let moved = |tempo: f64| (tempo - known).abs() > TEMPO_EPSILON;
    if moved(link) {
        if (link - show).abs() > TEMPO_EPSILON {
            Tempo::ToShow(link)
        } else {
            Tempo::Same
        }
    } else if moved(show) {
        Tempo::ToLink(show)
    } else {
        Tempo::Same
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align_should_take_the_phase_of_the_target_with_the_bars_it_has() {
        // Beat 9.0 is phase 1 of its bar; the session is at phase 2.5 of another bar.
        assert_eq!(align(9.0, 102.5, 4.0), 10.5);
        // A count just ahead of the session's phase goes back to it, not on to the next bar.
        assert!((align(10.6, 6.5, 4.0) - 10.5).abs() < 1e-12);
        assert_eq!(align(8.0, 8.0, 4.0), 8.0);
    }

    #[test]
    fn align_should_never_move_a_count_by_more_than_half_a_bar() {
        for beats in [0.0, 0.3, 1.9, 2.0, 3.99, 7.7, -1.2] {
            for target in [0.0, 0.5, 1.0, 2.5, 3.9, -3.3] {
                let aligned = align(beats, target, 4.0);
                assert!(
                    (aligned - beats).abs() <= 2.0 + 1e-12,
                    "{beats} -> {target}"
                );
                let phase = (aligned - target).rem_euclid(4.0);
                assert!(phase < 1e-9 || (4.0 - phase) < 1e-9);
            }
        }
    }

    #[test]
    fn a_peer_changing_the_tempo_should_move_the_show() {
        assert_eq!(sync_tempo(120.0, 128.0, 120.0), Tempo::ToShow(128.0));
    }

    #[test]
    fn the_operator_changing_the_tempo_should_move_the_session() {
        assert_eq!(sync_tempo(120.0, 120.0, 90.0), Tempo::ToLink(90.0));
    }

    #[test]
    fn tempos_within_the_rounding_of_the_show_should_not_move_anything() {
        assert_eq!(sync_tempo(120.0, 120.004, 120.0), Tempo::Same);
        assert_eq!(sync_tempo(128.456, 128.456, 128.46), Tempo::Same);
        // The show already took the peer's tempo.
        assert_eq!(sync_tempo(120.0, 128.0, 128.0), Tempo::Same);
    }

    #[test]
    fn a_peer_should_win_when_both_move_at_once() {
        assert_eq!(sync_tempo(120.0, 128.0, 90.0), Tempo::ToShow(128.0));
    }

    #[test]
    fn a_session_without_peers_should_run_at_the_tempo_it_was_given() {
        let link = Link::new(97.0);
        assert!((link.tempo() - 97.0).abs() < 1e-9);
        link.set_tempo(101.5);
        assert!((link.tempo() - 101.5).abs() < 1e-6);
        assert!(!link.enabled());
        assert_eq!(link.peers(), 0);
    }
}
