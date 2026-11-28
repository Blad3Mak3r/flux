use super::*;

impl VideoOutputs {
    /// The deck every screen draws from, which the recorder shares.
    pub(crate) fn deck(&self) -> &SharedDeck {
        &self.deck
    }

    /// Media was relinked: clips and logos that could not be opened are tried again.
    pub fn retry_media(&self) {
        retry_logos();
        if let Some(deck) = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            deck.retry_failed();
        }
    }

    /// Opens, moves, renames or closes a window for every screen of the project, as the
    /// preferences place them.
    pub fn sync(&self, app: &AppHandle) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        let _one = self.syncing.lock().unwrap_or_else(PoisonError::into_inner);
        let screens = app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let prefs =
            preferences::preferences_get(app.clone(), app.state::<preferences::Store>()).ok();
        let (gone, wanted) = {
            let mut outputs = self.outputs.lock().unwrap_or_else(PoisonError::into_inner);
            let gone: Vec<Arc<VideoOutput>> = outputs
                .extract_if(.., |id, _| !screens.iter().any(|screen| &screen.id == id))
                .map(|(_, output)| output)
                .collect();
            let wanted: Vec<_> = screens
                .iter()
                .enumerate()
                .map(|(index, screen)| {
                    let output = outputs.entry(screen.id.clone()).or_insert_with(|| {
                        Arc::new(VideoOutput {
                            screen: screen.id.clone(),
                            live: Mutex::default(),
                            error: Mutex::default(),
                            pattern: Arc::clone(&self.pattern),
                            controls: Arc::clone(&self.controls),
                            fullscreen: Mutex::default(),
                            deck: Arc::clone(&self.deck),
                        })
                    });
                    let display = prefs.as_ref().and_then(|p| p.video_display_for(&screen.id));
                    (Arc::clone(output), display, screen.name.clone(), index)
                })
                .collect();
            (gone, wanted)
        };
        for output in gone {
            output.close();
        }
        for (output, display, name, index) in wanted {
            output.apply(app, display.as_deref(), &name, index);
        }
    }

    fn status(&self, app: &AppHandle) -> VideoOutputStatus {
        let displays: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
        let prefs =
            preferences::preferences_get(app.clone(), app.state::<preferences::Store>()).ok();
        let screens = app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let outputs = self.outputs.lock().unwrap_or_else(PoisonError::into_inner);
        let screens = screens
            .into_iter()
            .map(|screen| {
                let chosen = prefs.as_ref().and_then(|p| p.video_display_for(&screen.id));
                let missing = chosen
                    .as_ref()
                    .is_some_and(|id| !displays.iter().any(|d| &d.id == id));
                let (open, error) = outputs
                    .get(&screen.id)
                    .map_or((false, None), |output| output.state());
                ScreenStatus {
                    id: screen.id,
                    name: screen.name,
                    chosen,
                    open,
                    missing,
                    error,
                }
            })
            .collect();
        let decoding = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(flux_video::VideoDeck::decoding)
            .map(decoding_name);
        VideoOutputStatus {
            displays,
            screens,
            decoding,
        }
    }

    /// Closes every window for good: the app is quitting.
    fn close_all(&self) {
        self.closing.store(true, Ordering::Relaxed);
        let outputs =
            std::mem::take(&mut *self.outputs.lock().unwrap_or_else(PoisonError::into_inner));
        for output in outputs.into_values() {
            output.close();
        }
    }
}

/// Closes the windows when the last window of the editor goes, so the app quits with it: a window
/// without a webview would otherwise keep it running with nothing to bring the editor back.
pub fn window_gone(app: &AppHandle, label: &str) {
    if !label.starts_with(LABEL_PREFIX) && app.webview_windows().is_empty() {
        app.state::<SharedVideoOutput>().close_all();
        app.exit(0);
    }
}

/// Opens the screens on the displays saved in the preferences, then keeps following the project:
/// a screen added, removed or renamed, or another project opened, opens or closes its window.
/// Called at start, off the main thread.
pub fn restore(app: &AppHandle) {
    let outputs = Arc::clone(&app.state::<SharedVideoOutput>());
    let session = Arc::clone(&app.state::<SharedSession>());
    let mut seen = Vec::new();
    while !outputs.closing.load(Ordering::Relaxed) {
        let screens: Vec<(String, String)> = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens()
            .into_iter()
            .map(|screen| (screen.id, screen.name))
            .collect();
        if screens != seen {
            seen = screens;
            outputs.sync(app);
        }
        thread::sleep(Duration::from_millis(400));
    }
}

/// The displays and, for each screen, the display chosen and whether its window is open there.
#[tauri::command]
pub fn video_output_status(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
) -> VideoOutputStatus {
    output.status(&app)
}

/// Chooses the display a screen opens on (`None` closes it), keeps the choice in the local
/// preferences and opens or closes its window.
#[tauri::command]
pub async fn video_output_choose(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    screen: String,
    display: Option<String>,
) -> Result<VideoOutputStatus, String> {
    preferences::preferences_change(
        app.clone(),
        app.state::<preferences::Store>(),
        Change::VideoDisplay { screen, display },
    )?;
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        output.sync(&worker_app);
        output.status(&worker_app)
    })
    .await
    .map_err(|err| err.to_string())
}
