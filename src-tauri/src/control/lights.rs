use super::*;

/// How often the state of the show is looked at. Only a change is sent to the controller; the
/// look itself is a few atomic reads.
const LIGHTS_INTERVAL: Duration = Duration::from_millis(50);

/// How long a lit pad that blinks stays in each half of the blink.
const BLINK_HALF: Duration = Duration::from_millis(500);

/// How long to wait before trying to open the controller's output port again.
const LIGHTS_RETRY: Duration = Duration::from_secs(2);

/// The thread that lights the pads of the controller. Dropping it stops the thread, which puts
/// the pads out first.
pub(super) struct Lights {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Lights {
    pub(super) fn start(
        app: AppHandle,
        port: String,
        index: usize,
        bindings: Vec<Binding>,
    ) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("flux-control-lights".into())
            .spawn({
                let stop = Arc::clone(&stop);
                move || run_lights(&app, &port, index, &bindings, &stop)
            })
            .inspect_err(|err| log::warn!("remote control: the lights thread did not start: {err}"))
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Lights {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A port the pads of the VJ mappings are lit through, found by the name of the input the control
/// was learned on.
struct PadPort {
    port: Option<LightPort>,
    lamps: Lamps,
    retry_at: Instant,
}

/// The pads of the VJ mappings with what they show now, and the colours of the grid. Empty with no
/// VJ project open, which puts every pad out. The playing clips are read first and the session
/// is only tried, so the window never waits for the lights.
fn vj_pads(app: &AppHandle) -> (Vec<Pad>, Option<PadLights>) {
    let (playing, paused) = {
        let grid = app.state::<SharedGrid>();
        let grid = grid.lock().unwrap_or_else(PoisonError::into_inner);
        let clips: HashSet<String> = grid
            .play
            .playing()
            .values()
            .filter_map(|launched| launched.clip.clone())
            .collect();
        (clips, grid.paused())
    };
    let session = app.state::<SharedSession>();
    let Ok(session) = session.try_lock() else {
        return (Vec::new(), None);
    };
    session.vj_project().map_or((Vec::new(), None), |project| {
        (
            vj_feedback::pads(&project.vj, &|id| playing.contains(id), paused),
            project.vj.lights,
        )
    })
}

/// Lights the pads of the VJ mappings on every port they were learned on, writing only what
/// changed. A port is opened when a pad needs it and closed, its pads put out, when none does; one
/// that fails is reopened after [`LIGHTS_RETRY`] and gets the whole state again.
fn light_vj_pads(
    app: &AppHandle,
    default_port: &str,
    ports: &mut HashMap<String, PadPort>,
    failed_sends: &mut u64,
) {
    let (pads, grid_lights) = vj_pads(app);
    let groups = by_port(&pads, default_port);
    for (hint, group) in &groups {
        let entry = ports.entry(hint.clone()).or_insert_with(|| PadPort {
            port: None,
            lamps: Lamps::default(),
            retry_at: Instant::now(),
        });
        if entry.port.is_none() && Instant::now() >= entry.retry_at {
            match LightPort::open(hint, 0) {
                Ok(opened) => {
                    log::info!(
                        "remote control: lighting the VJ pads through {}",
                        opened.name()
                    );
                    entry.lamps.forget();
                    entry.port = Some(opened);
                }
                Err(err) => {
                    log::warn!("remote control: the VJ pads cannot be lit on {hint}: {err}");
                    entry.retry_at = Instant::now() + LIGHTS_RETRY;
                }
            }
        }
        let Some(open) = entry.port.as_mut() else {
            continue;
        };
        let base = grid_lights.unwrap_or_else(|| PadLights::preset(hint));
        let failed = entry.lamps.update_pads(group, base, open);
        if failed > 0 {
            *failed_sends += failed as u64;
            log::warn!(
                "remote control: {failed} VJ lights could not be sent ({failed_sends} so far), reopening {hint}"
            );
            entry.port = None;
            entry.retry_at = Instant::now() + LIGHTS_RETRY;
        }
    }
    ports.retain(|hint, entry| {
        if groups.contains_key(hint) {
            return true;
        }
        if let Some(open) = entry.port.as_mut() {
            entry.lamps.clear(open);
        }
        false
    });
}

/// Looks at the state of the show, from the same sources as the Live view, and writes to the
/// controller the pads that changed. It never touches the clock: it only reads the output's
/// published state, and a port that fails is counted, logged here (off the output path, and not
/// more than once per failure) and reopened.
fn run_lights(app: &AppHandle, name: &str, index: usize, bindings: &[Binding], stop: &AtomicBool) {
    let preset = Feedback::preset(name);
    let mut lamps = Lamps::default();
    let mut port: Option<LightPort> = None;
    let mut retry_at = Instant::now();
    let mut failed_sends = 0_u64;
    let mut vj_ports: HashMap<String, PadPort> = HashMap::new();
    let started = Instant::now();
    // The session is only tried: if the window holds it, the last known values do.
    let mut timeline = None;
    let mut timecode = None;

    while !stop.load(Ordering::Relaxed) {
        if port.is_none() && Instant::now() >= retry_at {
            match LightPort::open(name, index) {
                Ok(opened) => {
                    log::info!(
                        "remote control: lighting the pads through {}",
                        opened.name()
                    );
                    lamps.forget();
                    port = Some(opened);
                }
                Err(err) => {
                    log::warn!("remote control: the lights cannot be sent: {err}");
                    retry_at = Instant::now() + LIGHTS_RETRY;
                }
            }
        }
        if let Some(open) = port.as_mut() {
            if let Ok(session) = app.state::<SharedSession>().try_lock() {
                timeline = session.timeline_place();
                timecode = session.active_timecode();
            }
            let snapshot = light_snapshot(&app.state::<SharedOutput>());
            let (position, cues, show) = match &snapshot {
                Some((position, cues, show)) => (Some(position), cues.cues(), *show),
                None => (None, &[][..], None),
            };
            let facts = Facts {
                transport: position.map(|position| position.state),
                position: position.map_or(0, |position| position.frame),
                cues,
                start: timecode.as_ref().map_or(0, |timecode| timecode.start),
                fps: timecode
                    .as_ref()
                    .map_or(0, |timecode| timecode.frame_rate.nominal_fps()),
                show,
                timeline,
            };
            let blink_on =
                (started.elapsed().as_millis() / BLINK_HALF.as_millis()).is_multiple_of(2);
            let failed = lamps.update(bindings, preset, &facts, blink_on, open);
            if failed > 0 {
                failed_sends += failed as u64;
                log::warn!(
                    "remote control: {failed} lights could not be sent ({failed_sends} so far), reopening the port"
                );
                port = None;
                retry_at = Instant::now() + LIGHTS_RETRY;
            }
        }
        light_vj_pads(app, name, &mut vj_ports, &mut failed_sends);
        thread::sleep(LIGHTS_INTERVAL);
    }

    if let Some(open) = port.as_mut() {
        lamps.clear(open);
    }
    for entry in vj_ports.values_mut() {
        if let Some(open) = entry.port.as_mut() {
            entry.lamps.clear(open);
        }
    }
}
