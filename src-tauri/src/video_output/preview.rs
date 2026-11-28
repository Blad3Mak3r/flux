use super::*;

/// The width of the composition the editor's monitor shows; its height keeps the shape.
pub(super) const MONITOR_WIDTH: u32 = 480;

/// A deck that opens the videos of the open project.
fn new_deck(app: &AppHandle) -> Result<VideoDeck, String> {
    let cache = crate::video_commands::video_cache(app).map_err(|err| err.to_string())?;
    let session = Arc::clone(&app.state::<SharedSession>());
    Ok(VideoDeck::new(move |asset| {
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_file(asset, &cache)
            .ok()
    }))
}

/// Makes the deck every screen draws from, when no window has yet.
pub(crate) fn ensure_deck(deck: &SharedDeck, app: &AppHandle) -> Result<(), String> {
    let mut deck = deck.lock().unwrap_or_else(PoisonError::into_inner);
    if deck.is_none() {
        *deck = Some(new_deck(app)?);
    }
    Ok(())
}

/// The monitor's picture of `shown` through the composition's effects: its width and height as
/// two little-endian u16, then its RGBA pixels (no encoding to wait for on either side). The
/// output and the preview monitors both come from here, so a clip looks the same in each.
#[allow(clippy::too_many_arguments)]
fn monitor_picture(
    shown: &[flux_video::Shown],
    looks: &[flux_video::GroupLook],
    vj: &flux_project::vj::Vj,
    size: (u32, u32),
    width: u32,
    clock: flux_project::vj_modulation::Clock,
    last: &mut Option<Instant>,
    history: &mut flux_render::History,
    with_post: bool,
) -> Vec<u8> {
    let height = (u64::from(width) * u64::from(size.1) / u64::from(size.0.max(1))).max(1) as u32;
    let mut picture = Vec::with_capacity(4 + (width * height * 4) as usize);
    picture.extend_from_slice(&(width as u16).to_le_bytes());
    picture.extend_from_slice(&(height as u16).to_le_bytes());
    let at = Instant::now();
    let dt = last
        .replace(at)
        .map_or(1.0 / 60.0, |last| at.duration_since(last).as_secs_f32());
    let post = if with_post {
        composition_post(vj, clock, dt)
    } else {
        flux_render::Post::default()
    };
    let time = flux_render::Time {
        seconds: clock.seconds,
        beats: clock.beats,
    };
    picture.extend(flux_render::composite_items(
        &items(shown, looks, size, time),
        width,
        height,
        &post,
        history,
    ));
    picture
}

/// The grid cut down to `clip`'s layer, with that layer at full strength and its clip alone: what
/// the preview plays, so the clip is drawn as it is placed and not as the layer is mixed.
pub(super) fn preview_grid(vj: &flux_project::vj::Vj, clip: &str) -> Option<flux_project::vj::Vj> {
    let (layer, _) = vj.find_clip(clip)?;
    let mut layer = layer.clone();
    layer.clips.retain(|c| c.id == clip);
    layer.master = 1.0;
    layer.opacity = 1.0;
    layer.blend = flux_project::vj::Blend::Alpha;
    layer.transition = 0.0;
    layer.crossfade = None;
    layer.bypass = false;
    layer.solo = false;
    layer.group = None;
    layer.modulation.clear();
    let mut vj = vj.clone();
    vj.layers = vec![layer];
    Some(vj)
}

/// How long the preview goes on without being asked for a picture before it lets go of its decoders.
const PREVIEW_IDLE: Duration = Duration::from_secs(1);

/// What the editor's preview monitor plays: the selected clip alone, on a clock and decoders of its
/// own, so previewing never changes what is on air.
#[derive(Default)]
pub(super) struct Preview {
    clip: Option<String>,
    /// Its own clock and playhead (the grid's type, with only the clip launched).
    grid: crate::vj_commands::Grid,
    deck: Option<VideoDeck>,
    /// The session's revision and the clip the cut-down grid was made for, and the grid.
    cut: Option<(u64, String, String, flux_project::vj::Vj)>,
    drawn: Option<flux_project::vj::Vj>,
    history: flux_render::History,
    last: Option<Instant>,
    polled: Option<Instant>,
    /// A thread is watching for the preview to go unasked.
    watching: bool,
}

