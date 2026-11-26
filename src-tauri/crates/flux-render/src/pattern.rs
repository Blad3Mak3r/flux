//! The test card of the output, one for each slice, drawn in the slice's own pixels so that it can
//! line up with the physical LED cabinets behind it: a checker of cabinets, one pixel lines on the
//! boundaries (the first pixel of each cabinet), the cabinet ids, a circle and the diagonals that
//! show a stretch, the slice's name and resolution and cabinet count in the middle, colour bars
//! and a grey ramp, and a moving part (a 2 pixel marker on a slim strip, a clock and a binary frame counter) that
//! shows a frozen or stuttering output. Or one flat colour, for dead pixel checks.
//!
//! The pattern is made once per size and settings ([`layout`]); each refresh only copies it and
//! draws the moving part ([`SliceCard::frame`]). Nothing here is random or reads the clock.

use std::sync::Arc;

use flux_project::mapping::{CardMode, Numbering, TestCard};
use flux_text::{Align, BUNDLED_FAMILY, Block};

use crate::palette::{self, Palette};

/// Seconds the marker takes to cross its strip.
pub const SWEEP_SECONDS: f32 = 3.0;

/// Frames a second the counter counts.
const COUNTER_RATE: f32 = 60.0;

/// The logo's picture, decoded: straight RGBA, `width` by `height`. It is the same logo as long
/// as its `id` (the project's asset) and its pixels (the same allocation) are.
#[derive(Debug, Clone)]
pub struct Logo {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl PartialEq for Logo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.rgba, &other.rgba)
    }
}

/// A slice of the mapping as the card marks it: its name, `[x, y, width, height]` of the picture
/// (each 0 to 1), its settings and the logo they name, if it could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct CardSlice {
    pub name: String,
    pub input: [f32; 4],
    pub card: TestCard,
    pub logo: Option<Logo>,
}

/// The card of one slice, as pixels of the composition.
pub struct SliceCard {
    /// `[x, y, width, height]` in composition pixels.
    pub rect: [u32; 4],
    /// RGBA, `rect`'s width by its height, without the moving part.
    pub fixed: Vec<u8>,
    motion: Option<Motion>,
}

impl SliceCard {
    /// The card with its moving part at `seconds` into a minute and `clock` (`HH:MM:SS`) in `out`;
    /// `false` when it does not move (`out` is left alone, use [`Self::fixed`]).
    pub fn frame(&self, out: &mut Vec<u8>, seconds: f32, clock: &str) -> bool {
        let Some(motion) = &self.motion else {
            return false;
        };
        out.clear();
        out.extend_from_slice(&self.fixed);
        motion.draw(out, self.rect[2], self.rect[3], seconds, clock);
        true
    }
}

/// The pixels a slice's rectangle covers in a composition: whole pixels, at least one each way.
pub fn pixel_rect(input: [f32; 4], composition: (u32, u32)) -> [u32; 4] {
    let edge = |from: f32, side: f32, of: u32| {
        let a = (from * of as f32).round().clamp(0.0, of as f32) as u32;
        let b = ((from + side) * of as f32).round().clamp(0.0, of as f32) as u32;
        (a.min(of.saturating_sub(1)), b.max(a + 1).min(of))
    };
    let (x0, x1) = edge(input[0], input[2], composition.0);
    let (y0, y1) = edge(input[1], input[3], composition.1);
    [x0, y0, x1 - x0, y1 - y0]
}

/// The cards of `slices` in a composition of `composition` pixels; `caption` (what the screen is
/// called and its size) is written under the cabinet count.
pub fn layout(slices: &[CardSlice], composition: (u32, u32), caption: &str) -> Vec<SliceCard> {
    slices
        .iter()
        .enumerate()
        .map(|(index, slice)| {
            let rect = pixel_rect(slice.input, composition);
            let (fixed, motion) = render(slice, index, [rect[2], rect[3]], caption);
            SliceCard {
                rect,
                fixed,
                motion,
            }
        })
        .collect()
}

/// `cards` laid on a picture `width` pixels across (a box average, so a pixel line is not lost),
/// as `[width, height]` and RGBA: opaque where a card is, clear elsewhere.
pub fn preview(cards: &[SliceCard], composition: (u32, u32), width: u32) -> (u32, u32, Vec<u8>) {
    let width = width.clamp(1, composition.0);
    let height = ((composition.1 as u64 * width as u64) / composition.0 as u64).max(1) as u32;
    let mut sums = vec![[0u32; 4]; (width * height) as usize];
    for card in cards {
        let [x0, y0, w, h] = card.rect;
        // The moving part as it is at zero, so the clock has digits.
        let mut moving = Vec::new();
        let pixels = if card.frame(&mut moving, 0.0, "00:00:00") {
            &moving
        } else {
            &card.fixed
        };
        for y in 0..h {
            let dy = (u64::from(y0 + y) * u64::from(height) / u64::from(composition.1)) as u32;
            for x in 0..w {
                let dx = (u64::from(x0 + x) * u64::from(width) / u64::from(composition.0)) as u32;
                let at = ((y * w + x) * 4) as usize;
                let sum = &mut sums[(dy * width + dx) as usize];
                for (sum, byte) in sum.iter_mut().zip(&pixels[at..at + 3]) {
                    *sum += u32::from(*byte);
                }
                sum[3] += 1;
            }
        }
    }
    let rgba = sums
        .iter()
        .flat_map(|s| {
            let n = s[3].max(1);
            [
                (s[0] / n) as u8,
                (s[1] / n) as u8,
                (s[2] / n) as u8,
                if s[3] > 0 { 255 } else { 0 },
            ]
        })
        .collect();
    (width, height, rgba)
}

