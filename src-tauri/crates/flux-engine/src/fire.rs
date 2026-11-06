//! Firing the events of the cue tracks (MIDI, OSC, and what comes after) at their frame.
//!
//! A [`Schedule`] is a list of things to send, each with the sample it falls due at. A
//! [`Scheduler`] walks it as the transport moves, and [`run_fire`] is the thread that does so for
//! one destination, reading the position the audio callback publishes (like the MTC thread), so the
//! events are not quantised to the audio buffer.
//!
//! Rules, the same for every kind of event:
//!
//! - An event fires once, in order, when the position reaches its sample.
//! - **No chase.** A start, stop or seek ([`Scheduler::resync`]) re-arms the schedule at the new
//!   position: events before it are never fired, one exactly at it is, and seeking back makes the
//!   events already fired fire again when reached. Pausing and resuming does not resync.
//! - An event that is due while the thread runs late still fires, late: for show control a late
//!   cue beats a lost one.
//! - Whatever a sink is holding (a MIDI note that is on) is released at every resync, when the
//!   transport stops or pauses, and when the output closes.
//!
//! Nothing here logs: a failed send is counted for the log lane to report.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use flux_core::Clock;

use super::output::{MTC_TICK, MtcWait, SharedClock, mtc_wait};
use super::transport::TransportState;

/// Something to send at `at` samples from the timeline origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned<T> {
    pub at: u64,
    pub payload: T,
}

/// Everything one destination is sent, in the order it falls due.
#[derive(Debug, Clone, PartialEq)]
pub struct Schedule<T> {
    items: Vec<Planned<T>>,
}

impl<T> Schedule<T> {
    /// Stable, so things planned for the same sample keep their order.
    pub fn new(mut items: Vec<Planned<T>>) -> Self {
        items.sort_by_key(|item| item.at);
        Self { items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Planned<T>] {
        &self.items
    }
}

impl<T> Default for Schedule<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

/// Walks a [`Schedule`] as the transport moves.
#[derive(Debug)]
pub struct Scheduler<T> {
    schedule: Arc<Schedule<T>>,
    /// The first item not yet fired.
    next: usize,
    /// Everything before this sample has been fired or skipped.
    cursor: u64,
}

impl<T> Scheduler<T> {
    pub fn new(schedule: Arc<Schedule<T>>) -> Self {
        Self {
            schedule,
            next: 0,
            cursor: 0,
        }
    }

    /// Re-arms at `position` (samples), after a start, stop or seek: no chase.
    pub fn resync(&mut self, position: u64) {
        self.cursor = position;
        self.next = self
            .schedule
            .items
            .partition_point(|item| item.at < position);
    }

    /// Takes a new schedule (the project was edited) and carries on from where this one was.
    pub fn replace(&mut self, schedule: Arc<Schedule<T>>) {
        self.schedule = schedule;
        self.next = self
            .schedule
            .items
            .partition_point(|item| item.at < self.cursor);
    }

    /// Calls `send` for every item that has fallen due at `position`, in order.
    pub fn poll(&mut self, position: u64, mut send: impl FnMut(&T)) {
        while let Some(item) = self.schedule.items.get(self.next)
            && item.at <= position
        {
            send(&item.payload);
            self.next += 1;
        }
        self.cursor = self.cursor.max(position.saturating_add(1));
    }

    /// The sample the next item falls due at, so a caller can wait for it instead of polling.
    pub fn next_due(&self) -> Option<u64> {
        self.schedule.items.get(self.next).map(|item| item.at)
    }
}

/// A schedule that can be swapped while the thread that fires it runs, like the cue list.
#[derive(Debug)]
pub struct SharedSchedule<T> {
    current: Mutex<Arc<Schedule<T>>>,
    version: AtomicU64,
}

impl<T> SharedSchedule<T> {
    pub fn new(schedule: Schedule<T>) -> Self {
        Self {
            current: Mutex::new(Arc::new(schedule)),
            version: AtomicU64::new(0),
        }
    }

