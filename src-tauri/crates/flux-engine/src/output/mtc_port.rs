use super::*;

/// Connects to the MIDI output port a project's name hint refers to, the `index`-th one of that
/// name. Returns the port's position in the system's list, its name and the connection. The error
/// is a message for the user, since a missing port is not fatal.
pub(crate) fn connect_midi(
    hint: &str,
    index: usize,
    client: &str,
) -> Result<(usize, String, midir::MidiOutputConnection), String> {
    let output =
        midir::MidiOutput::new(client).map_err(|err| format!("MIDI is unavailable: {err}"))?;
    let ports: Vec<(String, midir::MidiOutputPort)> = output
        .ports()
        .into_iter()
        .filter_map(|port| Some((output.port_name(&port).ok()?, port)))
        .collect();
    let (name, port) = devices::find_by_name(hint, &ports, index, |(name, _)| name)
        .ok_or_else(|| format!("MIDI port not found: {hint}"))?;
    let name = name.clone();
    let at = ports
        .iter()
        .position(|(_, other)| other == port)
        .unwrap_or(0);
    output
        .connect(port, client)
        .map(|connection| (at, name.clone(), connection))
        .map_err(|err| format!("cannot open {name}: {err}"))
}

/// How the MTC thread waits for the next quarter frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MtcWait {
    Park(Duration),
    /// Busy-wait this long: the quarter frame is closer than the OS can promise to wake us.
    Spin(Duration),
}

/// Parks until shortly before the quarter frame is due, then spins the rest, so the message goes
/// out when it falls due and not up to a timer tick later. `fresh` is false when the published
/// position is old enough that waiting for it to reach the deadline could never end.
pub(crate) fn mtc_wait(until_due: Duration, fresh: bool) -> MtcWait {
    if !fresh {
        MtcWait::Park(MTC_TICK)
    } else if until_due <= MTC_SPIN {
        MtcWait::Spin(until_due)
    } else {
        MtcWait::Park((until_due - MTC_SPIN).min(MTC_TICK))
    }
}

/// A MIDI output port as `run_mtc` uses it.
pub(super) trait MidiLink {
    /// Sends one message. `false` means the port did not take it.
    fn send(&mut self, message: &[u8]) -> bool;
    /// Opens the port again after it was lost. `false` means it is still not there.
    fn reconnect(&mut self) -> bool;
}

/// The connection to a MIDI port and the name it was found by, to find it again after it is
/// unplugged.
pub(super) struct PortLink {
    pub(super) connection: midir::MidiOutputConnection,
    pub(super) hint: String,
    pub(super) index: usize,
}

impl MidiLink for PortLink {
    fn send(&mut self, message: &[u8]) -> bool {
        self.connection.send(message).is_ok()
    }

    fn reconnect(&mut self) -> bool {
        match connect_midi(&self.hint, self.index, MIDI_CLIENT_NAME) {
            Ok((_, _, connection)) => {
                self.connection = connection;
                true
            }
            Err(_) => false,
        }
    }
}

/// One MIDI port MTC goes to. A message the port refuses marks it lost; nothing is sent to it
/// until it is opened again, which is tried once a second.
pub(super) struct MtcPort<L> {
    pub(super) link: L,
    lost: Arc<AtomicBool>,
    pub(super) shared: Arc<SharedClock>,
    /// When a lost port is tried again, in the clock's nanoseconds.
    retry_at: u64,
}

impl<L: MidiLink> MtcPort<L> {
    pub(super) fn new(link: L, lost: Arc<AtomicBool>, shared: Arc<SharedClock>) -> Self {
        Self {
            link,
            lost,
            shared,
            retry_at: 0,
        }
    }

    pub(super) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    pub(super) fn send(&mut self, message: &[u8], now: u64) {
        if self.is_lost() {
            return;
        }
        if !self.link.send(message) {
            self.shared.mtc_failed();
            self.lost.store(true, Ordering::Relaxed);
            self.retry_at = now + RETRY_INTERVAL.as_nanos() as u64;
        }
    }

    /// Tries to open a lost port again when it is time. Returns whether it is back, in which case
    /// the next message must be a full frame.
    pub(super) fn retry(&mut self, now: u64) -> bool {
        if !self.is_lost() || now < self.retry_at {
            return false;
        }
        if self.link.reconnect() {
            self.lost.store(false, Ordering::Relaxed);
            return true;
        }
        self.retry_at = now + RETRY_INTERVAL.as_nanos() as u64;
        false
    }
}

/// Sends MTC from the position the audio callback publishes: a full frame whenever playback
/// starts or jumps, and quarter frames as they fall due. A port that goes away is marked lost and
/// opened again when it comes back.
pub(super) fn run_mtc(
    mut scheduler: MtcScheduler,
    clock: Clock,
    link: PortLink,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
) {
    let mut port = MtcPort::new(link, lost, Arc::clone(&shared));
    let mut seen_epoch = None;
    while !stop.load(Ordering::Relaxed) {
        if port.is_lost() {
            if port.retry(shared.now()) {
                seen_epoch = None;
            } else {
                thread::park_timeout(AUDIO_PARK);
                continue;
            }
        }
        let snapshot = shared.load();
        let now = shared.now();
        let dynamic = shared.show_timecode(&snapshot);
        let active_clock = dynamic.map_or(clock, |(clock, _)| clock);
        let position = snapshot.estimated_position(now, active_clock);
        if seen_epoch != Some(snapshot.epoch) {
            if let Some((clock, start)) = dynamic
                && let (Some(plan), Some(tick)) = (shared.show_plan.get(), snapshot.show)
            {
                scheduler.set_timecode(
                    clock,
                    plan.entries[tick.entry].timeline.timecode.frame_rate,
                    start,
                );
            }
            seen_epoch = Some(snapshot.epoch);
            port.send(&scheduler.resync(position), now);
        }
        let mut wait = MtcWait::Park(MTC_TICK);
        if snapshot.state == TransportState::Playing && !port.is_lost() {
            scheduler.poll(position, |message| port.send(&message, now));
            let samples = scheduler.next_due().saturating_sub(position);
            let until_due = Duration::from_nanos(
                (u128::from(samples) * 1_000_000_000 / u128::from(clock.sample_rate())) as u64,
            );
            let fresh =
                Duration::from_nanos(now.saturating_sub(snapshot.stamp)) < MAX_EXTRAPOLATION;
            wait = mtc_wait(until_due, fresh);
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
}