impl Preview {
    /// Lets go of the clip and its decoders.
    fn stop(&mut self) {
        *self = Self {
            watching: self.watching,
            ..Self::default()
        };
    }
}

/// Stops `preview` once nobody has asked for a picture for [`PREVIEW_IDLE`]; ends with it.
pub(super) fn watch(preview: Arc<Mutex<Preview>>) {
    let _ = thread::Builder::new()
        .name("flux-vj-preview".into())
        .spawn(move || {
            loop {
                thread::sleep(PREVIEW_IDLE / 2);
                let mut preview = preview.lock().unwrap_or_else(PoisonError::into_inner);
                if preview.polled.is_none_or(|at| at.elapsed() >= PREVIEW_IDLE) {
                    preview.stop();
                    preview.watching = false;
                    return;
                }
            }
        });
}

impl VideoOutputs {
    /// What a VJ project's grid sends to the screens now, before their mapping, drawn small on
    /// the CPU: its width and height as two little-endian u16, then its RGBA pixels. `None` for a
    /// project that is not VJ. It decodes the playing clips itself,
    /// so it shows them with no screen open too.
    pub fn composition(&self, app: &AppHandle) -> Result<Option<Vec<u8>>, String> {
        self.draw_composition(app, Some(MONITOR_WIDTH), None)
    }

    /// What `scope` alone contributes to the composition, as the picture of [`Self::composition`]
    /// but without the composition's effects and feedback: a layer with its own effects, opacity
    /// and blend, or the layers of a group mixed as the group is. Empty when the layer or group
    /// is unknown. It reads the pictures the shared deck decodes for the grid, so it opens nothing.
    pub fn scoped(&self, app: &AppHandle, scope: &Scope) -> Result<Vec<u8>, String> {
        Ok(self
            .draw_composition(app, Some(MONITOR_WIDTH), Some(scope))?
            .unwrap_or_default())
    }

    /// The composition as [`Self::composition`] draws it, `width` pixels across (its own width
    /// when `None`, from a history of its own so the monitor's is left alone).
    pub(super) fn draw_composition(
        &self,
        app: &AppHandle,
        width: Option<u32>,
        scope: Option<&Scope>,
    ) -> Result<Option<Vec<u8>>, String> {
        let session = Arc::clone(&app.state::<SharedSession>());
        // The grid is copied again only when the session moved.
        let mut guard = self.monitor.lock().unwrap_or_else(PoisonError::into_inner);
        let frames = &mut *guard;
        {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let (revision, active) = session.revision();
            if frames
                .grid
                .as_ref()
                .is_none_or(|(r, a, _)| (*r, a.as_str()) != (revision, active))
            {
                frames.grid = Some((revision, active.to_owned(), session.grid_copy()));
                frames.drawn = None;
                frames.pictures.clear();
            }
        }
        let revision = frames.grid.as_ref().map_or(0, |(r, _, _)| *r);
        let key = scope_key(scope);
        if width == Some(MONITOR_WIDTH)
            && let Some((at, rev, picture)) = frames.pictures.get(&key)
            && picture_is_fresh(at.elapsed(), *rev, revision)
        {
            return Ok(Some(picture.clone()));
        }
        let Some(vj) = frames.grid.as_ref().and_then(|(_, _, vj)| vj.as_ref()) else {
            return Ok(None);
        };
        ensure_deck(&self.deck, app)?;
        let bpm = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active_timeline_copy()
            .map_or(120.0, |timeline| timeline.tempo.bpm);
        let (play, now, clock) = tick_grid(&app.state::<crate::vj_commands::SharedGrid>(), vj, bpm);
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let drawn = modulation.draw(vj, &mut frames.drawn, clock);
        let size = composition_size(&session);
        let mut shown = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .map(|deck| deck.show_grid(drawn, &play, now, size))
            .unwrap_or_default();
        let mut looks = looks_of(&play, drawn, now);
        match scope {
            Some(Scope::Layer(id)) => {
                shown.retain(|s| &s.layer.track == id);
                shown.iter_mut().for_each(|s| s.layer.group = None);
                looks.clear();
            }
            Some(Scope::Group(id)) => {
                shown.retain(|s| s.layer.group.as_ref() == Some(id));
                looks.retain(|look| &look.id == id);
            }
            None => {}
        }
        let mut fresh = flux_render::History::default();
        let mut scoped_last = None;
        let (history, last) = if width.is_some() && scope.is_none() {
            (&mut frames.history, &mut frames.last)
        } else {
            (&mut fresh, &mut scoped_last)
        };
        let picture = monitor_picture(
            &shown,
            &looks,
            drawn,
            size,
            width.unwrap_or(size.0),
            clock,
            last,
            history,
            scope.is_none(),
        );
        if width == Some(MONITOR_WIDTH) {
            frames
                .pictures
                .insert(key, (Instant::now(), revision, picture.clone()));
        }
        Ok(Some(picture))
    }

