use super::*;

/// How a slice is marked on the test card.
pub(super) fn card_slice(slice: &flux_project::mapping::Slice) -> CardSlice {
    let r = slice.input;
    CardSlice {
        name: slice.name.clone(),
        input: [r.x, r.y, r.width, r.height],
        card: slice.test_card.clone(),
        logo: None,
    }
}

/// The logos read so far, by the id of the project's image; `None` for one that could not be.
static LOGOS: OnceLock<Mutex<HashMap<String, Option<Logo>>>> = OnceLock::new();

/// Forgets the logos that could not be read, which may be there now.
pub(super) fn retry_logos() {
    if let Some(known) = LOGOS.get() {
        known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, logo| logo.is_some());
    }
}

/// Reads the image `id` of the project, once: FFmpeg decodes its first picture.
fn logo_for(session: &SharedSession, cache: &Path, id: &str) -> Option<Logo> {
    let known = LOGOS.get_or_init(Default::default);
    if let Some(found) = known.lock().unwrap_or_else(PoisonError::into_inner).get(id) {
        return found.clone();
    }
    let path = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .video_file(id, cache)
        .ok();
    let logo = path.and_then(|path| {
        let frame = flux_video::VideoFile::open(&path).ok()?.frame(0).ok()?;
        Some(Logo {
            id: id.to_owned(),
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.into(),
        })
    });
    known
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.to_owned(), logo.clone());
    logo
}

/// Gives each slice that shows a logo its picture.
pub(super) fn with_logos(slices: &mut [CardSlice], session: &SharedSession, cache: &Path) {
    for slice in slices {
        if slice.card.show_logo && !slice.card.logo.is_empty() {
            slice.logo = logo_for(session, cache, &slice.card.logo);
        }
    }
}

/// What the test card says of a screen: its name and the size of its window in pixels.
pub(super) fn card_caption(name: &str, width: u32, height: u32) -> String {
    format!("{name}  {width} x {height}")
}

pub(super) fn title(name: &str) -> String {
    format!("FLUX video · {name}")
}

/// The test cards of a screen's slices laid out on the composition, small, as the Advanced Output
/// shows them over the picture: the width and height as two little-endian u16, then RGBA, clear
/// where no card is shown. Empty while no card is.
#[tauri::command]
pub async fn video_card_preview(
    app: AppHandle,
    output: State<'_, SharedVideoOutput>,
    screen: String,
) -> Result<tauri::ipc::Response, String> {
    let output = Arc::clone(&output);
    let bytes = tauri::async_runtime::spawn_blocking(move || output.card_preview(&app, &screen))
        .await
        .map_err(|err| err.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

impl VideoOutputs {
    fn card_preview(&self, app: &AppHandle, screen: &str) -> Vec<u8> {
        let session = Arc::clone(&app.state::<SharedSession>());
        let size = composition_size(&session);
        let screens = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .video_screens();
        let Some(found) = screens.iter().find(|s| s.id == screen) else {
            return Vec::new();
        };
        let everywhere =
            self.controls.card.load(Ordering::Relaxed) || self.pattern.load(Ordering::Relaxed);
        let mut slices: Vec<CardSlice> = found
            .slices
            .iter()
            .filter(|slice| !slice.hidden)
            .map(card_slice)
            .collect();
        if slices.is_empty() {
            slices.push(CardSlice {
                name: found.name.clone(),
                input: [0.0, 0.0, 1.0, 1.0],
                card: Default::default(),
                logo: None,
            });
        }
        if let Ok(cache) = crate::video_commands::video_cache(app) {
            with_logos(&mut slices, &session, &cache);
        }
        let cards = flux_render::pattern::layout(&slices, size, &found.name);
        let shown: Vec<_> = cards
            .into_iter()
            .zip(&slices)
            .filter(|(_, slice)| everywhere || slice.card.show)
            .map(|(card, _)| card)
            .collect();
        if shown.is_empty() {
            return Vec::new();
        }
        let (width, height, rgba) = flux_render::pattern::preview(&shown, size, 640);
        let mut out = Vec::with_capacity(rgba.len() + 4);
        out.extend_from_slice(&(width as u16).to_le_bytes());
        out.extend_from_slice(&(height as u16).to_le_bytes());
        out.extend_from_slice(&rgba);
        out
    }
}

/// The colour (`#rrggbb`) the test card of the slice at `index` takes when it names none.
#[tauri::command]
pub fn video_card_base(index: usize) -> String {
    let [r, g, b] = flux_render::palette::default_base(index);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Shows or hides the test pattern where there is no picture.
#[tauri::command]
pub fn video_output_pattern(output: State<'_, SharedVideoOutput>, on: bool) {
    output.pattern.store(on, Ordering::Relaxed);
}
