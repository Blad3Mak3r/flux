//! The composition drawn on the CPU, small, for a monitor in the editor: the same pictures at the
//! same places, opacities and blends the GPU draws on the screens, before their mapping.

use crate::post::{History, Post};
use crate::{Blend, Item, Picture};

/// Where an output column or row samples a picture: between its pixels `a` and `b`, `t` of the
/// way to `b`.
#[derive(Clone, Copy)]
pub(crate) struct Tap {
    pub(crate) a: usize,
    pub(crate) b: usize,
    pub(crate) t: f32,
}

/// Where coordinate `at` (0 to 1 across a side of `pixels` pixels) samples: between pixels, the
/// edge ones repeating beyond.
pub(crate) fn tap(at: f32, pixels: usize) -> Tap {
    let p = (at * pixels as f32 - 0.5).max(0.0);
    let a = (p as usize).min(pixels - 1);
    Tap {
        a,
        b: (a + 1).min(pixels - 1),
        t: (p - a as f32).min(1.0),
    }
}

/// For each of `out` output pixels along one side, where it samples a picture of `pixels` pixels
/// placed from `start` over `size` of the output, or `None` outside it.
fn taps(out: usize, start: f32, size: f32, pixels: usize) -> Vec<Option<Tap>> {
    (0..out)
        .map(|i| {
            let at = ((i as f32 + 0.5) / out as f32 - start) / size;
            (0.0..1.0).contains(&at).then(|| tap(at, pixels))
        })
        .collect()
}

/// Draws `pictures`, the first at the bottom, over black into `width` x `height` pixels of 8-bit
/// sRGB RGBA. Each output pixel blends the four pixels of each picture around it (bilinear), and
/// the layers are mixed in encoded (gamma) values as the GPU mixes them, as Resolume does.
pub fn composite(pictures: &[Picture], width: u32, height: u32) -> Vec<u8> {
    composite_post(
        pictures,
        width,
        height,
        &Post::default(),
        &mut History::default(),
    )
}

/// [`composite`] with the composition's effects `post` applied to the mix before it is encoded.
/// `history` is what the monitor remembers of the frame before, which feedback and trails draw
/// again; it is forgotten when `post` has no effect and when the size changes. With no effect the
/// result is exactly what [`composite`] always drew.
pub fn composite_post(
    pictures: &[Picture],
    width: u32,
    height: u32,
    post: &Post,
    history: &mut History,
) -> Vec<u8> {
    let items: Vec<Item> = pictures.iter().copied().map(Item::Picture).collect();
    composite_items(&items, width, height, post, history)
}