    pub fn replace(&self, schedule: Schedule<T>) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(schedule);
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn load(&self) -> Arc<Schedule<T>> {
        Arc::clone(&self.current.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Where a schedule's payloads go: a MIDI port, an OSC socket.
pub trait Sink<T> {
    /// Sends one payload. `false` means it could not be sent.
    fn send(&mut self, payload: &T) -> bool;

    /// Lets go of whatever the sink is holding, e.g. sends a Note Off for every note that is on.
    fn release(&mut self) {}
}

/// Fires `schedule` into `sink` as the transport that `shared` publishes moves, until `stop`.
pub(crate) fn run_fire<T, S: Sink<T>>(
    schedule: &SharedSchedule<T>,
    clock: Clock,
    mut sink: S,
    shared: &SharedClock,
    stop: &AtomicBool,
) {
    let mut version = schedule.version();
    let mut scheduler = Scheduler::new(schedule.load());
    let mut seen_epoch = None;
    let mut was_playing = false;
    while !stop.load(Ordering::Relaxed) {
        let snapshot = shared.load();
        let now = shared.now();
        let position = snapshot.estimated_position(now, clock);
        if seen_epoch != Some(snapshot.epoch) {
            seen_epoch = Some(snapshot.epoch);
            sink.release();
            scheduler.resync(position);
        }
        if schedule.version() != version {
            version = schedule.version();
            scheduler.replace(schedule.load());
        }
        let playing = snapshot.state == TransportState::Playing;
        if was_playing && !playing {
            sink.release();
        }
        was_playing = playing;

        let mut wait = MtcWait::Park(MTC_TICK);
        if playing {
            scheduler.poll(position, |payload| {
                if !sink.send(payload) {
                    shared.cue_failed();
                }
            });
            if let Some(due) = scheduler.next_due() {
                let samples = due.saturating_sub(position);
                let until_due = Duration::from_nanos(
                    (u128::from(samples) * 1_000_000_000 / u128::from(clock.sample_rate())) as u64,
                );
                let fresh = Duration::from_nanos(now.saturating_sub(snapshot.stamp))
                    < super::output::MAX_EXTRAPOLATION;
                wait = mtc_wait(until_due, fresh);
            }
        }
        match wait {
            MtcWait::Park(time) => thread::park_timeout(time),
            MtcWait::Spin(time) => {
                let deadline = shared.now() + time.as_nanos() as u64;
                while shared.now() < deadline && !stop.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
            }
        }
    }
    sink.release();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler(times: &[u64]) -> Scheduler<u64> {
        let items = times
            .iter()
            .map(|&at| Planned { at, payload: at })
            .collect();
        Scheduler::new(Arc::new(Schedule::new(items)))
    }

    fn poll(scheduler: &mut Scheduler<u64>, position: u64) -> Vec<u64> {
        let mut fired = Vec::new();
        scheduler.poll(position, |&payload| fired.push(payload));
        fired
    }

    #[test]
    fn poll_should_fire_each_event_once_when_the_position_reaches_it() {
        let mut s = scheduler(&[100, 200]);
        s.resync(0);
        assert!(poll(&mut s, 99).is_empty());
        assert_eq!(poll(&mut s, 100), [100]);
        assert!(poll(&mut s, 150).is_empty());
        assert_eq!(poll(&mut s, 200), [200]);
        assert!(poll(&mut s, 5_000).is_empty());
    }

    #[test]
    fn poll_should_fire_everything_due_in_order() {
        let mut s = scheduler(&[300, 100, 200]);
        s.resync(0);
        assert_eq!(poll(&mut s, 250), [100, 200]);
        assert_eq!(poll(&mut s, 250), Vec::<u64>::new());
    }

    #[test]
    fn events_at_the_same_sample_should_keep_their_order() {
        let items = vec![
            Planned {
                at: 10,
                payload: "a",
            },
            Planned {
                at: 10,
                payload: "b",
            },
        ];
        let mut s = Scheduler::new(Arc::new(Schedule::new(items)));
        s.resync(0);
        let mut fired = Vec::new();
        s.poll(10, |&payload| fired.push(payload));
        assert_eq!(fired, ["a", "b"]);
    }

    #[test]
    fn an_event_at_the_start_should_fire_on_play() {
        let mut s = scheduler(&[0, 50]);
        s.resync(0);
        assert_eq!(poll(&mut s, 0), [0]);
    }

    #[test]
    fn resync_after_a_seek_past_events_should_not_fire_them() {
        let mut s = scheduler(&[100, 200, 300]);
        s.resync(0);
        s.resync(250);
        assert_eq!(poll(&mut s, 260), Vec::<u64>::new());
        assert_eq!(poll(&mut s, 300), [300]);
    }

    #[test]
    fn resync_onto_an_event_should_fire_it() {
        let mut s = scheduler(&[100, 200]);
        s.resync(200);
        assert_eq!(poll(&mut s, 200), [200]);
    }

    #[test]
    fn seeking_back_should_fire_the_events_again() {
        let mut s = scheduler(&[100, 200]);
        s.resync(0);
        assert_eq!(poll(&mut s, 250), [100, 200]);
        s.resync(50);
        assert_eq!(poll(&mut s, 250), [100, 200]);
    }

    #[test]
    fn pausing_without_a_resync_should_not_fire_anything_twice() {
        let mut s = scheduler(&[100]);
        s.resync(0);
        assert_eq!(poll(&mut s, 100), [100]);
        // Paused and resumed: the caller does not resync, it only polls again.
        assert!(poll(&mut s, 100).is_empty());
        assert!(poll(&mut s, 120).is_empty());
    }

    #[test]
    fn next_due_should_follow_the_events_as_they_fire() {
        let mut s = scheduler(&[100, 200]);
        s.resync(0);
        assert_eq!(s.next_due(), Some(100));
        poll(&mut s, 100);
        assert_eq!(s.next_due(), Some(200));
        poll(&mut s, 200);
        assert_eq!(s.next_due(), None);
    }

    #[test]
    fn replace_should_carry_on_after_what_already_fired() {
        let mut s = scheduler(&[100, 200]);
        s.resync(0);
        poll(&mut s, 150);
        // An edit adds an event before the playhead and one after it.
        let items = [50, 100, 175, 200]
            .iter()
            .map(|&at| Planned { at, payload: at })
            .collect();
        s.replace(Arc::new(Schedule::new(items)));
        assert_eq!(poll(&mut s, 300), [175, 200]);
    }

    #[test]
    fn replace_just_after_a_resync_should_keep_an_event_at_the_position() {
        let mut s = scheduler(&[100]);
        s.resync(100);
        s.replace(Arc::new(Schedule::new(vec![Planned {
            at: 100,
            payload: 1,
        }])));
        assert_eq!(poll(&mut s, 100), [1]);
    }

    #[test]
    fn a_shared_schedule_should_change_version_when_replaced() {
        let shared = SharedSchedule::new(Schedule::<u64>::default());
        let before = shared.version();
        shared.replace(Schedule::new(vec![Planned { at: 1, payload: 1 }]));
        assert_ne!(shared.version(), before);
        assert_eq!(shared.load().len(), 1);
    }
}
