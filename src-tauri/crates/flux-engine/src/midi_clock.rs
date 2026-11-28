//! MIDI clock out: 24 timing clocks per quarter note, Start, Continue and Stop with the transport,
//! and Song Position Pointer when the transport is moved. Flux only sends it; it never follows one.
//!
//! The schedule is pure, so it is tested without a port: [`ClockScheduler`] is told the transport's
//! state and a position in samples and says which messages are due. The thread that sends them is
//! in `output/midi_clock.rs`.
//!
//! The ticks are placed on the position, not on a timer of their own: a tick falls due when the
//! position passes the sample it belongs to, and the next one is a tick's length after that one,
//! not after the time the previous was sent, so nothing accumulates.

use flux_project::{MAX_BPM, MIN_BPM};

use crate::transport::TransportState;

/// Clocks in a quarter note.
pub const TICKS_PER_BEAT: f64 = 24.0;
/// Clocks in one step of the Song Position Pointer, a MIDI beat: a sixteenth note.
pub const TICKS_PER_POSITION: u64 = 6;
/// A scheduler that is behind by more than this many seconds (it was not looked at) starts over
/// from the position instead of sending the clocks it missed in a burst.
const MAX_LAG_SECONDS: f64 = 0.25;

/// A message of the MIDI clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// A timing clock (0xF8).
    Clock,
    /// Start from the beginning (0xFA).
    Start,
    /// Go on from where the position pointer is (0xFB).
    Continue,
    /// Stop (0xFC).
    Stop,
    /// Song Position Pointer (0xF2): the position in MIDI beats, up to 16383.
    Position(u16),
}

impl Message {
    /// The bytes as they go to the port.
    pub fn bytes(self) -> ([u8; 3], usize) {
        match self {
            Self::Clock => ([0xF8, 0, 0], 1),
            Self::Start => ([0xFA, 0, 0], 1),
            Self::Continue => ([0xFB, 0, 0], 1),
            Self::Stop => ([0xFC, 0, 0], 1),
            Self::Position(beats) => {
                let beats = beats.min(0x3FFF);
                ([0xF2, (beats & 0x7F) as u8, (beats >> 7) as u8], 3)
            }
        }
    }
}

/// How the position the scheduler is given moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// It is the transport's own position (a cue show): the clock counts from the start of the
    /// timeline, runs only while the transport plays and says where it is when it is moved.
    Follow,
    /// It is the time (a VJ show): the clock runs all the time and Start, Continue and Stop only
    /// follow the transport's state.
    Free,
}

/// Samples between two clocks at `bpm` beats a minute, which is brought into the range a show can
/// have.
pub fn samples_per_tick(rate: u32, bpm: f64) -> f64 {
    let bpm = if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        120.0
    };
    f64::from(rate) * 60.0 / (bpm * TICKS_PER_BEAT)
}

/// Which clocks and transport messages are due.
#[derive(Debug, Clone)]
pub struct ClockScheduler {
    rate: u32,
    mode: Mode,
    /// The position of the next clock, in samples.
    next: f64,
    /// Clocks are going out.
    ticking: bool,
    last: TransportState,
}

impl ClockScheduler {
    pub fn new(rate: u32, mode: Mode) -> Self {
        Self {
            rate,
            mode,
            next: 0.0,
            ticking: false,
            last: TransportState::Stopped,
        }
    }

    /// Looks at the transport and sends, through `emit`, what is due at `position` samples:
    /// the Start, Continue, Stop or Song Position the state or a move asks for, and then every
    /// clock whose sample the position has passed. `moved` is true when the transport was moved
    /// since the last call.
    pub fn step(
        &mut self,
        state: TransportState,
        position: u64,
        moved: bool,
        bpm: f64,
        mut emit: impl FnMut(Message),
    ) {
        let per_tick = samples_per_tick(self.rate, bpm);
        let was = self.last;
        self.last = state;
        match (self.mode, state) {
            (Mode::Follow, TransportState::Playing) => {
                if was != TransportState::Playing || moved {
                    // The first clock is the first sixteenth the position has not passed, and the
                    // pointer says which one it is.
                    let beats = (position as f64 / per_tick / TICKS_PER_POSITION as f64).ceil();
                    self.next = beats * TICKS_PER_POSITION as f64 * per_tick;
                    self.ticking = true;
                    let at = beats as u16;
                    if was == TransportState::Playing {
                        emit(Message::Position(at));
                    } else if at == 0 {
                        emit(Message::Start);
                    } else {
                        emit(Message::Position(at));
                        emit(Message::Continue);
                    }
                }
            }
            (Mode::Follow, _) => {
                if was == TransportState::Playing {
                    emit(Message::Stop);
                }
                self.ticking = false;
            }
            (Mode::Free, _) => {
                if !self.ticking {
                    self.next = position as f64;
                    self.ticking = true;
                }
                match (was, state) {
                    (TransportState::Stopped, TransportState::Playing) => emit(Message::Start),
                    (TransportState::Paused, TransportState::Playing) => emit(Message::Continue),
                    (TransportState::Playing, TransportState::Stopped | TransportState::Paused) => {
                        emit(Message::Stop);
                    }
                    _ => {}
                }
            }
        }
        if !self.ticking {
            return;
        }
        if position as f64 - self.next > f64::from(self.rate) * MAX_LAG_SECONDS {
            self.next = position as f64;
        }
        while self.next <= position as f64 {
            emit(Message::Clock);
            self.next += per_tick;
        }
    }

