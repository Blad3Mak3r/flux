use super::*;

pub(super) fn pack(size: tauri::PhysicalSize<u32>) -> u64 {
    (u64::from(size.width) << 32) | u64::from(size.height)
}

pub(super) fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// The render thread's state.
pub(super) struct Render {
    pub(super) screen: Screen,
    /// The id of the screen it draws, whose slices it shows.
    pub(super) screen_id: String,
    pub(super) deck: SharedDeck,
    pub(super) session: SharedSession,
    /// Where the project's media is unpacked to be read.
    pub(super) cache: PathBuf,
    pub(super) transport: SharedOutput,
    pub(super) size: Arc<AtomicU64>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) pattern: Arc<AtomicBool>,
    pub(super) controls: Arc<Controls>,
    /// The screen's name, for the test card.
    pub(super) screen_name: String,
    /// Off, the window stays open and black.
    pub(super) enabled: bool,
    /// The timeline and the slices last copied from the session, and the revision they were
    /// copied at.
    pub(super) timeline: Option<Timeline>,
    pub(super) quads: Vec<Quad>,
    pub(super) masks: Vec<Mask>,
    pub(super) seen: (u64, String),
    pub(super) reader: Option<PositionReader>,
    /// The test card of each slice, made when one is shown and again when the slices or their
    /// settings change; each one's frame with its moving part and the id its picture goes by.
    pub(super) cards: Option<Vec<flux_render::pattern::SliceCard>>,
    pub(super) card_size: (u32, u32),
    pub(super) card_slices: Vec<CardSlice>,
    /// What the card says of the screen, made again when it changes.
    pub(super) card_caption: String,
    pub(super) card_frames: Vec<Vec<u8>>,
    pub(super) card_ids: Vec<u64>,
    /// Counts the pictures the cards made, for their ids.
    pub(super) card_seq: u64,
    /// Where the clips were decoded at the last refresh, written to the log when it changes.
    pub(super) decoding: Option<flux_video::Decoding>,
    /// What the VJ grid plays, and the grid copied from the session for a VJ project, which
    /// shows its grid instead of a timeline.
    pub(super) grid: crate::vj_commands::SharedGrid,
    /// Puts the modulated values in the grid it draws.
    pub(super) modulation: crate::modulation::SharedModulation,
    pub(super) vj: Option<flux_project::vj::Vj>,
    /// The grid with its modulated values, kept between frames and patched; reset with `vj`.
    pub(super) drawn: Option<flux_project::vj::Vj>,
    /// When the last frame was drawn, to tell the effects with a history how long it has been.
    pub(super) last_frame: Option<Instant>,
}

/// The size the test card is made at when the project has no video; the GPU scales it to each
/// slice.
pub(super) const CARD_SIZE: (u32, u32) = (1920, 1080);

