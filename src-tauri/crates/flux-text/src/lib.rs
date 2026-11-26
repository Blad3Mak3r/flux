//! Text as a picture: the fonts the system has (and one bundled, so a missing font never stops a
//! text from drawing), a block of lines laid out and rasterised on the CPU into straight RGBA over a
//! transparent background. Pure Rust: `fontdb` finds the fonts, `rustybuzz` (a HarfBuzz port) shapes
//! each line (GPOS kerning, ligatures, marks) and `ab_glyph` draws the glyph ids it returns.
//!
//! A character the chosen face does not have is drawn with another face: a face of the system whose
//! cmap has it (the same style first), else the bundled Inter. A line is cut into runs of one face
//! and each run is shaped on its own. A glyph the face keeps in colour (bitmaps in CBDT or sbix,
//! layers in COLR v0/v1: see [`colour`]) is drawn with its own colours, only fading with the
//! block's opacity; any other glyph is an outline drawn in the block's colour.

mod colour;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use rustybuzz::ttf_parser;

/// The family that is bundled: the fallback for a family the machine does not have.
pub const BUNDLED_FAMILY: &str = "Inter";

const REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular-subset.otf");
const BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold-subset.otf");

/// Largest side of the bitmap one block is drawn in; a block past it is cut.
const MAX_SIDE: usize = 4096;
/// Blank pixels round the block, so a glyph that leans out of its advance is not cut.
const PAD: usize = 2;

/// How the lines of a block sit against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// A family the machine has, with the names of its faces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    pub name: String,
    pub styles: Vec<String>,
}

/// A block of text to draw on a canvas; sizes are in pixels of that canvas.
#[derive(Debug, Clone)]
pub struct Block<'a> {
    /// Its lines, split by `\n`.
    pub text: &'a str,
    pub family: &'a str,
    pub style: &'a str,
    /// The em of the font, in pixels.
    pub size: f32,
    pub colour: [u8; 3],
    /// 0 to 1: the alpha of the glyphs.
    pub opacity: f32,
    pub align: Align,
    /// Where the centre of the block is.
    pub centre: [f32; 2],
    /// Degrees, clockwise, about the centre.
    pub rotation: f32,
    /// Extra space after each glyph, in ems.
    pub tracking: f32,
    /// Extra space between lines, in ems.
    pub leading: f32,
}

// ---------------------------------------------------------------------------------------------
// Fonts

