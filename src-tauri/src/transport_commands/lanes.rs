use super::*;

/// The device lane: says once when the programme's device is back after it was lost.
#[derive(Default)]
pub(super) struct DeviceLane {
    last_check: Option<Instant>,
    announced: bool,
}

impl DeviceLane {
    /// Whether to tell the window now. `present` looks for the device, which is not cheap, so it
    /// only runs while the device is lost, at most once an interval.
    pub(super) fn back(
        &mut self,
        lost: bool,
        now: Instant,
        present: impl FnOnce() -> bool,
    ) -> bool {
        if !lost || self.announced {
            return false;
        }
        let due = self
            .last_check
            .is_none_or(|last| now.duration_since(last) >= DEVICE_CHECK_INTERVAL);
        if !due {
            return false;
        }
        self.last_check = Some(now);
        self.announced = present();
        self.announced
    }
}

/// The position lane: a position that did not move is not sent again.
#[derive(Default)]
pub(super) struct PositionLane {
    last: Option<Position>,
}

impl PositionLane {
    pub(super) fn next(&mut self, position: Position) -> Option<Position> {
        (self.last.replace(position) != Some(position)).then_some(position)
    }
}

/// The live lane: a state of the show that did not change is not sent again.
#[derive(Default)]
pub(super) struct LiveLane {
    last: Option<LiveState>,
}

impl LiveLane {
    pub(super) fn next(&mut self, state: LiveState) -> Option<LiveState> {
        if self.last.as_ref() == Some(&state) {
            return None;
        }
        self.last = Some(state.clone());
        Some(state)
    }
}

/// The end lane: tells when the show ran to its end, from the count the output keeps.
pub(super) struct EndLane {
    seen: u64,
}

impl EndLane {
    pub(super) fn new(seen: u64) -> Self {
        Self { seen }
    }

    /// Whether the show ended since the last call.
    pub(super) fn ended(&mut self, count: u64) -> bool {
        std::mem::replace(&mut self.seen, count) != count
    }
}

/// What the end lane does when the show ends: the next timeline becomes the active one, stopped at
/// its start, and the window is told.
#[derive(Clone)]
pub(super) struct Advance {
    pub(super) app: AppHandle,
    pub(super) session: SharedSession,
}

impl Advance {
    pub(super) fn run(&self) {
        let next = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next_timeline();
        match next {
            Ok(Some(state)) => {
                log::info!("show ended, the next timeline is the active one");
                let _ = self.app.emit(TIMELINE_ADVANCED, state);
            }
            Ok(None) => log::info!("show ended"),
            Err(err) => log::warn!("show ended, the next timeline could not be selected: {err}"),
        }
    }
}

/// The levels lane: silence is sent once, so the meters know to fall, and then not again.
pub(super) struct LevelsLane {
    /// The previous levels were all silence, which was already sent.
    silent: bool,
}

impl Default for LevelsLane {
    fn default() -> Self {
        Self { silent: true }
    }
}

impl LevelsLane {
    pub(super) fn next(&mut self, peaks: Vec<TrackPeak>) -> Option<Vec<TrackPeak>> {
        let quiet = peaks.iter().all(|track| track.peak == 0.0);
        let due = !(quiet && self.silent);
        self.silent = quiet;
        due.then_some(peaks)
    }
}

#[derive(Default)]
pub(super) struct AudioRoutingLane {
    last: Option<AudioRoutingState>,
}

impl AudioRoutingLane {
    pub(super) fn next(&mut self, state: AudioRoutingState) -> Option<AudioRoutingState> {
        if self.last.as_ref() == Some(&state) {
            return None;
        }
        self.last = Some(state.clone());
        Some(state)
    }
}
