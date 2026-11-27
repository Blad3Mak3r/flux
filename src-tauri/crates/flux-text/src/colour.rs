//! Colour glyphs: the pictures a font keeps for its emoji. Bitmaps (`CBDT`/`CBLC` as Noto Color
//! Emoji, `sbix` as Apple Color Emoji, both PNG) and layered vector glyphs (`COLR` v0 and v1, as
//! Segoe UI Emoji) are turned into a [`Tile`]: premultiplied RGBA at the size of the block's em.
//!
//! Shortcut: of the COLR v1 blend modes only the Porter-Duff ones are exact (the separable blends
//! such as Multiply draw as source-over); upgrade when a font needs them.

use ab_glyph_rasterizer::{Point, Rasterizer};
use rustybuzz::ttf_parser::colr::{
    ClipBox, ColorStop, CompositeMode, GradientExtend, Paint, Painter,
};
use rustybuzz::ttf_parser::{
    Face, GlyphId, OutlineBuilder, RasterGlyphImage, RasterImageFormat, RgbaColor, Transform,
};

/// Most pixels of a side a bitmap glyph may have, so a hostile font cannot ask for gigabytes.
const MAX_BITMAP: u32 = 2048;
/// The room round the em box a COLR glyph may paint in, in ems.
const MARGIN: f32 = 0.25;