/// The letters of column `n` from 0: A to Z, then AA.
fn letters(mut n: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// The id of the cabinet at `column`, `row` (from 0) in a grid `columns` wide.
pub fn cabinet_id(numbering: Numbering, column: u32, row: u32, columns: u32) -> String {
    match numbering {
        Numbering::Alpha => format!("{}{}", letters(column), row + 1),
        Numbering::Dotted => format!("{}.{}", column + 1, row + 1),
        Numbering::Sequence => format!("{:02}", row * columns + column + 1),
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

struct Canvas {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl Canvas {
    fn blend(&mut self, x: i64, y: i64, colour: [u8; 3], amount: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let at = ((y as u32 * self.width + x as u32) * 4) as usize;
        for (c, paint) in self.rgba[at..at + 3].iter_mut().zip(colour) {
            *c = (f32::from(*c) + (f32::from(paint) - f32::from(*c)) * amount).round() as u8;
        }
    }

    fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, colour: [u8; 3], amount: f32) {
        for row in y.max(0)..(y + h).min(i64::from(self.height)) {
            for column in x.max(0)..(x + w).min(i64::from(self.width)) {
                self.blend(column, row, colour, amount);
            }
        }
    }

    /// A rectangle with rounded corners of `radius`, the corners smoothed.
    fn rounded(
        &mut self,
        (x, y, w, h): (i64, i64, i64, i64),
        radius: f32,
        colour: [u8; 3],
        amount: f32,
    ) {
        let r = radius.min(w as f32 / 2.0).min(h as f32 / 2.0);
        for row in 0..h {
            for column in 0..w {
                let (fx, fy) = (column as f32 + 0.5, row as f32 + 0.5);
                let dx = (r - fx).max(fx - (w as f32 - r)).max(0.0);
                let dy = (r - fy).max(fy - (h as f32 - r)).max(0.0);
                let cover = (r - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.blend(x + column, y + row, colour, amount * cover);
                }
            }
        }
    }

    /// A disc of `radius` about `(cx, cy)`, its edge smoothed.
    fn disc(&mut self, (cx, cy): (f32, f32), radius: f32, colour: [u8; 3]) {
        let reach = radius.ceil() as i64 + 1;
        for y in cy as i64 - reach..=cy as i64 + reach {
            for x in cx as i64 - reach..=cx as i64 + reach {
                let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
                let cover = (radius - d + 0.5).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.blend(x, y, colour, cover);
                }
            }
        }
    }

    /// `logo` fitted in the `side` pixel square at `(x, y)`, keeping its shape, each pixel the
    /// average of the source pixels it covers.
    fn logo(&mut self, logo: &Logo, (x, y, side): (i64, i64, u32)) {
        if logo.width == 0 || logo.height == 0 {
            return;
        }
        let scale = side as f32 / logo.width.max(logo.height) as f32;
        let dw = ((logo.width as f32 * scale).round() as u32).max(1);
        let dh = ((logo.height as f32 * scale).round() as u32).max(1);
        let (x, y) = (
            x + i64::from(side - dw.min(side)) / 2,
            y + i64::from(side - dh.min(side)) / 2,
        );
        for oy in 0..dh {
            let (y0, y1) = span(oy, dh, logo.height);
            for ox in 0..dw {
                let (x0, x1) = span(ox, dw, logo.width);
                let mut sum = [0u64; 4];
                for sy in y0..y1 {
                    for sx in x0..x1 {
                        let at = ((sy * logo.width + sx) * 4) as usize;
                        let alpha = u64::from(logo.rgba[at + 3]);
                        for (s, v) in sum.iter_mut().zip(&logo.rgba[at..at + 3]) {
                            *s += u64::from(*v) * alpha;
                        }
                        sum[3] += alpha;
                    }
                }
                if sum[3] > 0 {
                    let count = u64::from((x1 - x0) * (y1 - y0));
                    let colour = [0, 1, 2].map(|i| (sum[i] / sum[3]) as u8);
                    let alpha = sum[3] as f32 / (count * 255) as f32;
                    self.blend(x + i64::from(ox), y + i64::from(oy), colour, alpha);
                }
            }
        }
    }

    fn image(&mut self, image: &Image, x: i64, y: i64) {
        for row in 0..image.height {
            for column in 0..image.width {
                let at = ((row * image.width + column) * 4) as usize;
                let alpha = f32::from(image.rgba[at + 3]) / 255.0;
                if alpha > 0.0 {
                    let colour = [image.rgba[at], image.rgba[at + 1], image.rgba[at + 2]];
                    self.blend(x + i64::from(column), y + i64::from(row), colour, alpha);
                }
            }
        }
    }
}

/// The source pixels `from..to` that output pixel `at` of `of` covers, at least one.
fn span(at: u32, of: u32, source: u32) -> (u32, u32) {
    let from = (u64::from(at) * u64::from(source) / u64::from(of)) as u32;
    let to = (u64::from(at + 1) * u64::from(source) / u64::from(of)) as u32;
    (from.min(source - 1), to.max(from + 1).min(source))
}

/// Text as a picture: straight RGBA, cut to its glyphs across and a whole line high.
struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

fn block(text: &str, size: f32, bold: bool, colour: [u8; 3], centre: [f32; 2]) -> Block<'_> {
    Block {
        text,
        family: BUNDLED_FAMILY,
        style: if bold { "Bold" } else { "Regular" },
        size,
        colour,
        opacity: 1.0,
        align: Align::Center,
        centre,
        rotation: 0.0,
        tracking: if size >= 24.0 { 0.12 } else { 0.07 },
        leading: 0.0,
    }
}

/// `text` drawn at `size` pixels, at most `max` wide.
fn text(text: &str, size: f32, bold: bool, colour: [u8; 3], max: u32) -> Image {
    let height = (size * 1.45).ceil() as u32;
    let font = flux_text::font(BUNDLED_FAMILY, if bold { "Bold" } else { "Regular" });
    let wide = flux_text::rasterise_with(
        &font,
        &block(
            text,
            size,
            bold,
            colour,
            [max as f32 / 2.0, height as f32 / 2.0],
        ),
        max,
        height,
    );
    let lit = |x: u32| (0..height).any(|y| wide[((y * max + x) * 4 + 3) as usize] > 0);
    let Some(first) = (0..max).find(|&x| lit(x)) else {
        return Image {
            width: 1,
            height,
            rgba: vec![0; (height * 4) as usize],
        };
    };
    let last = (0..max).rev().find(|&x| lit(x)).unwrap_or(first);
    let width = last - first + 1;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let from = ((y * max + first) * 4) as usize;
        rgba.extend_from_slice(&wide[from..from + (width * 4) as usize]);
    }
    Image {
        width,
        height,
        rgba,
    }
}

