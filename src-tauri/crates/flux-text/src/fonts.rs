use super::*;

/// The names a weight goes by, in the order of CSS weights from 100.
pub(super) const WEIGHTS: [&str; 9] = [
    "Thin",
    "ExtraLight",
    "Light",
    "Regular",
    "Medium",
    "SemiBold",
    "Bold",
    "ExtraBold",
    "Black",
];

/// The name of a face: its weight, then `Italic` or `Oblique`; `Regular` for the plain one and
/// `Italic` alone for a regular weight that leans.
pub(super) fn style_name(weight: u16, style: Style) -> String {
    let word = WEIGHTS[(usize::from(weight.clamp(100, 900)) / 100 - 1).min(8)];
    let lean = match style {
        Style::Normal => return word.to_owned(),
        Style::Italic => "Italic",
        Style::Oblique => "Oblique",
    };
    if word == "Regular" {
        lean.to_owned()
    } else {
        format!("{word} {lean}")
    }
}

/// The weight and slant a name of [`style_name`] stands for; anything else is `Regular`.
pub(super) fn parse_style(name: &str) -> (u16, Style) {
    let lower = name.to_lowercase();
    let (word, style) = if let Some(word) = lower.strip_suffix("oblique") {
        (word, Style::Oblique)
    } else if let Some(word) = lower.strip_suffix("italic") {
        (word, Style::Italic)
    } else {
        (lower.as_str(), Style::Normal)
    };
    let word = word.trim();
    let weight = WEIGHTS
        .iter()
        .position(|w| w.to_lowercase() == word)
        .map_or(400, |i| 100 * (i as u16 + 1));
    (weight, style)
}

/// One face of a font file, opened for shaping and for drawing from the same bytes.
pub struct Face {
    pub(super) shaper: rustybuzz::Face<'static>,
    pub(super) glyphs: FontRef<'static>,
    /// Font units in an em.
    pub(super) upem: f32,
    /// Tells two faces apart, for the cache of drawn blocks.
    pub(super) key: u64,
}

impl PartialEq for Face {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Face {
    /// `None` when `data` is no font. The bytes live as long as the program: a face is opened once
    /// and kept (see [`Fonts`]), so they are never freed anyway.
    fn open(data: &'static [u8], index: u32) -> Option<Arc<Self>> {
        static KEYS: AtomicU64 = AtomicU64::new(0);
        let shaper = rustybuzz::Face::from_slice(data, index)?;
        let glyphs = FontRef::try_from_slice_and_index(data, index).ok()?;
        Some(Arc::new(Self {
            upem: shaper.units_per_em().max(1) as f32,
            shaper,
            glyphs,
            key: KEYS.fetch_add(1, Ordering::Relaxed),
        }))
    }

    /// Whether the face has a glyph for `ch`.
    pub(super) fn has(&self, ch: char) -> bool {
        self.glyphs.glyph_id(ch).0 != 0
    }

    /// The scale that draws an em of `em` pixels, the same em the shaper's advances are scaled by
    /// (`em / upem`). A `PxScale` is the font's height, not its em. Not `pt_to_px_scale`: it also
    /// multiplies by 96/72, which drew the glyphs a third wider than their advances.
    pub(super) fn scale(&self, em: f32) -> PxScale {
        PxScale::from(em * self.glyphs.height_unscaled() / self.upem)
    }
}

/// A character at a weight, and whether the slant is upright.
pub(super) type Wanted = (char, u16, bool);

pub(super) struct Fonts {
    db: Database,
    /// Faces already opened, by family and style as asked.
    named: Mutex<HashMap<(String, String), Arc<Face>>>,
    /// Faces of the system opened by id, `None` when the file is no font.
    by_id: Mutex<HashMap<fontdb::ID, Option<Arc<Face>>>>,
    /// The face the system has for a character at a weight and slant, `None` when none does.
    fallback: Mutex<HashMap<Wanted, Option<Arc<Face>>>>,
}

pub(super) fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        Fonts {
            db,
            named: Mutex::default(),
            by_id: Mutex::default(),
            fallback: Mutex::default(),
        }
    })
}

pub(super) fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the system's fonts now, on a thread of its own, so the first text does not wait for them.
pub fn warm() {
    std::thread::spawn(|| {
        fonts();
    });
}