    /// What `clip` alone looks like in the composition (see [`Self::composition`]), as that
    /// picture: its fit, scale, position and effects, at full strength on black. The clip plays on
    /// a clock of its own from its start, whatever is on air. `None` stops the preview, and so
    /// does going unasked for a second. Empty when there is no such clip.
    pub fn preview(&self, app: &AppHandle, clip: Option<&str>) -> Result<Vec<u8>, String> {
        let mut guard = self.preview.lock().unwrap_or_else(PoisonError::into_inner);
        let preview = &mut *guard;
        let Some(clip) = clip else {
            preview.stop();
            return Ok(Vec::new());
        };
        let session = Arc::clone(&app.state::<SharedSession>());
        let bpm = {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let (revision, active) = session.revision();
            let current = preview.cut.as_ref().is_some_and(|(r, a, c, _)| {
                (*r, a.as_str(), c.as_str()) == (revision, active, clip)
            });
            if !current {
                preview.cut = session
                    .grid_ref()
                    .and_then(|vj| preview_grid(vj, clip))
                    .map(|vj| (revision, active.to_owned(), clip.to_owned(), vj));
                preview.drawn = None;
            }
            session.tempo_bpm()
        };
        let Some((_, _, _, vj)) = preview.cut.as_ref() else {
            preview.stop();
            return Ok(Vec::new());
        };
        preview.polled = Some(Instant::now());
        if !preview.watching {
            preview.watching = true;
            watch(Arc::clone(&self.preview));
        }
        let tick = preview.grid.tick(vj, bpm);
        if preview.clip.as_deref() != Some(clip) {
            preview.clip = Some(clip.to_owned());
            // A new deck, so the last clip's decoders close and a file that failed to open is tried again.
            preview.deck = None;
            preview.history = flux_render::History::default();
            preview.grid.play = flux_video::GridPlay::default();
            preview.grid.play.launch(vj, clip, tick);
        }
        if preview.deck.is_none() {
            preview.deck = Some(new_deck(app)?);
        }
        let clock = flux_project::vj_modulation::Clock {
            seconds: tick.time.as_secs_f64(),
            beats: tick.beats,
        };
        let modulation = Arc::clone(&app.state::<crate::modulation::SharedModulation>());
        let drawn = modulation.draw(vj, &mut preview.drawn, clock);
        let size = composition_size(&session);
        let shown = preview
            .deck
            .as_mut()
            .map(|deck| deck.show_grid(drawn, &preview.grid.play, tick, size))
            .unwrap_or_default();
        Ok(monitor_picture(
            &shown,
            &looks_of(&preview.grid.play, drawn, tick),
            drawn,
            size,
            MONITOR_WIDTH,
            clock,
            &mut preview.last,
            &mut preview.history,
            true,
        ))
    }
}