impl Render {
    pub(super) fn run(mut self) {
        while !self.stop.load(Ordering::Relaxed) {
            let (width, height) = unpack(self.size.load(Ordering::Relaxed));
            self.screen.resize(width, height);
            let drawn = self.step();
            if !drawn {
                // Nothing on show, or a window the system is not showing: no need for every refresh.
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// Draws one refresh; `false` when it drew black or nothing.
    fn step(&mut self) -> bool {
        self.refresh();
        let bpm = self.timeline.as_ref().map_or(120.0, |t| t.tempo.bpm);
        let at = Instant::now();
        let dt = self
            .last_frame
            .replace(at)
            .map_or(1.0 / 60.0, |last| at.duration_since(last).as_secs_f32());
        let (shown, looks, time, post) = match &self.vj {
            Some(vj) => {
                let frame = grid_frame(
                    vj,
                    bpm,
                    &GridSources {
                        grid: &self.grid,
                        modulation: &self.modulation,
                        deck: &self.deck,
                    },
                    &mut self.drawn,
                    self.card_size,
                    dt,
                );
                (frame.shown, frame.looks, frame.time, frame.post)
            }
            None => (
                self.timeline_pictures(),
                Vec::new(),
                flux_render::Time::default(),
                flux_render::Post::default(),
            ),
        };
        if self.screen_id == flux_project::mapping::MAIN_SCREEN {
            self.note_decoding();
        }
        let disabled = self.controls.disabled.load(Ordering::Relaxed) || !self.enabled;
        let forced = self.controls.card.load(Ordering::Relaxed);
        let mut pictures = if disabled || forced {
            Vec::new()
        } else {
            items(&shown, &looks, self.card_size, time)
        };
        // The whole card replaces the picture; a slice that asks for its own is laid over it.
        let everywhere = forced || (pictures.is_empty() && self.pattern.load(Ordering::Relaxed));
        let on = if disabled {
            Vec::new()
        } else {
            self.prepare_cards(everywhere)
        };
        let card = !on.is_empty();
        for &at in &on {
            let [x, y, width, height] = self.cards.as_ref().map_or([0; 4], |cards| cards[at].rect);
            let (cw, ch) = (self.card_size.0 as f32, self.card_size.1 as f32);
            let fixed = self.cards.as_ref().map(|cards| &cards[at].fixed);
            let moving = &self.card_frames[at];
            pictures.push(flux_render::Item::Picture(Picture {
                id: self.card_ids[at],
                width,
                height,
                rgba: if moving.is_empty() {
                    fixed.map_or(&[][..], |f| &f[..])
                } else {
                    moving
                },
                place: [
                    x as f32 / cw,
                    y as f32 / ch,
                    width as f32 / cw,
                    height as f32 / ch,
                ],
                opacity: 1.0,
                blend: flux_render::Blend::Alpha,
                effects: Default::default(),
                nearest: true,
            }));
        }
        let drawn = !pictures.is_empty();
        // The test card is what the slices are lined up with: nothing changes it.
        let post = if card || disabled {
            flux_render::Post::default()
        } else {
            post
        };
        // A lost surface draws nothing and is tried again on the next refresh.
        self.screen
            .draw(&pictures, self.card_size, &self.quads, &self.masks, &post)
            .is_ok()
            && drawn
    }

    /// The layers the timeline shows where the transport is, topmost first.
    fn timeline_pictures(&mut self) -> Vec<flux_video::Shown> {
        // The transport is locked only while it changes: then the last reader is used.
        if let Ok(output) = self.transport.try_lock() {
            self.reader = output.as_ref().map(|running| running.reader());
        }
        let (timeline, cue) = match &self.reader {
            Some(reader) => {
                let (state, frame) = reader.timeline_frame();
                let cue = if state == TransportState::Stopped {
                    Cue::Black
                } else {
                    Cue::At(frame)
                };
                match reader.show() {
                    Some((plan, tick)) => (Some(plan.entries[tick.entry].timeline.clone()), cue),
                    None => (self.timeline.clone(), cue),
                }
            }
            None => (None, Cue::Black),
        };
        match &timeline {
            // Held only while the deck asks for frames: the pictures are shared, not copied.
            Some(timeline) => self
                .deck
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .map(|deck| deck.show(timeline, cue))
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Writes to the log where the clips are decoded when it changes; the main screen's window
    /// does it for all of them, which share one deck.
    fn note_decoding(&mut self) {
        let now = self
            .deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(flux_video::VideoDeck::decoding);
        if now != self.decoding {
            if let Some(decoding) = now {
                log::info!("video: decoding on {}", decoding_name(decoding));
            }
            self.decoding = now;
        }
    }

    /// Makes the cards of the slices to show (all of them when `everywhere`, else those that ask
    /// for it) and their moving part for now; the indexes of the cards to draw.
    fn prepare_cards(&mut self, everywhere: bool) -> Vec<usize> {
        let wanted: Vec<usize> = (0..self.card_slices.len())
            .filter(|&at| everywhere || self.card_slices[at].card.show)
            .collect();
        if wanted.is_empty() {
            return wanted;
        }
        let (window_width, window_height) = unpack(self.size.load(Ordering::Relaxed));
        let caption = card_caption(&self.screen_name, window_width, window_height);
        if caption != self.card_caption {
            self.card_caption = caption;
            self.cards = None;
        }
        if self.cards.is_none() {
            let cards =
                flux_render::pattern::layout(&self.card_slices, self.card_size, &self.card_caption);
            self.card_frames = vec![Vec::new(); cards.len()];
            self.card_ids = cards.iter().map(|_| self.next_card_id()).collect();
            self.cards = Some(cards);
        }
        // Timed by the system clock, so the marker crosses every screen together, a minute being a
        // whole number of crossings.
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() % 60_000);
        let seconds = millis as f32 / 1000.0;
        let clock = chrono::Local::now().format("%H:%M:%S").to_string();
        for &at in &wanted {
            let moved = self
                .cards
                .as_ref()
                .is_some_and(|cards| cards[at].frame(&mut self.card_frames[at], seconds, &clock));
            if moved {
                // Each refresh is a new picture.
                self.card_ids[at] = self.next_card_id();
            } else {
                self.card_frames[at].clear();
            }
        }
        wanted
    }

    /// An id for a picture of a card: high, apart from the hashes of video frames.
    fn next_card_id(&mut self) -> u64 {
        self.card_seq += 1;
        u64::MAX - self.card_seq
    }

    /// Copies the timeline being edited and the slices again when the document changed.
    fn refresh(&mut self) {
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let (revision, active) = session.revision();
        if (revision, active) == (self.seen.0, self.seen.1.as_str()) {
            return;
        }
        self.seen = (revision, active.to_owned());
        self.timeline = session.active_timeline_copy();
        self.vj = session.grid_copy();
        self.drawn = None;
        let screens = session.video_screens();
        let this = screens.iter().find(|screen| screen.id == self.screen_id);
        let slices = this.map_or(&[][..], |screen| &screen.slices);
        self.masks = this.map_or_else(Vec::new, |screen| masks_of(&screen.masks));
        if let Some(screen) = this {
            self.screen_name.clone_from(&screen.name);
            self.enabled = screen.enabled;
        }
        let shown = slices.iter().filter(|slice| !slice.hidden);
        self.quads = shown.clone().map(quad_of).collect();
        let mut card_slices: Vec<CardSlice> = shown.map(card_slice).collect();
        if card_slices.is_empty() {
            // No slices: the card is of the whole picture, under the screen's name.
            card_slices.push(CardSlice {
                name: self.screen_name.clone(),
                input: [0.0, 0.0, 1.0, 1.0],
                card: Default::default(),
                logo: None,
            });
        }
        drop(session);
        with_logos(&mut card_slices, &self.session, &self.cache);
        let size = composition_size(&self.session);
        if (size, &card_slices) != (self.card_size, &self.card_slices) {
            self.card_size = size;
            self.card_slices = card_slices;
            self.cards = None;
        }
    }
}

/// `cpu`, or the GPU device's name.
pub(super) fn decoding_name(decoding: flux_video::Decoding) -> String {
    match decoding {
        flux_video::Decoding::Software => "cpu".to_owned(),
        flux_video::Decoding::Hardware(device) => device.to_owned(),
    }
}

/// Whether a picture at `place` fills the whole composition, hiding what is under it.
pub(super) fn covers(place: [f32; 4]) -> bool {
    const SLACK: f32 = 1e-4;
    let [x, y, w, h] = place;
    x <= SLACK && y <= SLACK && x + w >= 1.0 - SLACK && y + h >= 1.0 - SLACK
}

/// The renderer's sizing for a clip's fit.
pub(super) fn sizing(fit: flux_project::VideoFit) -> flux_render::Sizing {
    use flux_project::VideoFit;
    match fit {
        VideoFit::Fit => flux_render::Sizing::Fit,
        VideoFit::Fill => flux_render::Sizing::Fill,
        VideoFit::Stretch => flux_render::Sizing::Stretch,
        VideoFit::None => flux_render::Sizing::None,
    }
}

/// The render's blend for a layer's.
pub(super) fn blend(blend: flux_project::vj::Blend) -> flux_render::Blend {
    use flux_project::vj::Blend as B;
    match blend {
        B::Alpha => flux_render::Blend::Alpha,
        B::Add => flux_render::Blend::Add,
        B::Subtract => flux_render::Blend::Subtract,
        B::Multiply => flux_render::Blend::Multiply,
        B::Screen => flux_render::Blend::Screen,
        B::Lighten => flux_render::Blend::Lighten,
        B::Darken => flux_render::Blend::Darken,
        B::Difference => flux_render::Blend::Difference,
        B::Exclusion => flux_render::Blend::Exclusion,
        B::Overlay => flux_render::Blend::Overlay,
        B::SoftLight => flux_render::Blend::SoftLight,
        B::HardLight => flux_render::Blend::HardLight,
        B::ColorDodge => flux_render::Blend::ColorDodge,
        B::ColorBurn => flux_render::Blend::ColorBurn,
        B::LinearLight => flux_render::Blend::LinearLight,
        B::VividLight => flux_render::Blend::VividLight,
        B::PinLight => flux_render::Blend::PinLight,
        B::Hue => flux_render::Blend::Hue,
        B::Saturation => flux_render::Blend::Saturation,
        B::Color => flux_render::Blend::Color,
        B::Luminosity => flux_render::Blend::Luminosity,
        B::AlphaCut => flux_render::Blend::AlphaCut,
        B::LumaKey => flux_render::Blend::LumaKey,
        B::LumaKeyInverted => flux_render::Blend::LumaKeyInverted,
        B::LumaIsAlpha => flux_render::Blend::LumaIsAlpha,
        B::DifferenceInverted => flux_render::Blend::DifferenceInverted,
        B::Mask50 => flux_render::Blend::Mask50,
        B::Rgb => flux_render::Blend::Rgb,
        B::Displace => flux_render::Blend::Displace,
    }
}
