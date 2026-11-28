use super::*;

/// What the Output menu shows of the output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputState {
    /// Every screen is black.
    pub disabled: bool,
    /// Every screen shows the test card.
    pub test_card: bool,
    /// How many displays there are to put the output full screen on.
    pub displays: u32,
    /// The display (from 1) the main screen covers; absent while it is a window.
    pub fullscreen: Option<u32>,
}

/// What the Output menu switches.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Control {
    Disable,
    TestCard,
}

/// The snapshot that was saved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub path: String,
    pub width: u32,
    pub height: u32,
}

/// The display `n` counts from 1, in the order the system lists them, as Resolume's
/// Output > Fullscreen > Display N.
pub(super) fn display_at(displays: &[Display], n: u32) -> Option<&Display> {
    displays.get(usize::try_from(n.checked_sub(1)?).ok()?)
}

impl VideoOutputs {
    fn output_state(&self, app: &AppHandle) -> OutputState {
        let displays: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
        let main = self
            .outputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(flux_project::mapping::MAIN_SCREEN)
            .cloned();
        let on = main.and_then(|output| {
            let live = output.live.lock().unwrap_or_else(PoisonError::into_inner);
            live.as_ref()
                .filter(|live| live.fullscreen && !live.stop.load(Ordering::Relaxed))
                .map(|live| live.display.clone())
        });
        OutputState {
            disabled: self.controls.disabled.load(Ordering::Relaxed),
            test_card: self.controls.card.load(Ordering::Relaxed),
            displays: displays.len() as u32,
            fullscreen: on
                .and_then(|id| displays.iter().position(|d| d.id == id))
                .map(|at| at as u32 + 1),
        }
    }

    /// Puts the main screen full screen on display `n` (from 1), or back in a window on the
    /// display it is on.
    fn set_fullscreen(&self, app: &AppHandle, display: Option<u32>) -> Result<(), String> {
        let main = self
            .outputs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(flux_project::mapping::MAIN_SCREEN)
            .cloned()
            .ok_or("the project has no output screen")?;
        let wish = match display {
            Some(n) => {
                let all: Vec<Display> = displays(app).into_iter().map(|(d, _)| d).collect();
                let id = display_at(&all, n)
                    .ok_or_else(|| format!("there is no display {n}"))?
                    .id
                    .clone();
                preferences::preferences_change(
                    app.clone(),
                    app.state::<preferences::Store>(),
                    Change::VideoDisplay {
                        screen: flux_project::mapping::MAIN_SCREEN.into(),
                        display: Some(id),
                    },
                )?;
                true
            }
            None => false,
        };
        *main
            .fullscreen
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(wish);
        self.sync(app);
        let (open, error) = main.state();
        match (open, error) {
            (_, Some(error)) => Err(error),
            (false, None) => Err("the output is not on a display".into()),
            _ => Ok(()),
        }
    }

    /// Saves the composition's picture as a PNG in `dir`.
    fn snapshot(&self, app: &AppHandle, dir: &std::path::Path) -> Result<Snapshot, String> {
        let picture = self
            .draw_composition(app, None, None)?
            .ok_or("a snapshot needs a VJ show")?;
        let size = |at: usize| u32::from(u16::from_le_bytes([picture[at], picture[at + 1]]));
        let (width, height) = (size(0), size(2));
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
        let path = crate::snapshot::save(dir, &stamp, &picture[4..], width, height)?;
        Ok(Snapshot {
            path: path.to_string_lossy().into_owned(),
            width,
            height,
        })
    }
}

/// Sends whether the output is disabled or showing the test card, and where it is full screen,
/// down `on_state` whenever it changes (a display can be plugged in or out), until
/// [`crate::watch::watch_stop`].
#[tauri::command]
pub fn video_output_watch(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    watches: State<'_, crate::watch::Watches>,
    on_state: tauri::ipc::Channel<OutputState>,
) {
    let output = Arc::clone(&output);
    let mut changes = crate::watch::Changes::default();
    watches.start(on_state, Duration::from_millis(1500), move || {
        changes.next(output.output_state(&app))
    });
}

/// Flips Disable Output or Show Test Card, and answers the state.
#[tauri::command]
pub fn video_output_toggle(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    control: Control,
) -> OutputState {
    let flag = match control {
        Control::Disable => &output.controls.disabled,
        Control::TestCard => &output.controls.card,
    };
    let on = !flag.fetch_xor(true, Ordering::Relaxed);
    log::info!(
        "video output: {control:?} {}",
        if on { "on" } else { "off" }
    );
    output.output_state(&app)
}

