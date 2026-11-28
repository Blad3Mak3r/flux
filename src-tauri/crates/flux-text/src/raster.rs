use super::*;

/// The coverage (0 to 255) of a block of lines.
pub(super) struct Cover {
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) bytes: Vec<u8>,
    /// Premultiplied RGBA of the colour glyphs, drawn over `bytes`; empty when there are none.
    colour: Vec<u8>,
}

/// The blocks drawn last. Moving, turning and fading a block only resamples its coverage, so the
/// glyphs are drawn again only when the text, the face or the spacing change.
pub(super) static COVERS: Mutex<Vec<(u64, Arc<Cover>)>> = Mutex::new(Vec::new());
pub(super) const KEPT_COVERS: usize = 8;

pub(super) fn draw_block(face: &Arc<Face>, block: &Block<'_>) -> Arc<Cover> {
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

pub(super) fn draw_lines(face: &Arc<Face>, block: &Block<'_>) -> Cover {
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
pub(super) fn over(canvas: &mut [u8], size: (usize, usize), tile: &colour::Tile, ox: i64, oy: i64) {
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
pub(super) fn sample(cover: &[u8], size: (usize, usize), x: f32, y: f32) -> f32 {
    sample_channel(cover, size, 1, 0, x, y)
}

/// [`sample`] of channel `ch` of a bitmap that has `stride` channels.
pub(super) fn sample_channel(
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
