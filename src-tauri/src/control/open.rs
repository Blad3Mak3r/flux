use super::*;

impl Control {
    /// What the window is waiting to map, while it is in map mode.
    pub fn vj_learn(&self) -> &SharedLearn {
        &self.vj_learn
    }

    /// Sends a key of the window to the thread that runs the mappings.
    pub fn key(&self, message: KeyMsg) {
        let keys = self.lock().running.keys.clone();
        if let Some(keys) = keys {
            // The thread is gone while the inputs are being replaced.
            let _ = keys.send(Input::Key(message));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Engine> {
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn view(&self) -> ControlView {
        let (inputs, midi_error) = match devices::list_midi_inputs() {
            Ok(ports) => (ports, None),
            Err(err) => (Vec::new(), Some(err.to_string())),
        };
        let engine = self.lock();
        ControlView {
            feedback_default: feedback_default(&engine.prefs),
            prefs: engine.prefs.clone(),
            midi: engine.midi.clone(),
            osc: engine.osc.clone(),
            artnet: engine.artnet.clone(),
            artnet_packets: engine
                .running
                .artnet
                .as_ref()
                .map_or(0, ArtnetListener::packets),
            inputs,
            midi_error,
        }
    }

    /// Makes `prefs` the ones in use: keeps them and opens the inputs they ask for. The previous
    /// inputs are closed first, so that the same UDP port can be bound again.
    pub(super) fn apply(&self, app: &AppHandle, prefs: ControlPrefs) {
        let prefs = normalize(prefs);
        match prefs_file().map(|file| save(&file, &prefs)) {
            Some(Err(err)) => log::warn!("the remote control preferences were not saved: {err}"),
            None => log::warn!("the remote control preferences were not saved: no data folder"),
            Some(Ok(())) => {}
        }
        self.learning.store(false, Ordering::Relaxed);
        let mut engine = self.lock();
        engine.running = Running::default();
        let (running, midi, osc, artnet) = open(app, &prefs, &self.learning, &self.vj_learn);
        *engine = Engine {
            prefs,
            running,
            midi,
            osc,
            artnet,
        };
    }
}

/// Opens the inputs `prefs` ask for and starts the thread that executes them. The state of each
/// is reported and logged here, on the caller's thread.
pub(super) fn open(
    app: &AppHandle,
    prefs: &ControlPrefs,
    learning: &Arc<AtomicBool>,
    vj_learn: &SharedLearn,
) -> (Running, LinkState, LinkState, LinkState) {
    let (tx, rx) = mpsc::channel();
    let mut running = Running::default();

    let midi = match (&prefs.midi.enabled, &prefs.midi.port_name) {
        (true, Some(name)) => match MidiListener::open(name, prefs.midi.port_index, tx.clone()) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to the MIDI port {}",
                    listener.name()
                );
                let state = LinkState::Listening(listener.name().to_owned());
                running.midi = Some(listener);
                state
            }
            Err(ListenError::Missing) => {
                log::warn!("remote control: the MIDI port {name} is not connected");
                LinkState::Missing
            }
            Err(err @ ListenError::Unavailable(_)) => {
                log::warn!("remote control: {err}");
                LinkState::Error(err.to_string())
            }
        },
        _ => LinkState::Off,
    };

    let osc = if prefs.osc.enabled {
        match crate::osc_api::OscServer::bind(
            app.clone(),
            &prefs.osc.bind_address,
            prefs.osc.port,
            prefs.osc.tcp_enabled,
            tx.clone(),
        ) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to OSC on UDP port {}",
                    listener.port()
                );
                let state = LinkState::Listening(listener.port().to_string());
                running.osc = Some(listener);
                state
            }
            Err(err) => {
                log::warn!(
                    "remote control: OSC port {} is not available: {err}",
                    prefs.osc.port
                );
                LinkState::Error(err.to_string())
            }
        }
    } else {
        LinkState::Off
    };

    let artnet = if prefs.artnet.enabled {
        match ArtnetListener::bind(
            &prefs.artnet.bind_address,
            ARTNET_UDP_PORT,
            prefs.artnet.universe,
            tx.clone(),
        ) {
            Ok(listener) => {
                log::info!(
                    "remote control: listening to Art-Net on {}:{}",
                    prefs.artnet.bind_address,
                    listener.port()
                );
                let state = LinkState::Listening(format!(
                    "{}:{}",
                    prefs.artnet.bind_address,
                    listener.port()
                ));
                running.artnet = Some(listener);
                state
            }
            Err(err) => {
                log::warn!("remote control: Art-Net is not available: {err}");
                LinkState::Error(err.to_string())
            }
        }
    } else {
        LinkState::Off
    };

    if prefs.midi.enabled && prefs.midi.feedback {
        running.lights = prefs.midi.port_name.as_ref().and_then(|name| {
            Lights::start(
                app.clone(),
                name.clone(),
                prefs.midi.port_index,
                prefs.midi.bindings.clone(),
            )
        });
    }

    // The inputs and `running.keys` hold the only senders, so the thread ends when they are
    // dropped. It always runs: the keyboard is an input that needs no preferences.
    running.keys = Some(tx);
    {
        let port = match &midi {
            LinkState::Listening(name) => name.clone(),
            _ => String::new(),
        };
        let dispatcher = Dispatcher {
            app: app.clone(),
            bindings: prefs.midi.bindings.clone(),
            learning: Arc::clone(learning),
            vj: VjRunner::new(app.clone(), Arc::clone(vj_learn), port),
        };
        running.worker = thread::Builder::new()
            .name("flux-control".into())
            .spawn(move || dispatcher.run(&rx))
            .inspect_err(|err| log::warn!("remote control: the thread did not start: {err}"))
            .ok();
    }
    (running, midi, osc, artnet)
}

/// The values a binding without its own lights its pad with: those of the controller's preset.
pub(super) fn feedback_default(prefs: &ControlPrefs) -> Feedback {
    prefs
        .midi
        .port_name
        .as_deref()
        .map_or(Feedback::GENERIC, Feedback::preset)
}