/// A drawn colour glyph: premultiplied RGBA, with its top left corner `x` pixels right of the glyph
/// origin and `y` pixels below the baseline.
pub(crate) struct Tile {
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// The colour picture of `glyph` at an em of `em` pixels, if the face has one.
pub(crate) fn tile(face: &Face<'_>, glyph: GlyphId, em: f32) -> Option<Tile> {
    if em.is_nan() || em < 1.0 {
        return None;
    }
    let ppem = em.round().clamp(1.0, f32::from(u16::MAX)) as u16;
    if let Some(image) = face.glyph_raster_image(glyph, ppem)
        && image.format == RasterImageFormat::PNG
    {
        return bitmap(&image, em);
    }
    colr(face, glyph, em)
}

// ---------------------------------------------------------------------------------------------
// Bitmaps

fn bitmap(image: &RasterGlyphImage<'_>, em: f32) -> Option<Tile> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(image.data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (sw, sh) = (info.width as usize, info.height as usize);
    if sw == 0 || sh == 0 || sw.max(sh) > MAX_BITMAP as usize {
        return None;
    }
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buffer).ok()?;
    let channels = frame.color_type.samples();
    let source: Vec<[f32; 4]> = buffer[..frame.buffer_size()]
        .chunks_exact(channels)
        .map(|px| {
            let (rgb, a) = match *px {
                [g] => ([g; 3], 255),
                [g, a] => ([g; 3], a),
                [r, g, b] => ([r, g, b], 255),
                [r, g, b, a] => ([r, g, b], a),
                _ => ([0; 3], 0),
            };
            let a = f32::from(a) / 255.0;
            [
                f32::from(rgb[0]) * a,
                f32::from(rgb[1]) * a,
                f32::from(rgb[2]) * a,
                a * 255.0,
            ]
        })
        .collect();
    if source.len() < sw * sh {
        return None;
    }
    // The strike is drawn for `pixels_per_em`; the block wants `em`.
    let k = em / f32::from(image.pixels_per_em.max(1));
    let side = MAX_BITMAP as usize;
    let dw = ((sw as f32 * k).round() as usize).clamp(1, side);
    let dh = ((sh as f32 * k).round() as usize).clamp(1, side);
    let (fx, fy) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
    let mut rgba = vec![0u8; dw * dh * 4];
    // Box filter: each pixel is the mean of the source pixels it covers, so a shrunk emoji is smooth.
    for y in 0..dh {
        let (y0, y1) = span(y, fy, sh);
        for x in 0..dw {
            let (x0, x1) = span(x, fx, sw);
            let mut sum = [0.0f32; 4];
            for sy in y0..y1 {
                for px in &source[sy * sw + x0..sy * sw + x1] {
                    (0..4).for_each(|c| sum[c] += px[c]);
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            for c in 0..4 {
                rgba[(y * dw + x) * 4 + c] = (sum[c] / n).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Some(Tile {
        x: (f32::from(image.x) * k).round() as i32,
        // `y` is the bottom of the picture above the baseline.
        y: -((f32::from(image.y) * k).round() as i32 + dh as i32),
        w: dw,
        h: dh,
        rgba,
    })
}

/// The source pixels `from..to` that destination pixel `at` covers, never empty.
fn span(at: usize, step: f32, max: usize) -> (usize, usize) {
    let from = ((at as f32 * step).floor() as usize).min(max - 1);
    let to = (((at + 1) as f32 * step).ceil() as usize).clamp(from + 1, max);
    (from, to)
}

// ---------------------------------------------------------------------------------------------
// COLR

/// `x' = a x + c y + e`, `y' = b x + d y + f`.
#[derive(Clone, Copy)]
struct Affine([f32; 6]);

impl Affine {
    const ID: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }

    /// The transform that applies `inner` first, then `self`.
    fn then_after(self, inner: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [ia, ib, ic, id, ie, iff] = inner.0;
        Self([
            a * ia + c * ib,
            b * ia + d * ib,
            a * ic + c * id,
            b * ic + d * id,
            a * ie + c * iff + e,
            b * ie + d * iff + f,
        ])
    }

    fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Some(Self([
            ia,
            ib,
            ic,
            id,
            -(ia * e + ic * f),
            -(ib * e + id * f),
        ]))
    }
}

#[derive(Clone, Copy)]
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Default)]
struct Path(Vec<Seg>);

impl OutlineBuilder for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::Move(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Seg::Line(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(Seg::Quad(x1, y1, x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(Seg::Cubic(x1, y1, x2, y2, x, y));
    }
    fn close(&mut self) {
        self.0.push(Seg::Close);
    }
}

type Pixel = [f32; 4];

struct Paintbox<'f> {
    face: &'f Face<'f>,
    w: usize,
    h: usize,
    /// Font units to canvas pixels.
    base: Affine,
    /// The transforms pushed by the glyph, in font units, each already joined with the ones below.
    transforms: Vec<Affine>,
    outline: Path,
    /// Coverage masks of the clips pushed, each already cut by the one below.
    clips: Vec<Vec<f32>>,
    /// Premultiplied pixels of the layers, the last being drawn on.
    layers: Vec<Vec<Pixel>>,
    modes: Vec<CompositeMode>,
}

impl Paintbox<'_> {
    fn device(&self) -> Affine {
        self.base
            .then_after(self.transforms.last().copied().unwrap_or(Affine::ID))
    }

    fn mask_of(&self, path: &Path) -> Vec<f32> {
        let m = self.device();
        let at = |x: f32, y: f32| {
            let (x, y) = m.apply(x, y);
            Point { x, y }
        };
        let mut r = Rasterizer::new(self.w, self.h);
        let (mut start, mut pen) = (at(0.0, 0.0), at(0.0, 0.0));
        for seg in &path.0 {
            match *seg {
                Seg::Move(x, y) => {
                    // An open contour fills as if it were closed.
                    r.draw_line(pen, start);
                    start = at(x, y);
                    pen = start;
                }
                Seg::Line(x, y) => {
                    let to = at(x, y);
                    r.draw_line(pen, to);
                    pen = to;
                }
                Seg::Quad(x1, y1, x, y) => {
                    let to = at(x, y);
                    r.draw_quad(pen, at(x1, y1), to);
                    pen = to;
                }
                Seg::Cubic(x1, y1, x2, y2, x, y) => {
                    let to = at(x, y);
                    r.draw_cubic(pen, at(x1, y1), at(x2, y2), to);
                    pen = to;
                }
                Seg::Close => {
                    r.draw_line(pen, start);
                    pen = start;
                }
            }
        }
        r.draw_line(pen, start);
        let mut mask = vec![0.0; self.w * self.h];
        r.for_each_pixel(|i, c| mask[i] = c.clamp(0.0, 1.0));
        mask
    }

    fn push_mask(&mut self, mut mask: Vec<f32>) {
        if let Some(below) = self.clips.last() {
            mask.iter_mut().zip(below).for_each(|(m, b)| *m *= b);
        }
        self.clips.push(mask);
    }

    /// The colour of `paint` at the pixel `(px, py)`, straight (not premultiplied).
    fn colour_at(paint: &Paint<'_>, inverse: Affine, px: f32, py: f32) -> Pixel {
        let (x, y) = inverse.apply(px + 0.5, py + 0.5);
        let (stops, extend, t) = match paint {
            Paint::Solid(c) => return rgba(*c),
            Paint::LinearGradient(g) => {
                let (dx, dy) = (g.x1 - g.x0, g.y1 - g.y0);
                // The isolines run along p0 p2; the gradient line is p0 p1 skewed to meet them.
                let (nx, ny) = (-(g.y2 - g.y0), g.x2 - g.x0);
                let den = dx * nx + dy * ny;
                let t = (den.abs() > 1e-9).then(|| ((x - g.x0) * nx + (y - g.y0) * ny) / den);
                (stops_of(g.stops(0, &[])), g.extend, t)
            }
            Paint::RadialGradient(g) => {
                let (cx, cy, dr) = (g.x1 - g.x0, g.y1 - g.y0, g.r1 - g.r0);
                let (ox, oy) = (x - g.x0, y - g.y0);
                let a = cx * cx + cy * cy - dr * dr;
                let b = ox * cx + oy * cy + g.r0 * dr;
                let c = ox * ox + oy * oy - g.r0 * g.r0;
                let valid = |s: f32| g.r0 + s * dr >= 0.0;
                let t = if a.abs() < 1e-9 {
                    (b.abs() > 1e-9)
                        .then(|| c / (2.0 * b))
                        .filter(|s| valid(*s))
                } else if b * b - a * c >= 0.0 {
                    let root = (b * b - a * c).sqrt();
                    let (s1, s2) = ((b + root) / a, (b - root) / a);
                    [s1.max(s2), s1.min(s2)].into_iter().find(|s| valid(*s))
                } else {
                    None
                };
                (stops_of(g.stops(0, &[])), g.extend, t)
            }
            Paint::SweepGradient(g) => {
                // Angles are stored in half turns; the glyph's y points up, so they run
                // counter-clockwise.
                let angle = (y - g.center_y)
                    .atan2(x - g.center_x)
                    .to_degrees()
                    .rem_euclid(360.0);
                let (start, end) = (g.start_angle * 180.0, g.end_angle * 180.0);
                let t = ((end - start).abs() > 1e-6)
                    .then(|| (angle - start.rem_euclid(360.0)) / (end - start));
                (stops_of(g.stops(0, &[])), g.extend, t)
            }
        };
        match t {
            Some(t) if !stops.is_empty() => gradient(&stops, extend, t),
            _ => [0.0; 4],
        }
    }

    fn paint_pixels(&mut self, paint: &Paint<'_>) {
        let Some(inverse) = self.device().inverse() else {
            return;
        };
        // A v0 layer is the stored outline; a v1 paint fills the clip it sits in.
        let stored;
        let mask: &[f32] = match self.clips.last() {
            Some(clip) => clip,
            None => {
                stored = self.mask_of(&self.outline);
                &stored
            }
        };
        let Some(layer) = self.layers.last_mut() else {
            return;
        };
        for (i, &cover) in mask.iter().enumerate() {
            if cover > 0.0 {
                let c = Self::colour_at(paint, inverse, (i % self.w) as f32, (i / self.w) as f32);
                let a = c[3] * cover;
                let src = [c[0] * a, c[1] * a, c[2] * a, a];
                layer[i] = composite(src, layer[i], CompositeMode::SourceOver);
            }
        }
    }
}

fn rgba(c: RgbaColor) -> Pixel {
    [
        f32::from(c.red) / 255.0,
        f32::from(c.green) / 255.0,
        f32::from(c.blue) / 255.0,
        f32::from(c.alpha) / 255.0,
    ]
}

fn stops_of(stops: impl Iterator<Item = ColorStop>) -> Vec<(f32, Pixel)> {
    let mut v: Vec<_> = stops.map(|s| (s.stop_offset, rgba(s.color))).collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// The colour of a sorted, non-empty colour line at `t`.
fn gradient(stops: &[(f32, Pixel)], extend: GradientExtend, t: f32) -> Pixel {
    let (first, last) = (stops[0].0, stops[stops.len() - 1].0);
    let span = last - first;
    let t = if span <= 1e-9 {
        t
    } else {
        match extend {
            GradientExtend::Pad => t,
            GradientExtend::Repeat => first + (t - first).rem_euclid(span),
            GradientExtend::Reflect => {
                let m = (t - first).rem_euclid(2.0 * span);
                first + if m > span { 2.0 * span - m } else { m }
            }
        }
    };
    if t <= first {
        return stops[0].1;
    }
    for pair in stops.windows(2) {
        let ((t0, c0), (t1, c1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * f);
        }
    }
    stops[stops.len() - 1].1
}

/// Porter-Duff `src` onto `dst`, both premultiplied.
fn composite(src: Pixel, dst: Pixel, mode: CompositeMode) -> Pixel {
    use CompositeMode::*;
    let (sa, da) = (src[3], dst[3]);
    let (fs, fd) = match mode {
        Clear => (0.0, 0.0),
        Source => (1.0, 0.0),
        Destination => (0.0, 1.0),
        DestinationOver => (1.0 - da, 1.0),
        SourceIn => (da, 0.0),
        DestinationIn => (0.0, sa),
        SourceOut => (1.0 - da, 0.0),
        DestinationOut => (0.0, 1.0 - sa),
        SourceAtop => (da, 1.0 - sa),
        DestinationAtop => (1.0 - da, sa),
        Xor => (1.0 - da, 1.0 - sa),
        Plus => (1.0, 1.0),
        _ => (1.0, 1.0 - sa),
    };
    std::array::from_fn(|i| (src[i] * fs + dst[i] * fd).min(1.0))
}

impl<'a> Painter<'a> for Paintbox<'a> {
    fn outline_glyph(&mut self, glyph_id: GlyphId) {
        self.outline = Path::default();
        self.face.outline_glyph(glyph_id, &mut self.outline);
    }

    fn paint(&mut self, paint: Paint<'a>) {
        self.paint_pixels(&paint);
    }

    fn push_clip(&mut self) {
        let mask = self.mask_of(&self.outline);
        self.push_mask(mask);
    }

    fn push_clip_box(&mut self, b: ClipBox) {
        let path = Path(vec![
            Seg::Move(b.x_min, b.y_min),
            Seg::Line(b.x_max, b.y_min),
            Seg::Line(b.x_max, b.y_max),
            Seg::Line(b.x_min, b.y_max),
            Seg::Close,
        ]);
        let mask = self.mask_of(&path);
        self.push_mask(mask);
    }

    fn pop_clip(&mut self) {
        self.clips.pop();
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        self.layers.push(vec![[0.0; 4]; self.w * self.h]);
        self.modes.push(mode);
    }

    fn pop_layer(&mut self) {
        if self.layers.len() < 2 {
            return;
        }
        let (Some(top), Some(mode)) = (self.layers.pop(), self.modes.pop()) else {
            return;
        };
        if let Some(below) = self.layers.last_mut() {
            for (d, s) in below.iter_mut().zip(top) {
                *d = composite(s, *d, mode);
            }
        }
    }

    fn push_transform(&mut self, t: Transform) {
        let outer = self.transforms.last().copied().unwrap_or(Affine::ID);
        self.transforms
            .push(outer.then_after(Affine([t.a, t.b, t.c, t.d, t.e, t.f])));
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }
}

fn colr(face: &Face<'_>, glyph: GlyphId, em: f32) -> Option<Tile> {
    if !face.is_color_glyph(glyph) {
        return None;
    }
    let s = em / f32::from(face.units_per_em().max(1));
    let margin = (em * MARGIN).ceil();
    let advance = face
        .glyph_hor_advance(glyph)
        .map_or(em, |a| f32::from(a) * s);
    let ascent = (f32::from(face.ascender()) * s).max(em * 0.8);
    let descent = (f32::from(face.descender()) * s).min(-em * 0.2);
    let (left, top) = (-margin, (ascent + margin).ceil());
    let side = MAX_BITMAP as usize;
    let w = ((advance + 2.0 * margin).ceil() as usize).clamp(1, side);
    let h = ((top - descent + margin).ceil() as usize).clamp(1, side);
    let mut painter = Paintbox {
        face,
        w,
        h,
        base: Affine([s, 0.0, 0.0, -s, -left, top]),
        transforms: Vec::new(),
        outline: Path::default(),
        clips: Vec::new(),
        layers: vec![vec![[0.0; 4]; w * h]],
        modes: Vec::new(),
    };
    // The text colour is not known here; a layer that asks for it draws black.
    face.paint_color_glyph(glyph, 0, RgbaColor::new(0, 0, 0, 255), &mut painter)?;
    let rgba = painter
        .layers
        .first()?
        .iter()
        .flat_map(|p| p.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8))
        .collect();
    Some(Tile {
        x: left as i32,
        y: -(top as i32),
        w,
        h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gradient_should_pad_repeat_and_reflect_outside_its_stops() {
        let stops = [(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])];
        assert_eq!(gradient(&stops, GradientExtend::Pad, 2.0)[0], 1.0);
        assert_eq!(gradient(&stops, GradientExtend::Repeat, 1.25)[0], 0.25);
        assert_eq!(gradient(&stops, GradientExtend::Reflect, 1.25)[0], 0.75);
    }

    #[test]
    fn source_over_should_keep_the_destination_under_a_half_covered_source() {
        let out = composite(
            [0.5, 0.0, 0.0, 0.5],
            [0.0, 0.0, 1.0, 1.0],
            CompositeMode::SourceOver,
        );
        assert_eq!(out, [0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_transform_should_invert_back() {
        let t = Affine([2.0, 0.0, 0.0, -3.0, 5.0, 7.0]);
        let (x, y) = t.apply(1.0, 2.0);
        let (bx, by) = t.inverse().map_or((0.0, 0.0), |i| i.apply(x, y));
        assert!((bx - 1.0).abs() < 1e-5 && (by - 2.0).abs() < 1e-5);
    }

    /// An opaque 2x2 PNG (red, green / blue, white) drawn at twice its strike's em is 4x4.
    #[test]
    fn a_png_bitmap_should_scale_to_the_em_and_sit_on_the_baseline() {
        let mut data = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut data, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            let pixels = [
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ];
            w.write_image_data(&pixels).unwrap();
        }
        let image = RasterGlyphImage {
            x: 1,
            y: -1,
            width: 2,
            height: 2,
            pixels_per_em: 2,
            format: RasterImageFormat::PNG,
            data: &data,
        };
        let tile = bitmap(&image, 4.0).unwrap();
        // Bottom edge 2px below the baseline, 4px tall: the top is 2px above it.
        assert_eq!((tile.w, tile.h, tile.x, tile.y), (4, 4, 2, -2));
        assert_eq!(&tile.rgba[..4], &[255, 0, 0, 255]);
    }
}
