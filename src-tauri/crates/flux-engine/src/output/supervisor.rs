use super::*;

/// How long a stream may go without calling back before its device counts as gone. Hosts that
/// never report an unplugged device just stop calling.
pub(super) const STREAM_STALL: Duration = Duration::from_millis(1_000);
/// How long a stream just opened has to call back for the first time.
pub(super) const STREAM_GRACE: Duration = Duration::from_millis(3_000);
/// How often a lost stream or MIDI port is tried again.
pub(super) const RETRY_INTERVAL: Duration = Duration::from_millis(1_000);

/// Notices a stream that stopped calling back. Fed the callback count and the time.
#[derive(Debug)]
pub(super) struct Watchdog {
    beats: u64,
    /// When the count last moved (or the stream opened).
    since: Duration,
    started: bool,
}

impl Watchdog {
    pub(super) fn new(now: Duration) -> Self {
        Self {
            beats: 0,
            since: now,
            started: false,
        }
    }

    /// Whether the stream has been silent for too long as of `now`.
    pub(super) fn stalled(&mut self, beats: u64, now: Duration) -> bool {
        if beats != self.beats {
            self.beats = beats;
            self.since = now;
            self.started = true;
        }
        let allowed = if self.started {
            STREAM_STALL
        } else {
            STREAM_GRACE
        };
        now.saturating_sub(self.since) > allowed
    }
}

/// What the thread that holds a stream should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Idle,
    /// The device is gone: raise the lost flag, and close the stream when it will be retried.
    Lose,
    /// Try to open the stream again.
    Reopen,
}

/// Decides when a stream is lost and, for a follower, when to try to get it back.
#[derive(Debug)]
pub(super) struct Supervisor {
    watchdog: Watchdog,
    retry: bool,
    state: StreamState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamState {
    Open,
    /// Closed and waiting until this time to try again.
    Down(Duration),
    /// Lost, and not to be reopened.
    Gone,
}

impl Supervisor {
    pub(super) fn new(now: Duration, retry: bool) -> Self {
        Self {
            watchdog: Watchdog::new(now),
            retry,
            state: StreamState::Open,
        }
    }

    pub(super) fn step(&mut self, now: Duration, beats: u64, lost: bool) -> Step {
        match self.state {
            StreamState::Open if lost || self.watchdog.stalled(beats, now) => {
                self.state = if self.retry {
                    StreamState::Down(now + RETRY_INTERVAL)
                } else {
                    StreamState::Gone
                };
                Step::Lose
            }
            StreamState::Down(at) if now >= at => Step::Reopen,
            _ => Step::Idle,
        }
    }

    /// The stream opened again.
    pub(super) fn reopened(&mut self, now: Duration) {
        self.watchdog = Watchdog::new(now);
        self.state = StreamState::Open;
    }

    /// Opening it again failed.
    pub(super) fn reopen_failed(&mut self, now: Duration) {
        self.state = StreamState::Down(now + RETRY_INTERVAL);
    }
}

/// What a stream thread watches on the stream it holds.
pub(super) struct Watch {
    /// Bumped by every callback.
    pub(super) beat: Arc<AtomicU64>,
    /// Raised when the stream is lost; lowered when a retry opens it again.
    pub(super) lost: Arc<AtomicBool>,
    /// Whether to open the stream again once it is lost.
    pub(super) retry: bool,
}