/// Puts the output full screen on display `display` (from 1), or in a window when there is none.
#[tauri::command]
pub async fn video_output_fullscreen(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    display: Option<u32>,
) -> Result<OutputState, String> {
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        output.set_fullscreen(&worker_app, display)?;
        Ok(output.output_state(&worker_app))
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Saves a PNG of the composition in the snapshot folder of the preferences.
#[tauri::command]
pub async fn video_output_snapshot(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
) -> Result<Snapshot, String> {
    let output = Arc::clone(&output);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let chosen = preferences::preferences_get(
            worker_app.clone(),
            worker_app.state::<preferences::Store>(),
        )?
        .snapshot_dir;
        let data = worker_app
            .path()
            .app_data_dir()
            .map_err(|err| err.to_string())?;
        let dir = crate::snapshot::folder(chosen.as_deref(), dirs::picture_dir(), &data);
        let saved = output.snapshot(&worker_app, &dir);
        match &saved {
            Ok(shot) => log::info!("video output: snapshot saved to {}", shot.path),
            Err(err) => log::warn!("video output: snapshot failed: {err}"),
        }
        saved
    })
    .await
    .map_err(|err| err.to_string())?
}

/// What screen `screen` outputs, small, as the picture of [`vj_output_frame`]: the composition cut
/// by its visible slices and masks at the shape of its display (the composition's when it has
/// none), black while the screen or the whole output is off. Empty for an unknown screen or a
/// project that is not VJ.
#[tauri::command]
pub async fn vj_screen_frame(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    screen: String,
) -> Result<tauri::ipc::Response, String> {
    let output = Arc::clone(&output);
    let bytes = tauri::async_runtime::spawn_blocking(move || output.screen_frame(&app, &screen))
        .await
        .map_err(|err| err.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

impl VideoOutputs {
    fn screen_frame(&self, app: &AppHandle, id: &str) -> Result<Vec<u8>, String> {
        let session = Arc::clone(&app.state::<SharedSession>());
        let Some(found) = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens()
            .into_iter()
            .find(|s| s.id == id)
        else {
            return Ok(Vec::new());
        };
        let cached = self
            .monitor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .display_sizes
            .get(id)
            .filter(|(at, _)| at.elapsed() < DISPLAY_SIZE_KEPT)
            .map(|(_, size)| *size);
        let chosen = cached.unwrap_or_else(|| {
            let chosen =
                preferences::preferences_get(app.clone(), app.state::<preferences::Store>())
                    .ok()
                    .and_then(|p| p.video_display_for(id))
                    .and_then(|id| displays(app).into_iter().find(|(d, _)| d.id == id))
                    .map(|(d, _)| (d.width.max(1), d.height.max(1)));
            self.monitor
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .display_sizes
                .insert(id.to_owned(), (Instant::now(), chosen));
            chosen
        });
        let (ow, oh) = chosen.unwrap_or_else(|| composition_size(&session));
        let off = !found.enabled || self.controls.disabled.load(Ordering::Relaxed);
        let width = MONITOR_WIDTH;
        let height = (u64::from(width) * u64::from(oh) / u64::from(ow)).max(1) as u32;
        let rgba = if off {
            [0, 0, 0, 255].repeat((width * height) as usize)
        } else {
            let Some(frame) = self.composition(app)? else {
                return Ok(Vec::new());
            };
            let size = composition_size(&session);
            let (cw, ch) = (
                usize::from(u16::from_le_bytes([frame[0], frame[1]])),
                usize::from(u16::from_le_bytes([frame[2], frame[3]])),
            );
            let quads: Vec<Quad> = if found.slices.is_empty() {
                vec![flux_render::fitted_quad(size, (ow, oh))]
            } else {
                found
                    .slices
                    .iter()
                    .filter(|slice| !slice.hidden)
                    .map(quad_of)
                    .collect()
            };
            flux_render::map_screen(
                &frame[4..],
                (cw, ch),
                &quads,
                &masks_of(&found.masks),
                (width as usize, height as usize),
            )
        };
        let mut out = Vec::with_capacity(rgba.len() + 4);
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.extend_from_slice(&rgba);
        Ok(out)
    }
}