    /// The position the next clock falls due at, in samples; `None` while none goes out.
    pub fn next_due(&self) -> Option<u64> {
        self.ticking.then(|| self.next.ceil() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn run(
        scheduler: &mut ClockScheduler,
        state: TransportState,
        position: u64,
        moved: bool,
    ) -> Vec<Message> {
        let mut out = Vec::new();
        scheduler.step(state, position, moved, 120.0, |message| out.push(message));
        out
    }

    #[test]
    fn a_tick_should_last_a_twenty_fourth_of_a_beat() {
        // 120 beats a minute: a beat is half a second, 24000 samples.
        assert_eq!(samples_per_tick(RATE, 120.0), 1000.0);
        assert_eq!(samples_per_tick(RATE, f64::NAN), 1000.0);
        assert_eq!(samples_per_tick(RATE, 1.0), samples_per_tick(RATE, 20.0));
    }

    #[test]
    fn the_bytes_should_be_those_of_the_midi_specification() {
        assert_eq!(Message::Clock.bytes(), ([0xF8, 0, 0], 1));
        assert_eq!(Message::Start.bytes().0[0], 0xFA);
        assert_eq!(Message::Continue.bytes().0[0], 0xFB);
        assert_eq!(Message::Stop.bytes().0[0], 0xFC);
        // 300 MIDI beats: the low seven bits first.
        assert_eq!(Message::Position(300).bytes(), ([0xF2, 44, 2], 3));
        assert_eq!(Message::Position(u16::MAX).bytes(), ([0xF2, 127, 127], 3));
    }

    #[test]
    fn playing_from_the_start_should_send_start_and_the_first_clock() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        assert!(run(&mut scheduler, TransportState::Stopped, 0, false).is_empty());
        assert_eq!(
            run(&mut scheduler, TransportState::Playing, 0, true),
            [Message::Start, Message::Clock]
        );
        assert_eq!(scheduler.next_due(), Some(1000));
    }

    #[test]
    fn twenty_four_clocks_should_go_out_in_a_beat() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        run(&mut scheduler, TransportState::Playing, 0, true);
        let mut clocks = 0;
        for position in (100..=24_000).step_by(100) {
            clocks += run(&mut scheduler, TransportState::Playing, position, false).len();
        }
        // The one at 0 went out with the start; those at 1000 ... 24000 are 24 more.
        assert_eq!(clocks, 24);
    }

    #[test]
    fn playing_from_the_middle_should_point_at_the_next_sixteenth_and_continue() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        // A sixteenth is 6000 samples at 120: 10000 is between the second and the third.
        let sent = run(&mut scheduler, TransportState::Playing, 10_000, true);
        assert_eq!(sent, [Message::Position(2), Message::Continue]);
        assert_eq!(scheduler.next_due(), Some(12_000));
    }

    #[test]
    fn a_move_while_playing_should_only_send_the_position() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        run(&mut scheduler, TransportState::Playing, 0, true);
        assert_eq!(
            run(&mut scheduler, TransportState::Playing, 60_001, true),
            [Message::Position(11)]
        );
        assert_eq!(scheduler.next_due(), Some(66_000));
    }

    #[test]
    fn pausing_and_stopping_should_send_stop_once_and_no_clocks() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        run(&mut scheduler, TransportState::Playing, 0, true);
        assert_eq!(
            run(&mut scheduler, TransportState::Paused, 5_000, false),
            [Message::Stop]
        );
        assert!(run(&mut scheduler, TransportState::Stopped, 0, true).is_empty());
        assert_eq!(scheduler.next_due(), None);
    }

    #[test]
    fn a_stalled_scheduler_should_start_over_instead_of_sending_a_burst() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        run(&mut scheduler, TransportState::Playing, 0, true);
        let sent = run(&mut scheduler, TransportState::Playing, 480_000, false);
        assert_eq!(sent, [Message::Clock]);
    }

    #[test]
    fn a_tempo_change_should_change_the_distance_to_the_next_clock_only() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Follow);
        run(&mut scheduler, TransportState::Playing, 0, true);
        let mut out = Vec::new();
        // 240: a clock every 500 samples, from the one that went out at 1000.
        scheduler.step(TransportState::Playing, 1_000, false, 240.0, |m| {
            out.push(m);
        });
        assert_eq!(out, [Message::Clock]);
        assert_eq!(scheduler.next_due(), Some(1_500));
    }

    #[test]
    fn a_free_clock_should_tick_from_the_first_look_and_follow_the_transport_in_messages() {
        let mut scheduler = ClockScheduler::new(RATE, Mode::Free);
        assert_eq!(
            run(&mut scheduler, TransportState::Stopped, 5_000, false),
            [Message::Clock]
        );
        assert_eq!(
            run(&mut scheduler, TransportState::Playing, 5_500, false),
            [Message::Start]
        );
        assert_eq!(
            run(&mut scheduler, TransportState::Paused, 6_000, false),
            [Message::Stop, Message::Clock]
        );
        assert_eq!(
            run(&mut scheduler, TransportState::Playing, 6_100, false),
            [Message::Continue]
        );
        // A move of the transport says nothing: the clock is the time.
        assert!(run(&mut scheduler, TransportState::Playing, 6_200, true).is_empty());
    }
}
