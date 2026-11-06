//! The log lane: turns what a running output reports into log lines.
//!
//! It runs on a lane thread, never in an audio callback. The callbacks only set flags and bump
//! counters, and this compares them with what it saw last time.

use std::time::{Duration, Instant};

use log::Level;

use flux_engine::output::{Faults, Position};

/// Faults are logged at most this often, so a device that keeps failing does not flood the log.
const FAULT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct LogLane {
    position: Option<Position>,
    /// The faults already in the log.
    reported: Faults,
    last_fault_line: Option<Instant>,
}

impl LogLane {
    /// The lines for what changed since the last call.
    pub fn next(
        &mut self,
        position: Position,
        faults: Faults,
        now: Instant,
    ) -> Vec<(Level, String)> {
        let mut lines = Vec::new();
        // The first position is the state the output started in, which the start already logged.
        if let Some(before) = self.position.replace(position) {
            if before.state != position.state {
                lines.push((
                    Level::Info,
                    format!("transport {:?} at frame {}", position.state, position.frame),
                ));
            }
            for (name, was, is) in [
                (
                    "audio device lost",
                    before.device_lost,
                    position.device_lost,
                ),
                ("LTC device lost", before.ltc_lost, position.ltc_lost),
                (
                    "device renders faster than real time",
                    before.clock_unpaced,
                    position.clock_unpaced,
                ),
            ] {
                if is && !was {
                    lines.push((Level::Warn, name.into()));
                }
            }
        }

        let due = self
            .last_fault_line
            .is_none_or(|last| now.duration_since(last) >= FAULT_INTERVAL);
        if due && let Some(line) = self.fault_line(faults) {
            self.reported = faults;
            self.last_fault_line = Some(now);
            lines.push((Level::Warn, line));
        }
        lines
    }

    /// What went wrong since the last fault line, if anything.
    fn fault_line(&self, faults: Faults) -> Option<String> {
        let new = [
            (faults.xruns - self.reported.xruns, "audio dropouts"),
            (
                faults.stream_errors - self.reported.stream_errors,
                "stream errors",
            ),
            (
                faults.mtc_errors - self.reported.mtc_errors,
                "MIDI messages not sent",
            ),
        ];
        let parts: Vec<String> = new
            .into_iter()
            .filter(|&(count, _)| count > 0)
            .map(|(count, what)| format!("{count} {what}"))
            .collect();
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_engine::transport::TransportState;

    fn at(state: TransportState) -> Position {
        Position {
            state,
            frame: 100,
            position_samples: 0,
            device_lost: false,
            ltc_lost: false,
            clock_unpaced: false,
        }
    }

    #[test]
    fn next_should_log_nothing_for_the_first_position() {
        let mut lane = LogLane::default();
        let lines = lane.next(
            at(TransportState::Stopped),
            Faults::default(),
            Instant::now(),
        );
        assert!(lines.is_empty());
    }

    #[test]
    fn next_should_log_a_state_change_once() {
        let mut lane = LogLane::default();
        let now = Instant::now();
        lane.next(at(TransportState::Stopped), Faults::default(), now);

        let lines = lane.next(at(TransportState::Playing), Faults::default(), now);
        assert_eq!(
            lines,
            [(Level::Info, "transport Playing at frame 100".into())]
        );
        assert!(
            lane.next(at(TransportState::Playing), Faults::default(), now)
                .is_empty()
        );
    }

    #[test]
    fn next_should_log_a_lost_device_when_the_flag_is_raised() {
        let mut lane = LogLane::default();
        let now = Instant::now();
        lane.next(at(TransportState::Playing), Faults::default(), now);
        let lost = Position {
            device_lost: true,
            ..at(TransportState::Playing)
        };

        let lines = lane.next(lost, Faults::default(), now);
        assert_eq!(lines, [(Level::Warn, "audio device lost".into())]);
        assert!(lane.next(lost, Faults::default(), now).is_empty());
    }

    #[test]
    fn next_should_batch_faults_to_one_line_a_second() {
        let mut lane = LogLane::default();
        let start = Instant::now();
        let position = at(TransportState::Playing);
        let faults = |xruns| Faults {
            xruns,
            ..Faults::default()
        };

        let lines = lane.next(position, faults(2), start);
        assert_eq!(lines, [(Level::Warn, "2 audio dropouts".into())]);
        // More dropouts inside the second wait for the next line, and are not lost.
        assert!(
            lane.next(position, faults(5), start + Duration::from_millis(500))
                .is_empty()
        );
        let lines = lane.next(position, faults(5), start + FAULT_INTERVAL);
        assert_eq!(lines, [(Level::Warn, "3 audio dropouts".into())]);
    }
}