const BARS: [[u8; 3]; 8] = [
    [235, 235, 235],
    [235, 235, 16],
    [16, 235, 235],
    [16, 235, 16],
    [235, 16, 235],
    [235, 16, 16],
    [16, 16, 235],
    [16, 16, 16],
];

/// The fill of a mode that is a flat colour.
fn solid(card: &TestCard) -> Option<[u8; 3]> {
    Some(match card.mode {
        CardMode::Pattern => return None,
        CardMode::Red => [255, 0, 0],
        CardMode::Green => [0, 255, 0],
        CardMode::Blue => [0, 0, 255],
        CardMode::White => [255, 255, 255],
        CardMode::Black => [0, 0, 0],
        CardMode::Custom => card.custom_rgb(),
    })
}

/// One piece of the stack in the middle of the card.
enum Piece {
    Logo { diameter: u32 },
    Label(Vec<Image>),
    Clock { height: u32 },
    Bars { width: u32, height: u32 },
}

impl Piece {
    fn height(&self, gap: u32) -> u32 {
        match self {
            Self::Logo { diameter } => *diameter + gap / 2,
            Self::Label(lines) => lines.iter().map(|l| l.height).sum::<u32>() + gap * 2,
            Self::Clock { height, .. } => *height + gap,
            Self::Bars { height, .. } => *height,
        }
    }
}

fn render(
    slice: &CardSlice,
    index: usize,
    size: [u32; 2],
    caption: &str,
) -> (Vec<u8>, Option<Motion>) {
    let [w, h] = size;
    let card = &slice.card;
    if let Some(colour) = solid(card) {
        let rgba = [colour[0], colour[1], colour[2], 255].repeat((w * h) as usize);
        return (rgba, None);
    }
    let base = card
        .base_rgb()
        .unwrap_or_else(|| palette::default_base(index));
    let p = palette::palette(base, card.scheme);
    let (cw, ch) = (card.cabinet_width.max(1), card.cabinet_height.max(1));
    let mut c = Canvas {
        rgba: vec![255; (w * h * 4) as usize],
        width: w,
        height: h,
    };
    // The checker of cabinets.
    for y in 0..h {
        for x in 0..w {
            let tone = p.cabinet[((x / cw + y / ch) % 2) as usize];
            let at = ((y * w + x) * 4) as usize;
            c.rgba[at..at + 3].copy_from_slice(&tone);
        }
    }
    let layers = card.layers;
    let (columns, rows) = (w.div_ceil(cw), h.div_ceil(ch));
    if layers.grid {
        grid(&mut c, &p, (cw, ch));
    }
    // An orientation mark in the top left corner: a mirrored or turned wall shows at once.
    let mark = (w.min(h) as f32 * 0.025).clamp(8.0, 20.0) as i64;
    for y in 0..mark {
        for x in 0..mark - y {
            c.blend(x, y, p.accent, 1.0);
        }
    }
    let m = w.min(h) as f32;
    let thick = card.guide_width as f32;
    if layers.circle {
        circle(
            &mut c,
            p.accent,
            (w as f32 / 2.0, h as f32 / 2.0),
            m / 2.0 - 3.0 - thick,
            thick,
        );
    }
    if layers.diagonals {
        diagonals(&mut c, p.accent, thick);
    }
    if layers.crosshair {
        centre_lines(&mut c, p.accent, card.guide_width);
    }
    // The ids last, so no guide runs through one.
    if layers.numbers {
        numbers(&mut c, &p, card, (cw, ch), columns);
    }
    let motion = stack(&mut c, &p, slice, (cw, ch), (columns, rows), caption);
    (c.rgba, motion)
}

/// Lines on the first pixel of each cabinet, the outline of the slice and a mark at every corner.
fn grid(c: &mut Canvas, p: &Palette, (cw, ch): (u32, u32)) {
    let (w, h) = (i64::from(c.width), i64::from(c.height));
    let (cw, ch) = (i64::from(cw), i64::from(ch));
    for x in (cw..w).step_by(cw as usize) {
        c.rect(x, 0, 1, h, p.line, 1.0);
    }
    for y in (ch..h).step_by(ch as usize) {
        c.rect(0, y, w, 1, p.line, 1.0);
    }
    c.rect(0, 0, w, 2, p.frame, 1.0);
    c.rect(0, h - 2, w, 2, p.frame, 1.0);
    c.rect(0, 0, 2, h, p.frame, 1.0);
    c.rect(w - 2, 0, 2, h, p.frame, 1.0);
    for y in (ch..h).step_by(ch as usize) {
        for x in (cw..w).step_by(cw as usize) {
            c.rect(x - 4, y, 9, 1, p.frame, 1.0);
            c.rect(x, y - 4, 1, 9, p.frame, 1.0);
        }
    }
}

