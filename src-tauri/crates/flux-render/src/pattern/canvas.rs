use super::*;

pub(super) struct Canvas {
    pub(super) rgba: Vec<u8>,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Canvas {
    pub(super) fn blend(&mut self, x: i64, y: i64, colour: [u8; 3], amount: f32) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let at = ((y as u32 * self.width + x as u32) * 4) as usize;
        for (c, paint) in self.rgba[at..at + 3].iter_mut().zip(colour) {
            *c = (f32::from(*c) + (f32::from(paint) - f32::from(*c)) * amount).round() as u8;
        }
    }

    pub(super) fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, colour: [u8; 3], amount: f32) {
        for row in y.max(0)..(y + h).min(i64::from(self.height)) {
            for column in x.max(0)..(x + w).min(i64::from(self.width)) {
                self.blend(column, row, colour, amount);
            }
        }
    }

    /// A rectangle with rounded corners of `radius`, the corners smoothed.
    pub(super) fn rounded(
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
    pub(super) fn disc(&mut self, (cx, cy): (f32, f32), radius: f32, colour: [u8; 3]) {
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
    pub(super) fn logo(&mut self, logo: &Logo, (x, y, side): (i64, i64, u32)) {
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

    pub(super) fn image(&mut self, image: &Image, x: i64, y: i64) {
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
pub(super) fn span(at: u32, of: u32, source: u32) -> (u32, u32) {
    let from = (u64::from(at) * u64::from(source) / u64::from(of)) as u32;
    let to = (u64::from(at + 1) * u64::from(source) / u64::from(of)) as u32;
    (from.min(source - 1), to.max(from + 1).min(source))
}

/// Text as a picture: straight RGBA, cut to its glyphs across and a whole line high.
pub(super) struct Image {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba: Vec<u8>,
}

pub(super) fn block(
    text: &str,
    size: f32,
    bold: bool,
    colour: [u8; 3],
    centre: [f32; 2],
) -> Block<'_> {
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
pub(super) fn text(text: &str, size: f32, bold: bool, colour: [u8; 3], max: u32) -> Image {
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