pub(super) fn bundled(weight: u16) -> Arc<Face> {
    static CACHE: OnceLock<[Arc<Face>; 2]> = OnceLock::new();
    let both = CACHE.get_or_init(|| {
        [REGULAR, BOLD]
            .map(|bytes| Face::open(bytes, 0).expect("the bundled font is a valid font file"))
    });
    both[usize::from(weight >= 600)].clone()
}

impl Fonts {
    /// The face of the system with `id`, opened once.
    fn face(&self, id: fontdb::ID) -> Option<Arc<Face>> {
        if let Some(found) = locked(&self.by_id).get(&id) {
            return found.clone();
        }
        // The bytes are copied once and kept for good: the face stays in the cache.
        let opened = self
            .db
            .with_face_data(id, |data, index| {
                Face::open(Box::leak(data.to_vec().into_boxed_slice()), index)
            })
            .flatten();
        locked(&self.by_id).insert(id, opened.clone());
        opened
    }

    /// The face of the system that has `ch`, the closest to `want` in weight and slant first; one
    /// in colour (bitmaps or COLR layers) first, else one with an outline, else any.
    pub(super) fn covering(&self, ch: char, want: (u16, Style)) -> Option<Arc<Face>> {
        let key = (ch, want.0, want.1 == Style::Normal);
        if let Some(found) = locked(&self.fallback).get(&key) {
            return found.clone();
        }
        let mut faces: Vec<&fontdb::FaceInfo> = self.db.faces().collect();
        faces.sort_by_key(|f| (f.style != want.1, f.weight.0.abs_diff(want.0)));
        // The cmap is read from the file without opening the face for drawing.
        let covers = |face: &fontdb::FaceInfo, pass: u8| {
            self.db
                .with_face_data(face.id, |data, index| {
                    let face = ttf_parser::Face::parse(data, index).ok()?;
                    let glyph = face.glyph_index(ch)?;
                    match pass {
                        0 => {
                            face.glyph_raster_image(glyph, 128).is_some()
                                || face.is_color_glyph(glyph)
                        }
                        1 => face.glyph_bounding_box(glyph).is_some(),
                        _ => true,
                    }
                    .then_some(())
                })
                .flatten()
                .is_some()
        };
        let found = (0..3)
            .find_map(|pass| faces.iter().find(|f| covers(f, pass)))
            .and_then(|f| self.face(f.id));
        locked(&self.fallback).insert(key, found.clone());
        found
    }
}

/// The face for `family` in `style`: the closest one the machine has, else the bundled font. It
/// never fails.
pub fn font(family: &str, style: &str) -> Arc<Face> {
    let (weight, slant) = parse_style(style);
    let fonts = fonts();
    let key = (family.to_lowercase(), style.to_lowercase());
    if let Some(found) = locked(&fonts.named).get(&key) {
        return found.clone();
    }
    let query = Query {
        families: &[Family::Name(family)],
        weight: Weight(weight),
        stretch: Stretch::Normal,
        style: slant,
    };
    let found = fonts
        .db
        .query(&query)
        .and_then(|id| fonts.face(id))
        .unwrap_or_else(|| bundled(weight));
    locked(&fonts.named).insert(key, found.clone());
    found
}

/// The families the machine has, by name, each with its faces from the lightest; the bundled one
/// is in the list too, so there is always something to pick.
pub fn families() -> Vec<FontFamily> {
    let mut found: HashMap<String, Vec<(u16, Style)>> = HashMap::new();
    for face in fonts().db.faces() {
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        found
            .entry(name.clone())
            .or_default()
            .push((face.weight.0, face.style));
    }
    found
        .entry(BUNDLED_FAMILY.to_owned())
        .or_insert_with(|| vec![(400, Style::Normal), (700, Style::Normal)]);
    let mut all: Vec<FontFamily> = found
        .into_iter()
        .map(|(name, mut faces)| {
            faces.sort_by_key(|&(weight, style)| (style != Style::Normal, weight));
            let mut styles: Vec<String> = Vec::new();
            for (weight, style) in faces {
                let named = style_name(weight, style);
                if !styles.contains(&named) {
                    styles.push(named);
                }
            }
            FontFamily { name, styles }
        })
        .collect();
    all.sort_by_key(|family| family.name.to_lowercase());
    all
}

// ---------------------------------------------------------------------------------------------
// Layout and drawing