/// The id of each cabinet in its top left corner, left out where it would not be readable.
fn numbers(c: &mut Canvas, p: &Palette, card: &TestCard, (cw, ch): (u32, u32), columns: u32) {
    let (w, h) = (c.width, c.height);
    if cw < 44 || ch < 36 {
        return;
    }
    let size = (cw.min(ch) as f32 * 0.17).clamp(9.0, 22.0);
    let inset = (size * 0.5).max(4.0) as i64 + 1;
    // One colour for every cabinet, readable on both tones of the checker.
    let ink = palette::readable_over(&p.cabinet, p.hue, p.chroma * 0.3);
    for row in 0..h.div_ceil(ch) {
        for column in 0..columns {
            let (x0, y0) = (column * cw, row * ch);
            if w - x0 < 44 || h - y0 < 36 {
                continue;
            }
            let id = cabinet_id(card.numbering, column, row, columns);
            let image = text(&id, size, true, ink, cw - 4);
            let (x, y) = (i64::from(x0) + inset, i64::from(y0) + inset - 2);
            // A backing in the cabinet's own tone, so a guide that runs under the id is hidden.
            let tone = p.cabinet[((column + row) % 2) as usize];
            let pad = 2;
            c.rounded(
                (
                    x - pad,
                    y + i64::from(image.height) / 6,
                    i64::from(image.width) + pad * 2,
                    i64::from(image.height) * 2 / 3 + pad,
                ),
                3.0,
                tone,
                0.94,
            );
            c.image(&image, x, y);
        }
    }
}

/// Dashed lines through the middle, `thick` pixels wide, the dashes mirrored about the centre.
fn centre_lines(c: &mut Canvas, colour: [u8; 3], thick: u32) {
    let (w, h) = (i64::from(c.width), i64::from(c.height));
    let t = i64::from(thick);
    let (on, off) = (t * 6, t * 4);
    let dashed = |from: i64, to: i64, run: &mut dyn FnMut(i64, i64)| {
        // From the middle outwards both ways, so the pattern is the same on each side.
        let mid = (from + to) / 2;
        let mut at = 0;
        while mid + at < to || mid - at > from {
            run(mid + at, (mid + at + on).min(to));
            run((mid - at - on).max(from), mid - at);
            at += on + off;
        }
    };
    let x0 = (w - t) / 2;
    let y0 = (h - t) / 2;
    dashed(0, h, &mut |a, b| c.rect(x0, a, t, b - a, colour, 0.95));
    dashed(0, w, &mut |a, b| c.rect(a, y0, b - a, t, colour, 0.95));
}

fn circle(c: &mut Canvas, colour: [u8; 3], (cx, cy): (f32, f32), radius: f32, thick: f32) {
    if radius < 4.0 {
        return;
    }
    let reach = thick / 2.0 + 1.0;
    for y in 0..c.height {
        let dy = y as f32 + 0.5 - cy;
        let outer = ((radius + reach).powi(2) - dy * dy).max(0.0).sqrt();
        let inner = ((radius - reach).powi(2) - dy * dy).max(0.0).sqrt();
        if outer == 0.0 {
            continue;
        }
        let spans = [(cx - outer, cx - inner), (cx + inner, cx + outer)];
        for (from, to) in spans {
            for x in from.floor().max(0.0) as u32..(to.ceil().max(0.0) as u32).min(c.width) {
                let dx = x as f32 + 0.5 - cx;
                let cover = (thick / 2.0 + 0.5 - ((dx * dx + dy * dy).sqrt() - radius).abs())
                    .clamp(0.0, 1.0);
                c.blend(i64::from(x), i64::from(y), colour, cover * 0.9);
            }
        }
    }
}

/// Corner to corner, `thick` pixels wide.
fn diagonals(c: &mut Canvas, colour: [u8; 3], thick: f32) {
    let (w, h) = (c.width as f32, c.height as f32);
    let norm = w.hypot(h);
    for y in 0..c.height {
        for sign in [1.0_f32, -1.0] {
            let fy = y as f32 + 0.5;
            // The column the line crosses this row at, and a few pixels either side.
            let at = if sign > 0.0 {
                fy * w / h
            } else {
                w - fy * w / h
            };
            let span = ((thick + 1.0) * norm / h).ceil() + 1.0;
            for x in (at - span).floor().max(0.0) as u32
                ..((at + span).ceil().max(0.0) as u32).min(c.width)
            {
                let fx = x as f32 + 0.5;
                let line = if sign > 0.0 {
                    h * fx - w * fy
                } else {
                    h * (w - fx) - w * fy
                };
                let cover = (thick / 2.0 + 0.5 - line.abs() / norm).clamp(0.0, 1.0);
                if cover > 0.0 {
                    c.blend(i64::from(x), i64::from(y), colour, cover * 0.85);
                }
            }
        }
    }
}