/// [`composite_post`] of pictures and groups of them: a group's pictures are mixed together over
/// a clear picture first, and the mix is drawn like a picture of the group's own, with its
/// effects, blend and opacity.
pub fn composite_items(
    items: &[Item],
    width: u32,
    height: u32,
    post: &Post,
    history: &mut History,
) -> Vec<u8> {
    let tables = Tables {
        linear: (0..=255u8)
            .map(|v| to_linear(f32::from(v) / 255.0))
            .collect(),
        gamma: gamma_table(),
    };
    let (w, h) = (width as usize, height as usize);
    // The layers' mix, in encoded values; it is turned into linear light only for the effects.
    let mut canvas = Canvas::new(w, h, 1.0);
    for item in items {
        match item {
            Item::Picture(picture) => canvas.draw(picture, &tables),
            Item::Group(group) => {
                let mut inner = Canvas::new(w, h, 0.0);
                for picture in &group.pictures {
                    inner.draw(picture, &tables);
                }
                let rgba = inner.rgba();
                canvas.draw(
                    &Picture {
                        id: 0,
                        width,
                        height,
                        rgba: &rgba,
                        place: [0.0, 0.0, 1.0, 1.0],
                        opacity: group.opacity,
                        blend: group.blend,
                        effects: group.effects,
                        nearest: false,
                    },
                    &tables,
                );
            }
        }
    }
    let mut light = canvas.light;
    if post.is_empty() {
        history.clear();
    } else {
        for c in light.iter_mut().flatten() {
            *c = to_linear(*c);
        }
        crate::post::apply(post, &mut light, w, h, history);
        for c in light.iter_mut().flatten() {
            *c = tables.gamma.of(*c);
        }
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for [r, g, b] in light {
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        rgba.extend_from_slice(&[byte(r), byte(g), byte(b), 255]);
    }
    rgba
}

/// The tables the drawing reads: 8-bit encoded values to linear light, and back.
struct Tables {
    linear: Vec<f32>,
    gamma: GammaTable,
}

/// The mix of the pictures drawn so far, in encoded values, and how much of each pixel they cover
/// (whole for the composition, which starts black; clear for a group).
struct Canvas {
    w: usize,
    h: usize,
    light: Vec<[f32; 3]>,
    alpha: Vec<f32>,
}

impl Canvas {
    fn new(w: usize, h: usize, alpha: f32) -> Self {
        Self {
            w,
            h,
            light: vec![[0.0; 3]; w * h],
            alpha: vec![alpha; w * h],
        }
    }

    /// The canvas as 8-bit encoded straight RGBA, the picture a group's mix is.
    fn rgba(&self) -> Vec<u8> {
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        let mut rgba = Vec::with_capacity(self.w * self.h * 4);
        for (&[r, g, b], &a) in self.light.iter().zip(&self.alpha) {
            rgba.extend_from_slice(&[byte(r), byte(g), byte(b), byte(a)]);
        }
        rgba
    }

    /// Draws `picture` over what is there.
    fn draw(&mut self, picture: &Picture, tables: &Tables) {
        let (w, h) = (self.w, self.h);
        let [px, py, pw, ph] = picture.place;
        let (sw, sh) = (picture.width as usize, picture.height as usize);
        if pw <= 0.0
            || ph <= 0.0
            || sw == 0
            || sh == 0
            || picture.opacity <= 0.0
            || picture.rgba.len() < sw * sh * 4
        {
            return;
        }
        let (linear, gamma) = (&tables.linear, &tables.gamma);
        let columns = taps(w, px, pw, sw);
        // Displace reads the light as it was before this layer, not the pixels it has displaced.
        let before = (picture.blend == Blend::Displace).then(|| self.light.clone());
        let opacity = picture.opacity.min(1.0);
        // One source pixel: its light times its cover (premultiplied), and its cover.
        let pixel = |row: usize, column: usize| {
            let at = (row * sw + column) * 4;
            let p = &picture.rgba[at..at + 4];
            let cover = f32::from(p[3]) / 255.0;
            [
                linear[usize::from(p[0])] * cover,
                linear[usize::from(p[1])] * cover,
                linear[usize::from(p[2])] * cover,
                cover,
            ]
        };
        let lerp = |x: [f32; 4], y: [f32; 4], t: f32| {
            std::array::from_fn::<f32, 4, _>(|i| x[i] + (y[i] - x[i]) * t)
        };
        let (light, alpha) = (&mut self.light, &mut self.alpha);
        let mut put = |at: usize, [r, g, b, cover]: [f32; 4]| {
            if cover <= 0.0 {
                return;
            }
            let colour = [r, g, b].map(|premultiplied| gamma.of(premultiplied / cover));
            let below = light[at];
            let made = before.as_ref().map(|before| {
                let x = at % w;
                let shift = displace_shift(colour, w);
                before[at - x + (x as i32 + shift).clamp(0, w as i32 - 1) as usize]
            });
            let made = made.unwrap_or(below);
            if alpha[at] >= 1.0 {
                light[at] =
                    blend_light(picture.blend, colour, cover * opacity, opacity, below, made);
            } else {
                (light[at], alpha[at]) = blend_over(
                    picture.blend,
                    colour,
                    cover * opacity,
                    opacity,
                    (below, alpha[at]),
                    made,
                );
            }
        };
        let fx = &picture.effects;
        if fx.is_identity() {
            for (y, row) in taps(h, py, ph, sh).into_iter().enumerate() {
                let Some(row) = row else { continue };
                for (x, column) in columns.iter().enumerate() {
                    let Some(column) = column else { continue };
                    let top = lerp(pixel(row.a, column.a), pixel(row.a, column.b), column.t);
                    let bottom = lerp(pixel(row.b, column.a), pixel(row.b, column.b), column.t);
                    put(y * w + x, lerp(top, bottom, row.t));
                }
            }
            return;
        }
        // With effects each pixel finds where the picture is read, reads it there and colours it.
        let sample = |uv: [f32; 2]| {
            let (column, row) = (tap(uv[0], sw), tap(uv[1], sh));
            let top = lerp(pixel(row.a, column.a), pixel(row.a, column.b), column.t);
            let bottom = lerp(pixel(row.b, column.a), pixel(row.b, column.b), column.t);
            lerp(top, bottom, row.t)
        };
        let size = [sw as f32, sh as f32];
        for y in 0..h {
            let v = ((y as f32 + 0.5) / h as f32 - py) / ph;
            if !(0.0..1.0).contains(&v) {
                continue;
            }
            for x in 0..w {
                let u = ((x as f32 + 0.5) / w as f32 - px) / pw;
                if !(0.0..1.0).contains(&u) {
                    continue;
                }
                let Some((at, glitch)) = fx.walk([u, v], size) else {
                    continue;
                };
                let [r, g, b, cover] = fx.sample(&sample, at, glitch, size);
                if cover <= 0.0 {
                    continue;
                }
                let [r, g, b] = fx.colour_of([r, g, b], [u, v], size);
                put(y * w + x, [r * cover, g * cover, b * cover, cover]);
            }
        }
    }
}

/// Linear light to its encoded value through a table, finer in the dark where sRGB is.
struct GammaTable(Vec<f32>);

fn gamma_table() -> GammaTable {
    GammaTable(
        (0..=SRGB_STEPS)
            .map(|i| to_srgb((i as f32 / SRGB_STEPS as f32).powi(2)))
            .collect(),
    )
}

impl GammaTable {
    fn of(&self, linear: f32) -> f32 {
        self.0[(linear.clamp(0.0, 1.0).sqrt() * SRGB_STEPS as f32 + 0.5) as usize]
    }
}

/// Entries of the table that turns linear light into its encoded value.
const SRGB_STEPS: usize = 4096;

/// Displace shifts the light below sideways by up to this share of the width, either way.
const DISPLACE: f32 = 0.2;

/// How many of `width` pixels Displace shifts the light below for the picture's colour `src`:
/// none at mid grey, rounded as the shader does.
pub(crate) fn displace_shift(src: [f32; 3], width: usize) -> i32 {
    ((lum(src.map(|v| v.clamp(0.0, 1.0))) - 0.5) * DISPLACE * width as f32 + 0.5).floor() as i32
}

/// How much of the picture each channel shows, for the keys: its `cover` (alpha times `opacity`)
/// changed by what the key reads of the picture `s`, as the shader's `key_cover` has it.
fn key_cover(blend: Blend, cover: f32, opacity: f32, s: [f32; 3]) -> [f32; 3] {
    let l = lum(s);
    match blend {
        Blend::AlphaCut => [if cover >= 0.5 { 1.0 } else { 0.0 }; 3],
        Blend::LumaKey => [cover * l; 3],
        Blend::LumaKeyInverted => [cover * (1.0 - l); 3],
        Blend::LumaIsAlpha => [if cover > 0.0 { opacity * l } else { 0.0 }; 3],
        Blend::Rgb => [2.0, 1.0, 0.0].map(|n| (3.0 * cover - n).clamp(0.0, 1.0)),
        _ => [cover; 3],
    }
}

/// `src` (a straight, encoded colour) of a picture with alpha times `opacity` `cover` over
/// `below` (encoded too), as the picture shader draws it: `mix(below, blend(below, src), cover)`,
/// the cover changed per channel by the keys. `displaced` is the light below at the place
/// Displace reads (`below` for the other blends).
pub(crate) fn blend_light(
    blend: Blend,
    src: [f32; 3],
    cover: f32,
    opacity: f32,
    below: [f32; 3],
    displaced: [f32; 3],
) -> [f32; 3] {
    let b = below.map(|v| v.clamp(0.0, 1.0));
    let (mixed, cover) = blend_parts(blend, src, cover, opacity, below, displaced);
    std::array::from_fn(|c| (b[c] + (mixed[c] - b[c]) * cover[c]).clamp(0.0, 1.0))
}

/// [`blend_light`] over a `below` that is only `alpha` there (inside a group), the picture's
/// W3C compositing over a backdrop with an alpha of its own: the light and the alpha that result.
/// Over a whole backdrop it is [`blend_light`] with an alpha of 1.
pub(crate) fn blend_over(
    blend: Blend,
    src: [f32; 3],
    cover: f32,
    opacity: f32,
    (below, alpha): ([f32; 3], f32),
    displaced: [f32; 3],
) -> ([f32; 3], f32) {
    let b = below.map(|v| v.clamp(0.0, 1.0));
    let s = src.map(|v| v.clamp(0.0, 1.0));
    let (made, c) = blend_parts(blend, src, cover, opacity, below, displaced);
    let a = (c[0] + c[1] + c[2]) / 3.0;
    let out = alpha + a * (1.0 - alpha);
    let light = std::array::from_fn(|i| {
        let premultiplied =
            (1.0 - c[i]) * alpha * b[i] + c[i] * ((1.0 - alpha) * s[i] + alpha * made[i]);
        (premultiplied / out.max(1e-6)).clamp(0.0, 1.0)
    });
    (light, out)
}

/// What the blend makes of the light below and the picture, and how much of each channel of it
/// shows (the cover, changed by the keys).
fn blend_parts(
    blend: Blend,
    src: [f32; 3],
    cover: f32,
    opacity: f32,
    below: [f32; 3],
    displaced: [f32; 3],
) -> ([f32; 3], [f32; 3]) {
    let b = below.map(|v| v.clamp(0.0, 1.0));
    let s = src.map(|v| v.clamp(0.0, 1.0));
    let cover = key_cover(blend, cover, opacity, s);
    let mixed = match blend {
        Blend::Displace => displaced.map(|v| v.clamp(0.0, 1.0)),
        Blend::Mask50 => b.map(|v| v * lum(s)),
        Blend::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        Blend::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        Blend::Color => set_lum(s, lum(b)),
        Blend::Luminosity => set_lum(b, lum(s)),
        _ => std::array::from_fn(|c| separable(blend, b[c], s[c])),
    };
    (mixed, cover)
}

/// The W3C Compositing and Blending formula of a separable blend, of the light below `b` and the
/// picture's `s`, as the shader has it.
fn separable(blend: Blend, b: f32, s: f32) -> f32 {
    let hard = |b: f32, s: f32| {
        if s <= 0.5 {
            2.0 * b * s
        } else {
            1.0 - 2.0 * (1.0 - b) * (1.0 - s)
        }
    };
    let dodge = |b: f32, s: f32| {
        if b <= 0.0 {
            0.0
        } else if s >= 1.0 {
            1.0
        } else {
            (b / (1.0 - s)).min(1.0)
        }
    };
    let burn = |b: f32, s: f32| {
        if b >= 1.0 {
            1.0
        } else if s <= 0.0 {
            0.0
        } else {
            1.0 - ((1.0 - b) / s).min(1.0)
        }
    };
    match blend {
        Blend::Add => (b + s).min(1.0),
        Blend::Subtract => (b - s).max(0.0),
        Blend::Multiply => b * s,
        Blend::Screen => b + s - b * s,
        Blend::Lighten => b.max(s),
        Blend::Darken => b.min(s),
        Blend::Difference => (b - s).abs(),
        Blend::DifferenceInverted => 1.0 - (b - s).abs(),
        Blend::Exclusion => b + s - 2.0 * b * s,
        Blend::Overlay => hard(s, b),
        Blend::HardLight => hard(b, s),
        Blend::SoftLight => {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        Blend::ColorDodge => dodge(b, s),
        Blend::ColorBurn => burn(b, s),
        Blend::LinearLight => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        Blend::VividLight => {
            if s <= 0.5 {
                burn(b, 2.0 * s)
            } else {
                dodge(b, 2.0 * s - 1.0)
            }
        }
        Blend::PinLight => {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }
        _ => s,
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_colour(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_colour(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if x <= n {
        return [0.0; 3];
    }
    c.map(|v| (v - n) * s / (x - n))
}

pub(crate) fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

pub(crate) fn to_srgb(v: f32) -> f32 {
    let v = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests;
