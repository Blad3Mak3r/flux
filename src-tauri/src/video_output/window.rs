use super::*;

impl VideoOutput {
    pub(super) fn close(&self) {
        let live = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut live) = live {
            live.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = live.thread.take() {
                let _ = thread.join();
            }
            let _ = live.window.destroy();
        }
    }

    /// Opens the window on the chosen display, or closes it when there is none or it is not
    /// connected. When it is already open there it only takes the screen's new name.
    pub(super) fn apply(
        self: &Arc<Self>,
        app: &AppHandle,
        chosen: Option<&str>,
        name: &str,
        index: usize,
    ) {
        {
            let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
            let wish = *self
                .fullscreen
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(live) = live.as_mut()
                && chosen == Some(live.display.as_str())
                && wish.is_none_or(|full| full == live.fullscreen)
            {
                if live.name != name {
                    let _ = live.window.set_title(&title(name));
                    live.name = name.to_owned();
                }
                return;
            }
        }
        self.close();
        let Some(chosen) = chosen else {
            *self.error.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return;
        };
        let Some((display, monitor)) = displays(app).into_iter().find(|(d, _)| d.id == chosen)
        else {
            log::warn!("video output {name}: display {chosen} is not connected");
            return;
        };
        let result = self.open(app, &display, &monitor, name, index);
        if let Err(err) = &result {
            log::warn!("video output {name} could not open on {chosen}: {err}");
        } else {
            log::info!("video output {name} open on {chosen}");
        }
        *self.error.lock().unwrap_or_else(PoisonError::into_inner) = result.err();
    }

    fn open(
        self: &Arc<Self>,
        app: &AppHandle,
        display: &Display,
        monitor: &Monitor,
        name: &str,
        index: usize,
    ) -> Result<(), String> {
        let scale = monitor.scale_factor();
        let at = monitor.position();
        let (x, y) = (f64::from(at.x) / scale, f64::from(at.y) / scale);
        // On the editor's own display it is a window that can be moved aside, so a single screen
        // is not covered; on any other it fills the display.
        let label = format!("{LABEL_PREFIX}{}", self.screen);
        let mut builder = Window::builder(app, label)
            .title(title(name))
            .visible(true)
            .focused(false);
        if let Some(icon) = app.default_window_icon().cloned() {
            builder = builder.icon(icon).map_err(|err| err.to_string())?;
        }
        let wish = *self
            .fullscreen
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let fullscreen = wish.unwrap_or(!display.main);
        let window = if !fullscreen {
            // Each screen a little further in, so windows on one display do not hide each other.
            let step = 80.0 + 40.0 * index as f64;
            builder
                .inner_size(960.0, 540.0)
                .position(x + step, y + step)
                .build()
        } else {
            builder
                .decorations(false)
                .position(x, y)
                .inner_size(
                    f64::from(display.width) / scale,
                    f64::from(display.height) / scale,
                )
                .fullscreen(true)
                .build()
        }
        .map_err(|err| err.to_string())?;

        let size = Arc::new(AtomicU64::new(pack(
            window.inner_size().unwrap_or_default(),
        )));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (size, stop, output) = (Arc::clone(&size), Arc::clone(&stop), Arc::clone(self));
            window.on_window_event(move |event| match event {
                WindowEvent::Resized(new) => size.store(pack(*new), Ordering::Relaxed),
                WindowEvent::Destroyed => {
                    stop.store(true, Ordering::Relaxed);
                    // Closed by the user: it stays closed until the display is chosen again.
                    if let Ok(mut live) = output.live.try_lock()
                        && live
                            .as_ref()
                            .is_some_and(|l| l.stop.load(Ordering::Relaxed))
                    {
                        *live = None;
                    }
                }
                _ => {}
            });
        }

        // The GPU surface is made on the main thread, as macOS requires; the device is started on
        // the render thread, which answers once it can draw.
        let (target_tx, target_rx) = std::sync::mpsc::channel();
        let target = window.clone();
        app.run_on_main_thread(move || {
            let _ = target_tx.send(Screen::target(target));
        })
        .map_err(|err| err.to_string())?;
        let target = match target_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(target)) => target,
            Ok(Err(err)) => {
                let _ = window.destroy();
                return Err(err.to_string());
            }
            Err(_) => {
                let _ = window.destroy();
                return Err("the window did not answer".into());
            }
        };

        let session = Arc::clone(&app.state::<SharedSession>());
        let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
        ensure_deck(&self.deck, app)?;
        let deck = Arc::clone(&self.deck);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (width, height) = unpack(size.load(Ordering::Relaxed));
        let transport = Arc::clone(&app.state::<SharedOutput>());
        let grid = Arc::clone(&app.state::<crate::vj_commands::SharedGrid>());
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let render_stop = Arc::clone(&stop);
        let pattern = Arc::clone(&self.pattern);
        let controls = Arc::clone(&self.controls);
        let screen_id = self.screen.clone();
        let render = move |screen| Render {
            screen,
            screen_id,
            deck,
            session,
            cache,
            transport,
            size,
            stop: render_stop,
            pattern,
            controls,
            screen_name: String::new(),
            enabled: true,
            timeline: None,
            quads: Vec::new(),
            masks: Vec::new(),
            seen: (u64::MAX, String::new()),
            reader: None,
            cards: None,
            card_size: CARD_SIZE,
            card_slices: Vec::new(),
            card_caption: String::new(),
            card_frames: Vec::new(),
            card_ids: Vec::new(),
            card_seq: 0,
            decoding: None,
            grid,
            modulation,
            vj: None,
            drawn: None,
            last_frame: None,
        };
        let thread = thread::Builder::new()
            .name("flux-video-output".into())
            .spawn(move || match Screen::new(target, width, height) {
                Ok(screen) => {
                    let _ = ready_tx.send(Ok(screen.errors()));
                    render(screen).run();
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err.to_string()));
                }
            })
            .map_err(|err| err.to_string())?;
        let started = ready_rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| Err("the graphics device did not answer".into()));
        let gpu_errors = match started {
            Ok(errors) => errors,
            Err(err) => {
                stop.store(true, Ordering::Relaxed);
                let _ = window.destroy();
                return Err(err);
            }
        };
        *self.live.lock().unwrap_or_else(PoisonError::into_inner) = Some(Live {
            window,
            display: display.id.clone(),
            name: name.to_owned(),
            stop,
            thread: Some(thread),
            gpu_errors,
            fullscreen,
        });
        Ok(())
    }

    /// Whether the window is open, and the last error it met.
    pub(super) fn state(&self) -> (bool, Option<String>) {
        let (open, gpu_error) = self
            .live
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|live| !live.stop.load(Ordering::Relaxed))
            .map_or((false, None), |live| {
                let error = live
                    .gpu_errors
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                (true, error)
            });
        let error = self
            .error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .or(gpu_error);
        (open, error)
    }
}
