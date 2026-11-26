//! Text as a picture: the fonts the system has (and one bundled, so a missing font never stops a
//! text from drawing), a block of lines laid out and rasterised on the CPU into straight RGBA over a
//! transparent background. Pure Rust: `fontdb` finds the fonts and `ab_glyph` draws the glyphs.
//!
//! Kerning comes from the font's legacy `kern` table; a font that only has GPOS pair kerning is
//! spaced by its advances alone (shortcut: upgrade to `rustybuzz` if that shows).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use ab_glyph::{Font, FontArc, GlyphId, PxScale, ScaleFont, point};
use fontdb::{Database, Family, Query, Stretch, Style, Weight};

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

struct Fonts {
    db: Database,
    /// Faces already opened, by family and style as asked; `None` is the bundled one.
    open: Mutex<HashMap<(String, String), FontArc>>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        Fonts {
            db,
            open: Mutex::default(),
        }
    })
}

/// Reads the system's fonts now, on a thread of its own, so the first text does not wait for them.
pub fn warm() {
    std::thread::spawn(|| {
        fonts();
    });
}

fn bundled(weight: u16) -> FontArc {
    static CACHE: OnceLock<[FontArc; 2]> = OnceLock::new();
    let both = CACHE.get_or_init(|| {
        [REGULAR, BOLD].map(|bytes| {
            FontArc::try_from_slice(bytes).expect("the bundled font is a valid font file")
        })
    });
    both[usize::from(weight >= 600)].clone()
}

/// The face for `family` in `style`: the closest one the machine has, else the bundled font. It
/// never fails.
pub fn font(family: &str, style: &str) -> FontArc {
    let (weight, slant) = parse_style(style);
    let fonts = fonts();
    let key = (family.to_lowercase(), style.to_lowercase());
    if let Some(found) = fonts
        .open
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
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
        .and_then(|id| {
            fonts.db.with_face_data(id, |data, index| {
                ab_glyph::FontVec::try_from_vec_and_index(data.to_vec(), index).ok()
            })?
        })
        .map_or_else(|| bundled(weight), FontArc::new);
    fonts
        .open
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, found.clone());
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

/// One line of glyphs: where each starts along the line, and how long the line is.
struct Line {
    glyphs: Vec<(GlyphId, f32)>,
    width: f32,
}

fn layout_line(font: &FontArc, scale: PxScale, line: &str, tracking: f32) -> Line {
    let scaled = font.as_scaled(scale);
    let mut glyphs = Vec::with_capacity(line.len());
    let mut pen = 0.0;
    let mut last: Option<GlyphId> = None;
    for ch in line.chars() {
        let id = font.glyph_id(ch);
        if let Some(last) = last {
            pen += scaled.kern(last, id);
        }
        glyphs.push((id, pen));
        pen += scaled.h_advance(id) + tracking;
        last = Some(id);
    }
    Line {
        glyphs,
        width: if line.is_empty() { 0.0 } else { pen - tracking },
    }
}

/// The coverage (0 to 255) of a block of lines: its width, its height and the bytes.
fn draw_block(font: &FontArc, block: &Block<'_>) -> (usize, usize, Vec<u8>) {
    let em = block.size.max(1.0);
    let scale = font
        .pt_to_px_scale(em)
        .unwrap_or_else(|| PxScale::from(em * 1.2));
    let scaled = font.as_scaled(scale);
    let tracking = block.tracking * em;
    let lines: Vec<Line> = block
        .text
        .split('\n')
        .map(|line| layout_line(font, scale, line, tracking))
        .collect();
    let widest = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    let pitch = (scaled.height() + scaled.line_gap() + block.leading * em).max(1.0);
    let width = ((widest.ceil() as usize) + 2 * PAD).clamp(1, MAX_SIDE);
    let height = (((pitch * (lines.len() - 1) as f32 + scaled.height()).ceil() as usize) + 2 * PAD)
        .clamp(1, MAX_SIDE);
    let mut cover = vec![0u8; width * height];
    for (n, line) in lines.iter().enumerate() {
        let indent = match block.align {
            Align::Left => 0.0,
            Align::Center => (widest - line.width) / 2.0,
            Align::Right => widest - line.width,
        };
        let baseline = PAD as f32 + scaled.ascent() + pitch * n as f32;
        for &(id, at) in &line.glyphs {
            let glyph =
                id.with_scale_and_position(scale, point(PAD as f32 + indent + at, baseline));
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            let (left, top) = (bounds.min.x as i64, bounds.min.y as i64);
            outline.draw(|x, y, c| {
                let (x, y) = (left + i64::from(x), top + i64::from(y));
                if (0..width as i64).contains(&x) && (0..height as i64).contains(&y) {
                    let at = y as usize * width + x as usize;
                    cover[at] = cover[at].saturating_add((c * 255.0).round() as u8);
                }
            });
        }
    }
    (width, height, cover)
}

/// The coverage at `(x, y)` of a bitmap, bilinear; what is outside is nothing.
fn sample(cover: &[u8], size: (usize, usize), x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: i64, dy: i64| -> f32 {
        let (px, py) = (x0 as i64 + dx, y0 as i64 + dy);
        if (0..size.0 as i64).contains(&px) && (0..size.1 as i64).contains(&py) {
            f32::from(cover[py as usize * size.0 + px as usize])
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
pub fn rasterise_with(font: &FontArc, block: &Block<'_>, width: u32, height: u32) -> Vec<u8> {
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
    let (bw, bh, cover) = draw_block(font, block);
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
            let level = sample(&cover, (bw, bh), sx, sy) * opacity;
            out[(y * w + x) * 4 + 3] = level.round().clamp(0.0, 255.0) as u8;
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
        assert!(font.glyph_id('A').0 != 0);
        assert!(families().iter().any(|f| f.name == BUNDLED_FAMILY));
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
}