/// Lays the label, the clock and the bars one under the other about the middle, dropping the
/// bars and then the clock when the slice is too short for all of them; the clock is the moving
/// part's.
fn stack(
    c: &mut Canvas,
    p: &Palette,
    slice: &CardSlice,
    (cw, ch): (u32, u32),
    (columns, rows): (u32, u32),
    caption: &str,
) -> Option<Motion> {
    let layers = slice.card.layers;
    let (w, h) = (c.width, c.height);
    let m = w.min(h) as f32;
    let gap = (m * 0.03).max(6.0) as u32;
    let mut name_size = (m * 0.085).clamp(13.0, 110.0);
    let mut label = None;
    if layers.label {
        let cut = if w % cw != 0 || h % ch != 0 {
            " · edge cut"
        } else {
            ""
        };
        let card = &slice.card;
        let lines = [
            (
                if card.show_name {
                    slice.name.clone()
                } else {
                    String::new()
                },
                1.0,
                true,
                p.ink,
            ),
            (
                if card.show_resolution {
                    format!("{w} × {h}")
                } else {
                    String::new()
                },
                0.78,
                true,
                p.accent,
            ),
            (
                format!("CAB {columns} × {rows} · {cw} × {ch} px{cut}"),
                0.30,
                false,
                p.muted,
            ),
            (caption.to_owned(), 0.30, false, p.muted),
        ];
        // Shrunk until it fits across the slice and takes at most a little over half its height.
        for _ in 0..3 {
            let images: Vec<Image> = lines
                .iter()
                .filter(|(t, ..)| !t.is_empty())
                .map(|(t, k, bold, colour)| text(t, (name_size * k).max(10.0), *bold, *colour, w))
                .collect();
            let across = images.iter().map(|i| i.width).max().unwrap_or(0) as f32;
            let down = images.iter().map(|i| i.height).sum::<u32>() as f32;
            let fit =
                ((w as f32 * 0.84) / (across + name_size)).min((h as f32 * 0.52) / down.max(1.0));
            if fit >= 1.0 {
                label = Some(images);
                break;
            }
            name_size *= fit * 0.98;
        }
        if name_size < 9.0 {
            label = None;
        }
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let card = &slice.card;
    if card.show_logo && slice.logo.is_some() {
        let wanted = (m * card.logo_size as f32 / 100.0).min(h as f32 * 0.3) as u32;
        if wanted >= 24 {
            pieces.push(Piece::Logo { diameter: wanted });
        }
    }
    if let Some(images) = label {
        pieces.push(Piece::Label(images));
    }
    let digit = (name_size * 0.36).max(11.0);
    if layers.motion && w >= 280 && h >= 180 {
        pieces.push(Piece::Clock {
            height: (digit * 1.45) as u32 + (m * 0.03).clamp(6.0, 16.0) as u32 + 22,
        });
    }
    if layers.bars && w >= 260 && h >= 200 {
        pieces.push(Piece::Bars {
            width: ((w as f32 * 0.46) as u32).min(640),
            height: (h as f32 * 0.06).clamp(12.0, 56.0) as u32,
        });
    }
    let total = |pieces: &[Piece]| {
        pieces.iter().map(|q| q.height(gap)).sum::<u32>()
            + gap * pieces.len().saturating_sub(1) as u32
    };
    while total(&pieces) as f32 > h as f32 * 0.86 && pieces.len() > 1 {
        // Bars first, then the clock, last in the stack.
        pieces.pop();
    }
    // The stack is centred on the label's place, kept inside the slice.
    let room = h.saturating_sub(total(&pieces));
    let mut y = (i64::from(h) * i64::from(slice.card.label_y) / 100
        - i64::from(total(&pieces)) / 2)
        .clamp(0, i64::from(room));
    let mut motion = None;
    for piece in &pieces {
        match piece {
            Piece::Logo { diameter } => {
                if let Some(logo) = &slice.logo {
                    let r = *diameter as f32 / 2.0;
                    let centre = (w as f32 / 2.0, y as f32 + r);
                    c.disc((centre.0, centre.1), r, p.accent);
                    c.disc((centre.0, centre.1), r - 2.0, card.logo_back_rgb());
                    let side = (*diameter as f32 * 0.7) as u32;
                    let inset = i64::from((diameter - side) / 2);
                    let left = i64::from(w.saturating_sub(*diameter) / 2) + inset;
                    c.logo(logo, (left, y + inset, side));
                }
                y += i64::from(piece.height(gap)) - i64::from(gap);
            }
            Piece::Label(images) => {
                let across = images.iter().map(|i| i.width).max().unwrap_or(0);
                let pad = (name_size * 0.5) as u32;
                let (pw, ph) = (across + pad * 2, piece.height(gap) + pad);
                let x = i64::from(w.saturating_sub(pw) / 2);
                if slice.card.label_plate {
                    c.rounded(
                        (x, y, i64::from(pw), i64::from(ph)),
                        name_size * 0.35,
                        p.plate,
                        0.93,
                    );
                }
                let mut line_y = y + i64::from(pad) / 2 + i64::from(gap) / 2;
                for image in images {
                    c.image(image, x + i64::from((pw - image.width) / 2), line_y);
                    line_y += i64::from(image.height);
                    if std::ptr::eq(image, &images[1.min(images.len() - 1)]) {
                        line_y += i64::from(gap) / 2;
                    }
                }
                y += i64::from(ph);
            }
            Piece::Clock { height } => {
                let m2 = Motion::build(c, p, digit, (w, y, *height));
                y += i64::from(*height);
                motion = Some(m2);
            }
            Piece::Bars { width, height } => {
                let x = i64::from((w - width) / 2);
                let (colour_h, ramp_h) = (
                    i64::from(height * 2 / 3),
                    i64::from(height - height * 2 / 3),
                );
                for i in 0..*width {
                    let along = i as f32 / (*width - 1).max(1) as f32;
                    let bar = BARS[((along * BARS.len() as f32) as usize).min(BARS.len() - 1)];
                    let grey = (along * 255.0).round() as u8;
                    c.rect(x + i64::from(i), y, 1, colour_h, bar, 1.0);
                    c.rect(
                        x + i64::from(i),
                        y + colour_h,
                        1,
                        ramp_h,
                        [grey, grey, grey],
                        1.0,
                    );
                }
                y += i64::from(*height);
            }
        }
        y += i64::from(gap);
    }
    motion
}

// ---------------------------------------------------------------------------------------------
// The moving part

/// What changes on every refresh: the marker of the strip, the clock's digits and the counter's bits.
struct Motion {
    /// `0` to `9`, then `:`, each in a cell of its own, so the clock does not wobble.
    sprites: Vec<Image>,
    clock_at: (i64, i64),
    bits_at: (i64, i64),
    bit: i64,
    on: [u8; 3],
    off: [u8; 3],
    /// `[x, y, width]` of the slim strip under the counter, 4 pixels high.
    strip: [i64; 3],
    marker: [u8; 3],
}

impl Motion {
    /// Draws the empty plate of the clock and the counter into `c`, in the band `height` high at
    /// `y`, and measures where they go.
    fn build(c: &mut Canvas, p: &Palette, size: f32, (w, y, height): (u32, i64, u32)) -> Self {
        let line = (size * 1.45).ceil() as u32;
        let digit_w = text("8", size, true, p.ink, 64).width + (size * 0.14) as u32 + 1;
        let colon_w = digit_w * 2 / 3;
        let cell = |ch: char| {
            let width = if ch == ':' { colon_w } else { digit_w };
            let font = flux_text::font(BUNDLED_FAMILY, "Bold");
            let s = ch.to_string();
            let rgba = flux_text::rasterise_with(
                &font,
                &block(
                    &s,
                    size,
                    true,
                    p.ink,
                    [width as f32 / 2.0, line as f32 / 2.0],
                ),
                width,
                line,
            );
            Image {
                width,
                height: line,
                rgba,
            }
        };
        let sprites: Vec<Image> = "0123456789:".chars().map(cell).collect();
        let clock_w = digit_w * 8 + colon_w * 3;
        let bit = ((w.min(c.height) as f32) * 0.03).clamp(6.0, 16.0) as i64;
        let bits_w = bit * 8 + 4 * 7 / 2 * 2;
        let across = clock_w.max(bits_w as u32);
        let pad = (size * 0.5) as u32;
        let (pw, ph) = (across + pad * 2, height - 6);
        let x = i64::from((w - pw) / 2);
        c.rounded(
            (x, y, i64::from(pw), i64::from(ph)),
            size * 0.5,
            p.plate,
            0.93,
        );
        let clock_x = x + i64::from((pw - clock_w) / 2);
        // A slim strip along the foot of the plate, its track baked in; only the marker moves.
        let strip = [
            x + i64::from(pad),
            y + i64::from(ph) - 9,
            i64::from(pw - pad * 2),
        ];
        c.rect(
            strip[0],
            strip[1],
            strip[2],
            4,
            p.plate.map(|v| v.saturating_add(46)),
            1.0,
        );
        let marker = p.accent.map(|v| (u16::from(v) * 2 / 5 + 153) as u8);
        Self {
            sprites,
            clock_at: (clock_x, y + 3),
            bits_at: (
                x + i64::from((pw as i64 - bits_w) as u32 / 2),
                y + 3 + i64::from(line) + 2,
            ),
            bit,
            on: p.accent,
            off: p.plate.map(|v| v.saturating_add(46)),
            strip,
            marker,
        }
    }

    fn draw(&self, rgba: &mut Vec<u8>, w: u32, h: u32, seconds: f32, clock: &str) {
        let mut c = Canvas {
            rgba: std::mem::take(rgba),
            width: w,
            height: h,
        };
        // The marker: a 2 pixel line and a short trail behind it, kept inside the strip.
        let [sx, sy, sw] = self.strip;
        let trail = (sw / 8).max(6);
        let at = ((seconds / SWEEP_SECONDS).fract() * (sw + trail) as f32) as i64;
        for back in 0..trail + 2 {
            let x = at - back;
            if (0..sw).contains(&x) {
                let amount = if back < 2 {
                    1.0
                } else {
                    (1.0 - (back - 2) as f32 / trail as f32).powi(2) * 0.6
                };
                c.rect(sx + x, sy, 1, 4, self.marker, amount);
            }
        }
        let frame = (seconds * COUNTER_RATE) as u32;
        let text = format!("{clock}:{:02}", frame % 60);
        let (mut x, y) = self.clock_at;
        for ch in text.chars() {
            let at = match ch {
                '0'..='9' => ch as usize - '0' as usize,
                ':' => 10,
                _ => continue,
            };
            c.image(&self.sprites[at], x, y);
            x += i64::from(self.sprites[at].width);
        }
        let (bx, by) = self.bits_at;
        for bit in 0..8u32 {
            let on = (frame >> (7 - bit)) & 1 == 1;
            let colour = if on { self.on } else { self.off };
            c.rect(
                bx + i64::from(bit) * (self.bit + 4),
                by,
                self.bit,
                self.bit,
                colour,
                1.0,
            );
        }
        *rgba = c.rgba;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::mapping::{CardLayers, Scheme};

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * width + x) * 4) as usize;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    }

    fn slice(card: TestCard) -> CardSlice {
        CardSlice {
            name: "Slice 1".into(),
            input: [0.0, 0.0, 1.0, 1.0],
            card,
            logo: None,
        }
    }

    /// The card with only the layers named.
    fn only(grid: bool, numbers: bool) -> TestCard {
        TestCard {
            layers: CardLayers {
                grid,
                numbers,
                circle: false,
                diagonals: false,
                crosshair: false,
                label: false,
                bars: false,
                motion: false,
            },
            ..TestCard::default()
        }
    }

    fn made(card: TestCard, comp: (u32, u32)) -> SliceCard {
        layout(&[slice(card)], comp, "").remove(0)
    }

    #[test]
    fn the_grid_lines_should_fall_on_the_first_pixel_of_each_cabinet() {
        let (w, h) = (512, 384);
        let card = made(only(true, false), (w, h));
        let p = palette::palette(palette::default_base(0), Scheme::default());
        for x in [128, 256, 384] {
            assert_eq!(pixel(&card.fixed, w, x, 50), p.line, "column {x}");
            // One pixel wide: both neighbours are cabinet.
            assert_ne!(pixel(&card.fixed, w, x - 1, 50), p.line);
            assert_ne!(pixel(&card.fixed, w, x + 1, 50), p.line);
        }
        for y in [128, 256] {
            assert_eq!(pixel(&card.fixed, w, 60, y), p.line, "row {y}");
            assert_ne!(pixel(&card.fixed, w, 60, y + 1), p.line);
            assert_ne!(pixel(&card.fixed, w, 60, y - 1), p.line);
        }
        // The slice's outline is on its edge.
        assert_eq!(pixel(&card.fixed, w, w - 1, 70), p.frame);
        assert_eq!(pixel(&card.fixed, w, 70, 0), p.frame);
    }

    #[test]
    fn the_cabinet_size_should_move_the_lines() {
        let (w, h) = (256, 256);
        let card = made(
            TestCard {
                cabinet_width: 64,
                cabinet_height: 32,
                ..only(true, false)
            },
            (w, h),
        );
        let p = palette::palette(palette::default_base(0), Scheme::default());
        assert_eq!(pixel(&card.fixed, w, 64, 10), p.line);
        assert_eq!(pixel(&card.fixed, w, 140, 32), p.line);
        assert_ne!(pixel(&card.fixed, w, 100, 40), p.line);
    }

    #[test]
    fn the_checker_should_alternate_per_cabinet_and_the_last_cabinet_should_be_cut() {
        // 300 x 200 with 128 cabinets: a third column of 44 pixels and a second row of 72.
        let (w, h) = (300, 200);
        let card = made(only(false, false), (w, h));
        let p = palette::palette(palette::default_base(0), Scheme::default());
        assert_eq!(pixel(&card.fixed, w, 10, 10), p.cabinet[0]);
        assert_eq!(pixel(&card.fixed, w, 130, 10), p.cabinet[1]);
        assert_eq!(pixel(&card.fixed, w, 10, 130), p.cabinet[1]);
        assert_eq!(pixel(&card.fixed, w, 290, 190), p.cabinet[1]);
    }

    #[test]
    fn ids_should_follow_the_numbering() {
        assert_eq!(cabinet_id(Numbering::Alpha, 0, 0, 12), "A1");
        assert_eq!(cabinet_id(Numbering::Alpha, 1, 0, 12), "B1");
        assert_eq!(cabinet_id(Numbering::Alpha, 26, 2, 30), "AA3");
        assert_eq!(cabinet_id(Numbering::Dotted, 2, 4, 12), "3.5");
        assert_eq!(cabinet_id(Numbering::Sequence, 2, 1, 12), "15");
        assert_eq!(cabinet_id(Numbering::Sequence, 0, 0, 12), "01");
    }

    #[test]
    fn numbers_should_be_written_in_the_cabinets_that_are_big_enough_and_not_in_small_ones() {
        let (w, h) = (256, 128);
        let with = made(only(false, true), (w, h));
        let without = made(only(false, false), (w, h));
        // The top left of each of the two cabinets differs, the middle of one does not.
        let differs = |x0: u32| {
            (x0 + 4..x0 + 40).any(|x| {
                (4..30).any(|y| pixel(&with.fixed, w, x, y) != pixel(&without.fixed, w, x, y))
            })
        };
        assert!(differs(0) && differs(128));
        assert_eq!(
            pixel(&with.fixed, w, 100, 110),
            pixel(&without.fixed, w, 100, 110)
        );
        // Cabinets of 32 pixels are too small to read: no ids.
        let small = TestCard {
            cabinet_width: 32,
            cabinet_height: 32,
            ..only(false, true)
        };
        let small_without = TestCard {
            cabinet_width: 32,
            cabinet_height: 32,
            ..only(false, false)
        };
        assert_eq!(made(small, (w, h)).fixed, made(small_without, (w, h)).fixed);
    }

    #[test]
    fn a_solid_mode_should_fill_every_pixel() {
        let (w, h) = (96, 64);
        for (mode, colour) in [
            (CardMode::Red, [255, 0, 0]),
            (CardMode::Green, [0, 255, 0]),
            (CardMode::Blue, [0, 0, 255]),
            (CardMode::White, [255, 255, 255]),
            (CardMode::Black, [0, 0, 0]),
        ] {
            let card = made(
                TestCard {
                    mode,
                    ..TestCard::default()
                },
                (w, h),
            );
            assert!(
                card.fixed
                    .chunks(4)
                    .all(|px| px == [colour[0], colour[1], colour[2], 255])
            );
            assert!(
                !card.frame(&mut Vec::new(), 1.0, "00:00:00"),
                "no motion on a flat colour"
            );
        }
        let custom = TestCard {
            mode: CardMode::Custom,
            custom: "#102030".into(),
            ..TestCard::default()
        };
        assert_eq!(
            pixel(&made(custom, (w, h)).fixed, w, 5, 5),
            [0x10, 0x20, 0x30]
        );
    }

    #[test]
    fn the_card_should_be_deterministic_and_have_the_size_of_its_slice() {
        let slices = [CardSlice {
            name: "Wall".into(),
            input: [0.25, 0.5, 0.5, 0.25],
            card: TestCard::default(),
            logo: None,
        }];
        let a = layout(&slices, (1920, 1080), "Main  1920 x 1080");
        let b = layout(&slices, (1920, 1080), "Main  1920 x 1080");
        assert_eq!(a[0].rect, [480, 540, 960, 270]);
        assert_eq!(a[0].fixed.len(), 960 * 270 * 4);
        assert_eq!(a[0].fixed, b[0].fixed);
        let (mut one, mut two) = (Vec::new(), Vec::new());
        assert!(a[0].frame(&mut one, 1.25, "10:20:30"));
        assert!(b[0].frame(&mut two, 1.25, "10:20:30"));
        assert_eq!(one, two);
        // It moves: a quarter of a second later it is not the same picture.
        assert!(a[0].frame(&mut two, 1.5, "10:20:30"));
        assert_ne!(one, two);
    }

    #[test]
    fn the_label_should_be_drawn_in_the_middle_of_the_slice() {
        let (w, h) = (640, 384);
        let plain = made(only(false, false), (w, h));
        let labelled = made(
            TestCard {
                layers: CardLayers {
                    label: true,
                    ..only(false, false).layers
                },
                ..only(false, false)
            },
            (w, h),
        );
        let changed =
            |x: u32, y: u32| pixel(&plain.fixed, w, x, y) != pixel(&labelled.fixed, w, x, y);
        assert!(
            changed(w / 2, h / 2)
                || (w / 2 - 60..w / 2 + 60)
                    .any(|x| (h / 2 - 40..h / 2 + 40).any(|y| changed(x, y)))
        );
        assert!(!changed(5, 5) && !changed(w - 6, h - 6));
    }

    #[test]
    fn a_partial_slice_pixel_rect_should_snap_to_whole_pixels() {
        assert_eq!(
            pixel_rect([0.0, 0.0, 1.0, 1.0], (1920, 1080)),
            [0, 0, 1920, 1080]
        );
        assert_eq!(
            pixel_rect([0.3333, 0.0, 0.3333, 1.0], (1920, 1080)),
            [640, 0, 640, 1080]
        );
        let tiny = pixel_rect([0.5, 0.5, 0.0001, 0.0001], (100, 100));
        assert!(tiny[2] >= 1 && tiny[3] >= 1);
    }

    #[test]
    fn the_preview_should_be_clear_outside_the_cards_and_keep_their_lines() {
        let slices = [CardSlice {
            name: "A".into(),
            input: [0.0, 0.0, 0.5, 1.0],
            card: only(true, false),
            logo: None,
        }];
        let cards = layout(&slices, (512, 256), "");
        let (w, h, rgba) = preview(&cards, (512, 256), 256);
        assert_eq!((w, h), (256, 128));
        assert_eq!(rgba[((10 * w + 200) * 4 + 3) as usize], 0);
        assert_eq!(rgba[((10 * w + 20) * 4 + 3) as usize], 255);
    }

    /// A red square logo, 32 by 32.
    fn red_logo() -> Logo {
        Logo {
            id: "img".into(),
            width: 32,
            height: 32,
            rgba: [255u8, 0, 0, 255].repeat(32 * 32).into(),
        }
    }

    fn with_logo(show: bool) -> SliceCard {
        let card = TestCard {
            show_logo: show,
            logo_back: "#000000".into(),
            layers: CardLayers {
                label: false,
                bars: false,
                motion: false,
                ..CardLayers::default()
            },
            ..TestCard::default()
        };
        let slices = [CardSlice {
            logo: Some(red_logo()),
            ..slice(card)
        }];
        layout(&slices, (640, 480), "").remove(0)
    }

    #[test]
    fn the_logo_should_be_drawn_in_a_disc_when_it_is_shown() {
        let card = with_logo(true);
        let found = (0..480u32).any(|y| pixel(&card.fixed, 640, 320, y) == [255, 0, 0]);
        assert!(found);
    }

    #[test]
    fn the_logo_should_be_left_out_when_it_is_off() {
        let card = with_logo(false);
        let found = (0..480u32).any(|y| pixel(&card.fixed, 640, 320, y) == [255, 0, 0]);
        assert!(!found);
    }

    #[test]
    fn the_logo_should_sit_on_the_back_colour_of_its_disc() {
        let card = with_logo(true);
        let back = (0..480u32)
            .filter(|&y| pixel(&card.fixed, 640, 320, y) == [0, 0, 0])
            .count();
        assert!(back > 0);
    }

    #[test]
    fn the_moving_part_should_stay_in_the_stack_and_not_cross_the_whole_slice() {
        let card = made(TestCard::default(), (1280, 720));
        let (mut a, mut b) = (Vec::new(), Vec::new());
        assert!(card.frame(&mut a, 0.2, "10:20:30"));
        assert!(card.frame(&mut b, 1.4, "10:20:30"));
        let rows: Vec<u32> = (0..720u32)
            .filter(|&y| {
                let at = (y * 1280 * 4) as usize;
                let row = 1280 * 4;
                a[at..at + row] != b[at..at + row]
            })
            .collect();
        let (first, last) = (rows[0], rows[rows.len() - 1]);
        assert!(last - first < 160, "rows {first} to {last}");
    }

    #[test]
    fn the_ids_should_be_drawn_over_the_guides() {
        let layers = CardLayers {
            grid: false,
            circle: false,
            diagonals: true,
            crosshair: false,
            label: false,
            bars: false,
            motion: false,
            ..CardLayers::default()
        };
        let with = |numbers| TestCard {
            layers: CardLayers { numbers, ..layers },
            guide_width: 12,
            ..TestCard::default()
        };
        let (a, b) = (made(with(true), (512, 512)), made(with(false), (512, 512)));
        let differ = (0..60u32)
            .flat_map(|y| (0..90u32).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&a.fixed, 512, x, y) != pixel(&b.fixed, 512, x, y))
            .count();
        assert!(differ > 40, "{differ}");
    }
}
