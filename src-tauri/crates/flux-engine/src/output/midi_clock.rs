use super::*;
use crate::midi_clock::{ClockScheduler, Mode};

/// Sends MIDI clock to a port: the clocks fall due on the position the audio callback publishes
/// (a cue show) or on the time (a VJ show), and each is sent when its sample is reached, the last
/// stretch spun like MTC's. A port that goes away is marked lost and opened again when it comes
/// back, and the clock then starts over with a Start or Continue. Nothing here logs: a failure
/// is counted in the clock's faults.
pub(super) fn run_midi_clock<L: MidiLink>(
    mode: Mode,
    clock: Clock,
    link: L,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
) {
    let rate = clock.sample_rate();
    let mut port = MtcPort::new(link, lost, Arc::clone(&shared));
    let mut scheduler = ClockScheduler::new(rate, mode);
    let mut seen_epoch = None;
    while !stop.load(Ordering::Relaxed) {
        if port.is_lost() {
            if !port.retry(shared.now()) {
                thread::park_timeout(AUDIO_PARK);
                continue;
            }
            scheduler = ClockScheduler::new(rate, mode);
            seen_epoch = None;
        }
        let snapshot = shared.load();
        let now = shared.now();
        let (position, moved) = match mode {
            Mode::Follow => {
                let moved = seen_epoch != Some(snapshot.epoch);
                seen_epoch = Some(snapshot.epoch);
                (snapshot.estimated_position(now, clock), moved)
            }
            Mode::Free => (
                (u128::from(now) * u128::from(rate) / 1_000_000_000) as u64,
                false,
            ),
        };
        let bpm = f64::from_bits(shared.clock_bpm.load(Ordering::Relaxed));
        scheduler.step(snapshot.state, position, moved, bpm, |message| {
            let (bytes, len) = message.bytes();
            port.send(&bytes[..len], now);
        });
        let wait = match scheduler.next_due() {
            Some(due) => {
                let until_due = Duration::from_nanos(
                    (u128::from(due.saturating_sub(position)) * 1_000_000_000 / u128::from(rate))
                        as u64,
                );
                let fresh = mode == Mode::Free
                    || Duration::from_nanos(now.saturating_sub(snapshot.stamp)) < MAX_EXTRAPOLATION;
                mtc_wait(until_due, fresh)
            }
            None => MtcWait::Park(MTC_TICK),
        };
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
}

#[cfg(test)]
mod tests {
    use super::*;

    type Sent = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

    /// A port that remembers what it was sent and when.
    struct Recorder(Sent);

    impl MidiLink for Recorder {
        fn send(&mut self, message: &[u8]) -> bool {
            self.0
                .lock()
                .unwrap()
                .push((Instant::now(), message.to_vec()));
            true
        }

        fn reconnect(&mut self) -> bool {
            true
        }
    }

    /// Runs a free clock at `bpm` for `time` and returns the instants of its timing clocks.
    fn clocks(bpm: f64, time: Duration) -> Vec<Instant> {
        let shared = Arc::new(SharedClock::new());
        shared.set_clock_bpm(bpm);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop, link) = (
                Arc::clone(&shared),
                Arc::clone(&stop),
                Recorder(Arc::clone(&sent)),
            );
            thread::spawn(move || {
                let clock = Clock::new(ENGINE_RATE, FrameRate::Fps25);
                run_midi_clock(
                    crate::midi_clock::Mode::Free,
                    clock,
                    link,
                    shared,
                    stop,
                    Arc::new(AtomicBool::new(false)),
                );
            })
        };
        thread::sleep(time);
        stop.store(true, Ordering::Relaxed);
        thread.thread().unpark();
        thread.join().unwrap();
        let sent = sent.lock().unwrap();
        sent.iter()
            .filter(|(_, message)| message == &[0xF8])
            .map(|(at, _)| *at)
            .collect()
    }

    fn intervals(at: &[Instant]) -> Vec<f64> {
        at.windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64() * 1000.0)
            .collect()
    }

    #[test]
    fn a_free_clock_should_tick_at_the_tempo_without_drifting() {
        // 120 BPM: a clock every 20.833 ms. The mean over the run is what a drift would move.
        let at = clocks(120.0, Duration::from_millis(1200));
        assert!(at.len() > 40, "{} clocks", at.len());
        let span = at.last().unwrap().duration_since(at[0]).as_secs_f64() * 1000.0;
        let mean = span / (at.len() - 1) as f64;
        assert!((mean - 20.8333).abs() < 0.25, "mean interval {mean} ms");
    }

    /// Prints how far the clocks fall from where they should: run it with `--ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn measure_the_jitter_of_a_free_clock() {
        for bpm in [60.0, 120.0, 180.0] {
            let at = clocks(bpm, Duration::from_secs(10));
            let nominal = 60_000.0 / (bpm * 24.0);
            let mut error: Vec<f64> = intervals(&at)
                .iter()
                .map(|interval| (interval - nominal).abs() * 1000.0)
                .collect();
            error.sort_by(f64::total_cmp);
            let span = at.last().unwrap().duration_since(at[0]).as_secs_f64() * 1000.0;
            let expected = nominal * (at.len() - 1) as f64;
            println!(
                "{bpm} BPM: {} clocks, interval error us: median {:.0}, p99 {:.0}, max {:.0}; drift over the run {:.3} ms",
                at.len(),
                error[error.len() / 2],
                error[error.len() * 99 / 100],
                error[error.len() - 1],
                span - expected,
            );
        }
    }
}