/// The names a weight goes by, in the order of CSS weights from 100.
const WEIGHTS: [&str; 9] = [
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
fn style_name(weight: u16, style: Style) -> String {
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
fn parse_style(name: &str) -> (u16, Style) {
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
    shaper: rustybuzz::Face<'static>,
    glyphs: FontRef<'static>,
    /// Font units in an em.
    upem: f32,
    /// Tells two faces apart, for the cache of drawn blocks.
    key: u64,
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
    fn has(&self, ch: char) -> bool {
        self.glyphs.glyph_id(ch).0 != 0
    }

    /// The scale that draws an em of `em` pixels, the same em the shaper's advances are scaled by
    /// (`em / upem`). A `PxScale` is the font's height, not its em. Not `pt_to_px_scale`: it also
    /// multiplies by 96/72, which drew the glyphs a third wider than their advances.
    fn scale(&self, em: f32) -> PxScale {
        PxScale::from(em * self.glyphs.height_unscaled() / self.upem)
    }
}

/// A character at a weight, and whether the slant is upright.
type Wanted = (char, u16, bool);

struct Fonts {
    db: Database,
    /// Faces already opened, by family and style as asked.
    named: Mutex<HashMap<(String, String), Arc<Face>>>,
    /// Faces of the system opened by id, `None` when the file is no font.
    by_id: Mutex<HashMap<fontdb::ID, Option<Arc<Face>>>>,
    /// The face the system has for a character at a weight and slant, `None` when none does.
    fallback: Mutex<HashMap<Wanted, Option<Arc<Face>>>>,
}

fn fonts() -> &'static Fonts {
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

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the system's fonts now, on a thread of its own, so the first text does not wait for them.
pub fn warm() {
    std::thread::spawn(|| {
        fonts();
    });
}

fn bundled(weight: u16) -> Arc<Face> {
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
    fn covering(&self, ch: char, want: (u16, Style)) -> Option<Arc<Face>> {
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

/// A glyph of a line: its face, its id there, and where it sits against the start of the line and
/// its baseline (up is positive).
struct Placed {
    face: Arc<Face>,
    id: GlyphId,
    x: f32,
    up: f32,
}

/// One line of glyphs, and how long the line is.
struct Line {
    glyphs: Vec<Placed>,
    width: f32,
}

/// Whether `ch` joins the glyph before it instead of having a face of its own: a zero width joiner
/// or mark, or a variation selector, which no font has in its cmap.
fn joins(ch: char) -> bool {
    matches!(ch, '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// `line` cut into runs of the same face, as `face_of` picks it for each character.
fn split_runs<F: PartialEq>(line: &str, mut face_of: impl FnMut(char) -> F) -> Vec<(F, &str)> {
    let mut runs: Vec<(F, usize, usize)> = Vec::new();
    for (at, ch) in line.char_indices() {
        let end = at + ch.len_utf8();
        match runs.last_mut() {
            Some(last) if joins(ch) => last.2 = end,
            _ => {
                let face = face_of(ch);
                match runs.last_mut() {
                    Some(last) if last.0 == face => last.2 = end,
                    _ => runs.push((face, at, end)),
                }
            }
        }
    }
    runs.into_iter()
        .map(|(face, from, to)| (face, &line[from..to]))
        .collect()
}

/// The face that draws `ch`: the chosen one, else one of the system that has it, else the bundled
/// Inter; the chosen one again (its blank glyph) when nobody has it.
fn face_for(chosen: &Arc<Face>, want: (u16, Style), ch: char) -> Arc<Face> {
    if chosen.has(ch) || ch.is_whitespace() {
        return chosen.clone();
    }
    if let Some(found) = fonts().covering(ch, want) {
        return found;
    }
    let inter = bundled(want.0);
    if inter.has(ch) { inter } else { chosen.clone() }
}

fn layout_line(chosen: &Arc<Face>, want: (u16, Style), em: f32, line: &str, tracking: f32) -> Line {
    use rustybuzz::{Feature, UnicodeBuffer, ttf_parser::Tag};
    let mut glyphs = Vec::with_capacity(line.len());
    let mut pen = 0.0;
    // Space between glyphs would show a ligature as one stuck glyph: ligatures are off then.
    let off = |tag: &[u8; 4]| Feature::new(Tag::from_bytes(tag), 0, ..);
    let features: &[Feature] = &[off(b"liga"), off(b"clig")];
    let features = if tracking == 0.0 { &[] } else { features };
    for (face, run) in split_runs(line, |ch| face_for(chosen, want, ch)) {
        let k = em / face.upem;
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face.shaper, features, buffer);
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            glyphs.push(Placed {
                face: face.clone(),
                id: GlyphId(info.glyph_id as u16),
                x: pen + pos.x_offset as f32 * k,
                up: pos.y_offset as f32 * k,
            });
            pen += pos.x_advance as f32 * k + tracking;
        }
    }
    Line {
        glyphs,
        width: if line.is_empty() { 0.0 } else { pen - tracking },
    }
}

/// The coverage (0 to 255) of a block of lines.
struct Cover {
    width: usize,
    height: usize,
    bytes: Vec<u8>,
    /// Premultiplied RGBA of the colour glyphs, drawn over `bytes`; empty when there are none.
    colour: Vec<u8>,
}

/// The blocks drawn last. Moving, turning and fading a block only resamples its coverage, so the
/// glyphs are drawn again only when the text, the face or the spacing change.
static COVERS: Mutex<Vec<(u64, Arc<Cover>)>> = Mutex::new(Vec::new());
const KEPT_COVERS: usize = 8;

fn draw_block(face: &Arc<Face>, block: &Block<'_>) -> Arc<Cover> {
    let mut hasher = DefaultHasher::new();
    (face.key, block.text, block.align as u8).hash(&mut hasher);
    [block.size, block.tracking, block.leading]
        .map(f32::to_bits)
        .hash(&mut hasher);
    parse_style(block.style).hash(&mut hasher);
    let id = hasher.finish();
    if let Some((_, found)) = locked(&COVERS).iter().find(|(key, _)| *key == id) {
        return found.clone();
    }
    let cover = Arc::new(draw_lines(face, block));
    let mut kept = locked(&COVERS);
    if kept.len() >= KEPT_COVERS {
        kept.remove(0);
    }
    kept.push((id, cover.clone()));
    cover
}

fn draw_lines(face: &Arc<Face>, block: &Block<'_>) -> Cover {
    let em = block.size.max(1.0);
    let want = parse_style(block.style);
    let scaled = face.glyphs.as_scaled(face.scale(em));
    let tracking = block.tracking * em;
    let lines: Vec<Line> = block
        .text
        .split('\n')
        .map(|line| layout_line(face, want, em, line, tracking))
        .collect();
    let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let pitch = (scaled.height() + scaled.line_gap() + block.leading * em).max(1.0);
    let width = ((widest.ceil() as usize) + 2 * PAD).clamp(1, MAX_SIDE);
    let height = (((pitch * (lines.len() - 1) as f32 + scaled.height()).ceil() as usize) + 2 * PAD)
        .clamp(1, MAX_SIDE);
    let mut bytes = vec![0u8; width * height];
    let mut colour: Vec<u8> = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let indent = match block.align {
            Align::Left => 0.0,
            Align::Center => (widest - line.width) / 2.0,
            Align::Right => widest - line.width,
        };
        let baseline = PAD as f32 + scaled.ascent() + pitch * n as f32;
        for placed in &line.glyphs {
            let (ox, oy) = (PAD as f32 + indent + placed.x, baseline - placed.up);
            if let Some(tile) =
                colour::tile(&placed.face.shaper, ttf_parser::GlyphId(placed.id.0), em)
            {
                if colour.is_empty() {
                    colour = vec![0u8; width * height * 4];
                }
                over(
                    &mut colour,
                    (width, height),
                    &tile,
                    ox.round() as i64,
                    oy.round() as i64,
                );
                continue;
            }
            let glyph = placed.id.with_scale_and_position(
                placed.face.scale(em),
                point(PAD as f32 + indent + placed.x, baseline - placed.up),
            );
            let Some(outline) = placed.face.glyphs.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            let (left, top) = (bounds.min.x as i64, bounds.min.y as i64);
            outline.draw(|x, y, c| {
                let (x, y) = (left + i64::from(x), top + i64::from(y));
                if (0..width as i64).contains(&x) && (0..height as i64).contains(&y) {
                    let at = y as usize * width + x as usize;
                    bytes[at] = bytes[at].saturating_add((c * 255.0).round() as u8);
                }
            });
        }
    }
    Cover {
        width,
        height,
        bytes,
        colour,
    }
}

/// `tile` (premultiplied) laid over `canvas` (premultiplied RGBA) with its origin at `(ox, oy)`.
fn over(canvas: &mut [u8], size: (usize, usize), tile: &colour::Tile, ox: i64, oy: i64) {
    for ty in 0..tile.h {
        for tx in 0..tile.w {
            let (x, y) = (
                ox + i64::from(tile.x) + tx as i64,
                oy + i64::from(tile.y) + ty as i64,
            );
            if !(0..size.0 as i64).contains(&x) || !(0..size.1 as i64).contains(&y) {
                continue;
            }
            let src = &tile.rgba[(ty * tile.w + tx) * 4..][..4];
            let dst = &mut canvas[(y as usize * size.0 + x as usize) * 4..][..4];
            let keep = 255 - u16::from(src[3]);
            for c in 0..4 {
                dst[c] =
                    (u16::from(src[c]) + (u16::from(dst[c]) * keep + 127) / 255).min(255) as u8;
            }
        }
    }
}

/// The coverage at `(x, y)` of a bitmap, bilinear; what is outside is nothing.
fn sample(cover: &[u8], size: (usize, usize), x: f32, y: f32) -> f32 {
    sample_channel(cover, size, 1, 0, x, y)
}

/// [`sample`] of channel `ch` of a bitmap that has `stride` channels.
fn sample_channel(
    cover: &[u8],
    size: (usize, usize),
    stride: usize,
    ch: usize,
    x: f32,
    y: f32,
) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: i64, dy: i64| -> f32 {
        let (px, py) = (x0 as i64 + dx, y0 as i64 + dy);
        if (0..size.0 as i64).contains(&px) && (0..size.1 as i64).contains(&py) {
            f32::from(cover[(py as usize * size.0 + px as usize) * stride + ch])
        } else {
            0.0
        }
    };
    let top = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let bottom = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// `block` drawn with `font` on a canvas of `width` x `height`, as straight RGBA: the colour
/// everywhere, and the alpha where there are glyphs.
pub fn rasterise_with(font: &Arc<Face>, block: &Block<'_>, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = [block.colour[0], block.colour[1], block.colour[2], 0].repeat(w * h);
    let opacity = if block.opacity.is_nan() {
        1.0
    } else {
        block.opacity.clamp(0.0, 1.0)
    };
    if block.text.trim().is_empty() || opacity == 0.0 || w == 0 || h == 0 {
        return out;
    }
    let drawn = draw_block(font, block);
    let (bw, bh, cover) = (drawn.width, drawn.height, &drawn.bytes[..]);
    let (sin, cos) = block.rotation.to_radians().sin_cos();
    let straight = block.rotation == 0.0;
    // Unrotated, the block lands on whole pixels, so its glyphs stay as sharp as they were drawn.
    let centre = if straight {
        let corner = |c: f32, side: usize| (c - side as f32 / 2.0).round() + side as f32 / 2.0;
        [corner(block.centre[0], bw), corner(block.centre[1], bh)]
    } else {
        block.centre
    };
    let (half_w, half_h) = (bw as f32 / 2.0, bh as f32 / 2.0);
    // The box of the rotated block, cut to the canvas.
    let reach_x = half_w * cos.abs() + half_h * sin.abs();
    let reach_y = half_w * sin.abs() + half_h * cos.abs();
    let span = |c: f32, reach: f32, max: usize| {
        let lo = (c - reach).floor().max(0.0) as usize;
        let hi = ((c + reach).ceil().max(0.0) as usize).min(max);
        lo..hi
    };
    for y in span(centre[1], reach_y, h) {
        for x in span(centre[0], reach_x, w) {
            let (dx, dy) = (x as f32 + 0.5 - centre[0], y as f32 + 0.5 - centre[1]);
            // Back into the block's own axes.
            let sx = dx * cos + dy * sin + half_w - 0.5;
            let sy = -dx * sin + dy * cos + half_h - 0.5;
            let level = sample(cover, (bw, bh), sx, sy) * opacity;
            if drawn.colour.is_empty() {
                out[(y * w + x) * 4 + 3] = level.round().clamp(0.0, 255.0) as u8;
                continue;
            }
            // Colour glyphs keep their own colours over the text, and only fade with the opacity.
            let px: [f32; 4] = std::array::from_fn(|c| {
                sample_channel(&drawn.colour, (bw, bh), 4, c, sx, sy) * opacity
            });
            let keep = 1.0 - px[3] / 255.0;
            let alpha = px[3] + level * keep;
            let at = (y * w + x) * 4;
            for c in 0..3 {
                let text = f32::from(block.colour[c]) * level / 255.0 * keep;
                let rgb = if alpha > 0.0 {
                    (px[c] + text) * 255.0 / alpha
                } else {
                    f32::from(block.colour[c])
                };
                out[at + c] = rgb.round().clamp(0.0, 255.0) as u8;
            }
            out[at + 3] = alpha.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// `block` drawn on a canvas of `width` x `height`, in the font it names (see [`font`]).
pub fn rasterise(block: &Block<'_>, width: u32, height: u32) -> Vec<u8> {
    rasterise_with(&font(block.family, block.style), block, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str) -> Block<'_> {
        Block {
            text,
            family: BUNDLED_FAMILY,
            style: "Regular",
            size: 40.0,
            colour: [255, 128, 0],
            opacity: 1.0,
            align: Align::Center,
            centre: [100.0, 50.0],
            rotation: 0.0,
            tracking: 0.0,
            leading: 0.0,
        }
    }

    fn draw(block: &Block<'_>) -> Vec<u8> {
        // The bundled face, so the test needs no font of the machine.
        rasterise_with(&bundled(400), block, 200, 100)
    }

    /// The columns (first, last) with any glyph, over the rows from `from` to `to`.
    fn extent(picture: &[u8], rows: std::ops::Range<usize>) -> Option<(usize, usize)> {
        let lit = |x: usize| rows.clone().any(|y| picture[(y * 200 + x) * 4 + 3] > 0);
        let first = (0..200).find(|&x| lit(x))?;
        Some((first, (0..200).rev().find(|&x| lit(x))?))
    }

    fn covered(picture: &[u8]) -> usize {
        picture.chunks(4).filter(|p| p[3] > 0).count()
    }

    #[test]
    fn the_bundled_font_should_draw_accents_and_keep_the_colour_everywhere() {
        let picture = draw(&block("Canción"));
        assert_eq!(picture.len(), 200 * 100 * 4);
        assert!(covered(&picture) > 200, "some glyphs");
        assert!(picture.chunks(4).all(|p| p[..3] == [255, 128, 0]));
        let plain = draw(&block("Cancion"));
        assert_ne!(picture, plain, "the accent is drawn");
    }

    #[test]
    fn different_text_should_draw_different_pixels() {
        assert_ne!(draw(&block("one")), draw(&block("two")));
        assert_eq!(
            draw(&block("one")),
            draw(&block("one")),
            "and the same text the same"
        );
    }

    #[test]
    fn empty_text_and_no_opacity_should_draw_nothing() {
        assert_eq!(covered(&draw(&block(" \n "))), 0);
        let mut clear = block("text");
        clear.opacity = 0.0;
        assert_eq!(covered(&draw(&clear)), 0);
    }

    #[test]
    fn align_should_move_the_short_line_against_the_long_one() {
        let text = "WWWWWWWW\nii";
        // The second line is in the lower half of the block, below the centre row of 50.
        let lower = 50..100;
        let small = |align| Block {
            size: 16.0,
            align,
            ..block(text)
        };
        let (left, centre, right) = (
            small(Align::Left),
            small(Align::Center),
            small(Align::Right),
        );
        let first = |b: &Block<'_>| extent(&draw(b), lower.clone()).unwrap().0;
        let (l, c, r) = (first(&left), first(&centre), first(&right));
        assert!(l < c && c < r, "{l} {c} {r}");
    }

    #[test]
    fn position_and_size_should_move_and_grow_the_block() {
        let mut moved = block("W");
        moved.centre = [150.0, 50.0];
        let (a, b) = (
            extent(&draw(&block("W")), 0..100).unwrap(),
            extent(&draw(&moved), 0..100).unwrap(),
        );
        assert!(b.0 > a.0 + 40, "{a:?} {b:?}");
        let mut big = block("W");
        big.size = 80.0;
        assert!(covered(&draw(&big)) > covered(&draw(&block("W"))) * 2);
    }

    #[test]
    fn tracking_should_widen_a_line_and_leading_should_tall_the_block() {
        let mut wide = block("WWW");
        wide.tracking = 0.5;
        let (a, b) = (
            extent(&draw(&block("WWW")), 0..100).unwrap(),
            extent(&draw(&wide), 0..100).unwrap(),
        );
        assert!(b.1 - b.0 > a.1 - a.0 + 20);
        let rows = |picture: &[u8]| {
            let lit: Vec<usize> = (0..100)
                .filter(|&y| (0..200).any(|x| picture[(y * 200 + x) * 4 + 3] > 0))
                .collect();
            lit.last().unwrap() - lit.first().unwrap()
        };
        let two = Block {
            size: 16.0,
            ..block("W\nW")
        };
        let loose = Block {
            leading: 0.5,
            ..two.clone()
        };
        assert!(rows(&draw(&loose)) > rows(&draw(&two)) + 5);
    }

    #[test]
    fn rotation_should_turn_a_wide_block_tall() {
        let mut turned = block("WWWWWW");
        turned.rotation = 90.0;
        let flat = draw(&block("WWWWWW"));
        let upright = draw(&turned);
        let width = |p: &[u8]| {
            let (a, b) = extent(p, 0..100).unwrap();
            b - a
        };
        assert!(width(&upright) < width(&flat) / 2);
    }

    #[test]
    fn a_missing_family_should_fall_back_to_the_bundled_font() {
        let font = font("No Such Family 7", "Bold");
        assert!(font.has('A'));
        assert!(families().iter().any(|f| f.name == BUNDLED_FAMILY));
    }

    fn width_of(text: &str, tracking: f32) -> f32 {
        let face = bundled(400);
        layout_line(&face, (400, Style::Normal), 100.0, text, tracking).width
    }

    #[test]
    fn shaping_should_kern_pairs_the_font_kerns_in_gpos() {
        let (pair, apart) = (width_of("AV", 0.0), width_of("A", 0.0) + width_of("V", 0.0));
        assert!(pair < apart - 1.0, "{pair} is not under {apart}");
    }

    #[test]
    fn tracking_should_still_widen_a_shaped_line() {
        let loose = width_of("AVA", 10.0);
        assert!(
            (loose - (width_of("AVA", 0.0) + 20.0)).abs() < 0.5,
            "{loose}"
        );
    }

    #[test]
    fn runs_should_split_where_the_face_changes_and_keep_joiners_with_what_they_follow() {
        let face = |c: char| u8::from(c.is_ascii());
        let runs = split_runs("ab\u{416}\u{200D}c", face);
        assert_eq!(runs, vec![(1, "ab"), (0, "\u{416}\u{200D}"), (1, "c")]);
        assert!(split_runs("", face).is_empty());
    }

    #[test]
    fn a_character_no_font_has_should_not_stop_the_line_and_keep_the_others_in_place() {
        // A private use character: no face has it, so it draws as a blank or the face's own gap.
        let plain = width_of("AB", 0.0);
        let with = width_of("A\u{10FFFD}B", 0.0);
        assert!(with >= plain, "{with} {plain}");
        assert!(with < plain + 200.0, "{with} {plain}");
        let mut text = block("A\u{10FFFD}B \u{416}\u{2605}\u{1F389}");
        text.size = 20.0;
        assert!(covered(&draw(&text)) > 0);
    }

    #[test]
    fn a_character_the_chosen_face_lacks_should_fall_through_to_a_face_that_has_it() {
        // The bundled subset is Latin only: Cyrillic is absent, so another face is looked for.
        let inter = bundled(400);
        assert!(!inter.has('\u{416}'));
        let found = face_for(&inter, (400, Style::Normal), '\u{416}');
        assert!(
            found.has('\u{416}') || Arc::ptr_eq(&found, &inter),
            "a face with it, or the chosen one when the machine has none"
        );
        assert!(width_of("\u{416}\u{416}", 0.0) >= 0.0);
    }

    #[test]
    fn moving_and_fading_a_block_should_reuse_the_glyphs_it_drew() {
        let mut a = block("cached");
        let first = draw_block(&bundled(400), &a);
        a.centre = [10.0, 10.0];
        a.rotation = 30.0;
        a.opacity = 0.5;
        assert!(Arc::ptr_eq(&first, &draw_block(&bundled(400), &a)));
        a.size = 41.0;
        assert!(!Arc::ptr_eq(&first, &draw_block(&bundled(400), &a)));
    }

    #[test]
    fn a_colour_tile_should_land_at_its_offset_and_keep_the_pixels_under_it_when_clear() {
        let tile = colour::Tile {
            x: -1,
            y: -1,
            w: 2,
            h: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
        };
        let mut canvas = vec![0u8; 4 * 3 * 4];
        canvas[(2 * 4 + 1) * 4..][..4].copy_from_slice(&[0, 9, 0, 255]);
        over(&mut canvas, (4, 3), &tile, 2, 2);
        // Tile origin (2, 2) minus (1, 1): the red pixel is at (1, 1), the clear one over (2, 1).
        assert_eq!(&canvas[(4 + 1) * 4..][..4], &[255, 0, 0, 255]);
        assert_eq!(&canvas[(2 * 4 + 1) * 4..][..4], &[0, 9, 0, 255]);
    }

    #[test]
    fn style_names_should_read_back() {
        for (weight, style) in [
            (400, Style::Normal),
            (700, Style::Normal),
            (400, Style::Italic),
            (700, Style::Italic),
            (300, Style::Oblique),
        ] {
            assert_eq!(parse_style(&style_name(weight, style)), (weight, style));
        }
        assert_eq!(style_name(700, Style::Italic), "Bold Italic");
        assert_eq!(parse_style("nonsense"), (400, Style::Normal));
    }

    /// Columns (first, last) of the ink of `text` in a bitmap as wide as its own cover.
    fn ink_columns(block: &Block<'_>) -> (usize, usize, usize) {
        let cover = draw_lines(
            &bundled(if block.style == "Bold" { 700 } else { 400 }),
            block,
        );
        let lit = |x: usize| (0..cover.height).any(|y| cover.bytes[y * cover.width + x] > 0);
        let first = (0..cover.width).find(|&x| lit(x)).unwrap();
        let last = (0..cover.width).rev().find(|&x| lit(x)).unwrap();
        (first, last, cover.width)
    }

    #[test]
    fn ink_should_stay_inside_the_bitmap_for_overhanging_glyphs() {
        for text in ["BEBA", "AVA", "W", "AVATAR", "fj"] {
            for tracking in [0.0, 0.1, -0.05] {
                for style in ["Regular", "Bold"] {
                    let size = 300.0;
                    let (first, last, width) = ink_columns(&Block {
                        size,
                        tracking,
                        style,
                        ..block(text)
                    });
                    assert!(
                        first >= 1 && last + 2 <= width,
                        "{text} {style} {tracking}: {first}..{last} of {width}"
                    );
                }
            }
        }
    }

    #[test]
    fn drawn_ink_should_match_the_measured_width() {
        // At tracking 0 the line is as wide as its advances: the ink is within a few percent of it.
        let b = Block {
            size: 300.0,
            ..block("BEBA")
        };
        let face = bundled(400);
        let measured = layout_line(&face, parse_style("Regular"), 300.0, "BEBA", 0.0).width;
        let (first, last, _) = ink_columns(&b);
        let ink = (last - first) as f32;
        assert!(
            (ink - measured).abs() < measured * 0.08,
            "ink {ink} vs measured {measured}"
        );
    }

    #[test]
    fn letters_should_not_overlap_at_zero_tracking() {
        let face = bundled(400);
        let line = layout_line(&face, parse_style("Regular"), 300.0, "BEBA", 0.0);
        let boxes: Vec<(f32, f32)> = line
            .glyphs
            .iter()
            .map(|g| {
                let o = face
                    .glyphs
                    .outline_glyph(g.id.with_scale_and_position(face.scale(300.0), point(g.x, 0.0)))
                    .unwrap();
                (o.px_bounds().min.x, o.px_bounds().max.x)
            })
            .collect();
        for pair in boxes.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "{pair:?}");
        }
    }
}
