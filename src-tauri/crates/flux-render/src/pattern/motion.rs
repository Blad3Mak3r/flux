use super::*;

/// What changes on every refresh: the marker of the strip, the clock's digits and the counter's bits.
pub(super) struct Motion {
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
    pub(super) fn build(
        c: &mut Canvas,
        p: &Palette,
        size: f32,
        (w, y, height): (u32, i64, u32),
    ) -> Self {
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

    pub(super) fn draw(&self, rgba: &mut Vec<u8>, w: u32, h: u32, seconds: f32, clock: &str) {
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
