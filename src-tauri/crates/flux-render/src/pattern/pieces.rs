use super::*;

pub(super) const BARS: [[u8; 3]; 8] = [
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
pub(super) fn solid(card: &TestCard) -> Option<[u8; 3]> {
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
pub(super) enum Piece {
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

pub(super) fn render(
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
pub(super) fn grid(c: &mut Canvas, p: &Palette, (cw, ch): (u32, u32)) {
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
pub(super) fn numbers(
    c: &mut Canvas,
    p: &Palette,
    card: &TestCard,
    (cw, ch): (u32, u32),
    columns: u32,
) {
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
pub(super) fn centre_lines(c: &mut Canvas, colour: [u8; 3], thick: u32) {
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

pub(super) fn circle(
    c: &mut Canvas,
    colour: [u8; 3],
    (cx, cy): (f32, f32),
    radius: f32,
    thick: f32,
) {
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
pub(super) fn diagonals(c: &mut Canvas, colour: [u8; 3], thick: f32) {
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
pub(super) fn stack(
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
