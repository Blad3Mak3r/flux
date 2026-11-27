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

fn to_srgb(v: f32) -> f32 {
    let v = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Effects, Time};
    use flux_project::vj_effects::{Effect, EffectKind, MirrorAxis};

    fn picture(rgba: &[u8], place: [f32; 4], opacity: f32, blend: Blend) -> Picture<'_> {
        Picture {
            id: 0,
            width: 1,
            height: 1,
            rgba,
            place,
            opacity,
            blend,
            effects: Default::default(),
        }
    }

    fn pixel(out: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let at = (y * width + x) * 4;
        out[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn a_picture_should_cover_only_its_place_over_black() {
        let red = [255, 0, 0, 255];
        let out = composite(
            &[picture(&red, [0.5, 0.0, 0.5, 1.0], 1.0, Blend::Alpha)],
            4,
            2,
        );
        assert_eq!(pixel(&out, 4, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&out, 4, 3, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn a_picture_drawn_larger_should_blend_between_its_pixels() {
        // Black and white side by side, over four output pixels: the middle two are in between.
        let rgba = [0, 0, 0, 255, 255, 255, 255, 255];
        let wide = Picture {
            width: 2,
            ..picture(&rgba, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
        };
        let out = composite(&[wide], 4, 1);
        let reds: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        assert_eq!((reds[0], reds[3]), (0, 255));
        assert!(
            reds[1] > 0 && reds[1] < reds[2] && reds[2] < 255,
            "{reds:?}"
        );
    }

    #[test]
    fn opacity_and_blends_should_mix_encoded_values_as_resolume_does() {
        let white = [255, 255, 255, 255];
        let grey = [100, 100, 100, 255];
        let mid = [128, 128, 128, 255];
        let whole = [0.0, 0.0, 1.0, 1.0];
        // Half of white over black is the middle of the encoded values, not of the light.
        let half = composite(&[picture(&white, whole, 0.5, Blend::Alpha)], 1, 1);
        assert!(half[0].abs_diff(128) <= 1, "{}", half[0]);
        let added = composite(
            &[
                picture(&grey, whole, 1.0, Blend::Alpha),
                picture(&grey, whole, 1.0, Blend::Add),
            ],
            1,
            1,
        );
        assert!(added[0].abs_diff(200) <= 1, "{}", added[0]);
        // 128 times 128 in encoded values is 64, where light would make it 38 (a much darker one).
        let darker = composite(
            &[
                picture(&mid, whole, 1.0, Blend::Alpha),
                picture(&mid, whole, 1.0, Blend::Multiply),
            ],
            1,
            1,
        );
        assert!(darker[0].abs_diff(64) <= 1, "{}", darker[0]);
        // Screen of two 128s is 192.
        let screened = composite(
            &[
                picture(&mid, whole, 1.0, Blend::Alpha),
                picture(&mid, whole, 1.0, Blend::Screen),
            ],
            1,
            1,
        );
        assert!(screened[0].abs_diff(192) <= 1, "{}", screened[0]);
    }

    fn kinds(chain: &[EffectKind]) -> Vec<Effect> {
        chain
            .iter()
            .enumerate()
            .map(|(at, kind)| Effect {
                id: format!("e{at}"),
                bypass: false,
                modulation: Default::default(),
                kind: kind.clone(),
            })
            .collect()
    }

    /// A picture of `rgba` drawn over a whole output of its own size, with `chain` applied.
    fn effected(rgba: &[u8], (width, height): (u32, u32), chain: &[EffectKind]) -> Vec<u8> {
        let picture = Picture {
            width,
            height,
            effects: Effects::new(&kinds(chain), Time::default()),
            ..picture(rgba, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
        };
        composite(&[picture], width, height)
    }

    const FOUR: [u8; 16] = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];

    fn colours(out: &[u8]) -> Vec<[u8; 3]> {
        out.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
    }

    fn named(name: &str) -> EffectKind {
        EffectKind::default_of(name).unwrap()
    }

    #[test]
    fn a_picture_without_effects_should_be_drawn_as_it_always_was() {
        // The same picture, drawn on its fast path and through a program of effects that change
        // nothing, comes out the same.
        let plain = composite(
            &[Picture {
                width: 4,
                height: 1,
                ..picture(&FOUR, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
            }],
            4,
            1,
        );
        assert_eq!(colours(&plain), colours(&effected(&FOUR, (4, 1), &[])));
        let neutral = [
            EffectKind::Brightness { amount: 0.0 },
            EffectKind::Zoom { amount: 1.0 },
            EffectKind::Pixelate { size: 0.0 },
        ];
        assert_eq!(colours(&plain), colours(&effected(&FOUR, (4, 1), &neutral)));
        // And drawn larger than its own size, between its pixels.
        let wide = composite(
            &[Picture {
                width: 4,
                height: 1,
                ..picture(&FOUR, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
            }],
            8,
            2,
        );
        assert_eq!(colours(&wide).len(), 16);
    }

    #[test]
    fn mirror_and_shift_should_move_the_pixels_of_a_picture() {
        let (r, g, b, w) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]);
        let mirror = EffectKind::Mirror {
            axis: MirrorAxis::Horizontal,
        };
        assert_eq!(colours(&effected(&FOUR, (4, 1), &[mirror])), [r, g, g, r]);
        let shift = EffectKind::Shift { x: 0.5, y: 0.0 };
        assert_eq!(colours(&effected(&FOUR, (4, 1), &[shift])), [b, w, r, g]);
    }

    #[test]
    fn a_zoom_out_should_leave_the_picture_transparent_around_it() {
        let white = [255, 255, 255, 255];
        let zoom = EffectKind::Zoom { amount: 0.5 };
        let out = composite(
            &[Picture {
                effects: Effects::new(&kinds(&[zoom]), Time::default()),
                ..picture(&white, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
            }],
            4,
            1,
        );
        let reds: Vec<u8> = out.chunks(4).map(|p| p[0]).collect();
        assert_eq!(reds, [0, 255, 255, 0]);
    }

    #[test]
    fn colour_effects_should_change_the_light_of_a_picture() {
        let white = [255, 255, 255, 255];
        let invert = EffectKind::Invert { amount: 1.0 };
        assert_eq!(
            colours(&effected(&white, (1, 1), std::slice::from_ref(&invert))),
            [[0, 0, 0]]
        );
        let black = [0, 0, 0, 255];
        let brighten = EffectKind::Brightness { amount: 0.5 };
        let brighter = effected(&black, (1, 1), std::slice::from_ref(&brighten));
        // Half the light of white, as in the blend test.
        assert!(brighter[0].abs_diff(188) <= 1, "{}", brighter[0]);
        // The chain applies in order: brightening then inverting is not the reverse.
        let chain = [brighten.clone(), invert.clone()];
        let reverse = [invert, brighten];
        assert!(effected(&black, (1, 1), &chain)[0].abs_diff(188) <= 1);
        assert_eq!(effected(&black, (1, 1), &reverse)[0], 255);
    }

    #[test]
    fn posterize_and_threshold_should_cut_a_gradient_into_steps() {
        // A ramp of eight greys.
        let ramp: Vec<u8> = (0..8u8)
            .flat_map(|i| {
                let v = i * 36;
                [v, v, v, 255]
            })
            .collect();
        let reds = |chain: &[EffectKind]| -> Vec<u8> {
            effected(&ramp, (8, 1), chain)
                .chunks(4)
                .map(|p| p[0])
                .collect()
        };
        let levels = reds(&[EffectKind::Posterize { levels: 2.0 }]);
        assert!(levels.iter().all(|v| *v == 0 || *v == 255), "{levels:?}");
        assert!(levels[0] == 0 && levels[7] == 255);
        let cut = reds(&[EffectKind::Threshold {
            level: 0.5,
            softness: 0.0,
            dark: "#000000".into(),
            light: "#ffffff".into(),
        }]);
        assert_eq!(cut.iter().filter(|v| **v == 255).count(), 4, "{cut:?}");
        // They are non-decreasing: a step, not a flicker.
        assert!(cut.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn an_rgb_split_should_pull_red_and_blue_apart_at_an_edge() {
        // Black on the left, white on the right.
        let mut edge = Vec::new();
        for _ in 0..4 {
            for x in 0..64 {
                let v = if x < 32 { 0 } else { 255 };
                edge.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let split = EffectKind::RgbSplit {
            amount: 1.0,
            angle: 0.0,
            animate: false,
            pace: flux_project::vj_effects::Pace {
                sync: flux_project::vj_effects::Sync::Free,
                rate: 0.0,
                beats: 1.0,
            },
        };
        let out = effected(&edge, (64, 4), &[split]);
        let at = |x: usize| &out[(64 + x) * 4..(64 + x) * 4 + 3];
        // Far from the edge, grey stays grey.
        assert_eq!(at(2), [0, 0, 0]);
        assert_eq!(at(61), [255, 255, 255]);
        // Next to it the channels disagree: red reads from the right, blue from the left.
        let near = at(31);
        assert!(near[0] > near[2], "{near:?}");
        let other = at(32);
        assert!(other[0] > other[2] || other[2] < 255, "{other:?}");
    }

    #[test]
    fn a_kaleidoscope_should_draw_the_same_wedge_all_round() {
        // A picture with a bright wedge at the right of its centre, drawn on a square output.
        let n = 32usize;
        let mut rgba = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let bright = x > n / 2 && y.abs_diff(n / 2) < 3;
                let v = if bright { 255 } else { 0 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let kaleidoscope = EffectKind::Kaleidoscope {
            segments: 4.0,
            rotation: 0.0,
            spin: 0.0,
            x: 0.5,
            y: 0.5,
            pace: flux_project::vj_effects::Pace {
                sync: flux_project::vj_effects::Sync::Free,
                rate: 0.0,
                beats: 1.0,
            },
        };
        let out = effected(&rgba, (n as u32, n as u32), &[kaleidoscope]);
        let red = |x: usize, y: usize| out[(y * n + x) * 4];
        // The bright line lies along the right of the centre; the middle of a wedge reads it, so it
        // shows along the four diagonals, one in each quarter.
        for (x, y) in [(24, 24), (8, 8), (24, 8), (8, 24)] {
            assert_eq!(red(x, y), 255, "{x}, {y}");
        }
        // Between the arms stays dark.
        for (x, y) in [(24, 16), (8, 16), (16, 8), (16, 24)] {
            assert_eq!(red(x, y), 0, "{x}, {y}");
        }
    }

    #[test]
    fn a_twirl_and_a_tile_should_change_a_picture_and_leave_its_borders() {
        let mut rgba = Vec::new();
        for y in 0..16 {
            for x in 0..16 {
                rgba.extend_from_slice(&[(x * 16) as u8, (y * 16) as u8, 0, 255]);
            }
        }
        let twirled = effected(&rgba, (16, 16), &[named("twirl")]);
        let plain = effected(&rgba, (16, 16), &[]);
        assert_ne!(colours(&twirled), colours(&plain));
        assert_eq!(colours(&twirled)[0], colours(&plain)[0]);
        let tiled = effected(&rgba, (16, 16), &[named("tile")]);
        // Two tiles across: the left edge shows again in the middle of the output.
        assert_ne!(colours(&tiled), colours(&plain));
    }

    #[test]
    fn a_blur_should_soften_an_edge_and_leave_the_rest() {
        let mut edge = Vec::new();
        for _ in 0..40 {
            for x in 0..40 {
                let v = if x < 20 { 0 } else { 255 };
                edge.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let sharp = composite(
            &[Picture {
                width: 40,
                height: 40,
                ..picture(&edge, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
            }],
            40,
            40,
        );
        let blurred = effected(&edge, (40, 40), &[EffectKind::Blur { radius: 1.0 }]);
        let at = |out: &[u8], x: usize| out[(20 * 40 + x) * 4];
        assert_eq!((at(&sharp, 19), at(&sharp, 20)), (0, 255));
        assert!(at(&blurred, 19) > 0 && at(&blurred, 20) < 255);
        assert_eq!((at(&blurred, 2), at(&blurred, 37)), (0, 255));
    }

    #[test]
    fn an_edge_detect_should_light_the_edges_of_a_picture() {
        let mut edge = Vec::new();
        for _ in 0..16 {
            for x in 0..16 {
                let v = if x < 8 { 0 } else { 255 };
                edge.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let edges = effected(&edge, (16, 16), &[named("edgeDetect")]);
        let at = |x: usize| edges[(8 * 16 + x) * 4];
        assert_eq!((at(2), at(13)), (0, 0));
        assert!(at(7) > 100 || at(8) > 100);
    }

    // The composition's effects.

    const STEP: f32 = 1.0 / 60.0;

    fn post(chain: &[EffectKind]) -> Post {
        Post::new(&kinds(chain), Time::default(), STEP)
    }

    fn feedback(amount: f32, zoom: f32) -> EffectKind {
        EffectKind::Feedback {
            amount,
            zoom,
            rotate: 0.0,
            x: 0.0,
            y: 0.0,
            hue: 0.0,
            blend: flux_project::vj_effects::FeedbackBlend::Mix,
        }
    }

    /// A white dot at the pixel `x`, `y` of an output of 8 by 8 pixels, over black.
    fn dot(rgba: &[u8], x: usize, y: usize) -> Picture<'_> {
        picture(
            rgba,
            [x as f32 / 8.0, y as f32 / 8.0, 0.125, 0.125],
            1.0,
            Blend::Alpha,
        )
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn draw(pictures: &[Picture], post: &Post, history: &mut History) -> Vec<u8> {
        composite_post(pictures, 8, 8, post, history)
    }

    #[test]
    fn an_empty_stack_should_draw_exactly_what_it_always_did() {
        let pictures = [dot(&WHITE, 2, 3), dot(&WHITE, 5, 5)];
        let before = composite(&pictures, 8, 8);
        let mut history = History::default();
        for chain in [vec![], vec![EffectKind::Brightness { amount: 0.0 }]] {
            let out = draw(&pictures, &post(&chain), &mut history);
            assert_eq!(out, before);
        }
        // A bypassed effect is as good as none.
        let mut bypassed = kinds(&[feedback(0.9, 1.1)]);
        bypassed[0].bypass = true;
        let none = Post::new(&bypassed, Time::default(), STEP);
        assert!(none.is_empty());
        assert_eq!(draw(&pictures, &none, &mut history), before);
    }

    #[test]
    fn a_feedback_of_nothing_should_equal_none() {
        let mut history = History::default();
        let first = [dot(&WHITE, 2, 2)];
        let second = [dot(&WHITE, 6, 6)];
        let post = post(&[feedback(0.0, 1.5)]);
        draw(&first, &post, &mut history);
        let again = draw(&second, &post, &mut history);
        assert_eq!(again, composite(&second, 8, 8));
        // With some of it kept, the old dot is still there.
        let mut history = History::default();
        let post = super::tests::post(&[feedback(0.8, 1.0)]);
        draw(&first, &post, &mut history);
        let kept = draw(&second, &post, &mut history);
        assert!(pixel(&kept, 8, 2, 2)[0] > 0 && pixel(&kept, 8, 6, 6)[0] > 0);
    }

    #[test]
    fn trails_should_fade_behind_a_moving_dot() {
        let post = post(&[EffectKind::Trails {
            decay: 0.9,
            mode: flux_project::vj_effects::TrailMode::Lighten,
        }]);
        let mut history = History::default();
        let mut trail = Vec::new();
        for x in 1..=4 {
            let out = draw(&[dot(&WHITE, x, 4)], &post, &mut history);
            trail.push(pixel(&out, 8, 1, 4)[0]);
        }
        // The first dot's pixel is lit at first, then fades while the dot is elsewhere.
        assert_eq!(trail[0], 255);
        assert!(
            trail[1] > trail[2] && trail[2] > trail[3] && trail[3] > 0,
            "{trail:?}"
        );
        // The newest dot is as bright as ever.
        let last = draw(&[dot(&WHITE, 5, 4)], &post, &mut history);
        assert_eq!(pixel(&last, 8, 5, 4)[0], 255);
    }

    #[test]
    fn a_bloom_should_light_around_a_bright_pixel_and_leave_the_dark_alone() {
        let bloom = EffectKind::Bloom {
            threshold: 0.5,
            intensity: 1.0,
            radius: 0.3,
        };
        // A white dot of 8 pixels in a frame of 64, over black.
        let dot = [picture(&WHITE, [0.5, 0.5, 0.125, 0.125], 1.0, Blend::Alpha)];
        let plain = composite(&dot, 64, 64);
        let glowing = composite_post(
            &dot,
            64,
            64,
            &post(std::slice::from_ref(&bloom)),
            &mut History::default(),
        );
        let at = |out: &[u8], x: usize, y: usize| out[(y * 64 + x) * 4];
        // Next to the dot it was black and now it glows, less the further it is.
        assert_eq!(at(&plain, 30, 36), 0);
        let (near, far) = (at(&glowing, 30, 36), at(&glowing, 24, 36));
        assert!(near > 0 && near > far, "{near} {far}");
        // The corner is out of its reach, and the dot itself does not get darker.
        assert_eq!(at(&glowing, 63, 63), 0);
        assert_eq!(at(&glowing, 35, 35), 255);
        // A frame with nothing bright gets nothing.
        let dim = [20, 20, 20, 255];
        let dark = [picture(&dim, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)];
        let before = composite(&dark, 16, 16);
        let after = composite_post(&dark, 16, 16, &post(&[bloom]), &mut History::default());
        assert_eq!(before, after);
    }

    #[test]
    fn the_program_should_apply_to_the_whole_composition() {
        // Two dots on black, inverted as one picture: black turns white and the dots black.
        let pictures = [dot(&WHITE, 1, 1), dot(&WHITE, 6, 6)];
        let out = draw(
            &pictures,
            &post(&[EffectKind::Invert { amount: 1.0 }]),
            &mut History::default(),
        );
        assert_eq!(pixel(&out, 8, 4, 4), [255, 255, 255, 255]);
        assert_eq!(pixel(&out, 8, 1, 1), [0, 0, 0, 255]);
        assert_eq!(pixel(&out, 8, 6, 6), [0, 0, 0, 255]);
        // And a zoom of the whole frame moves what is in it.
        let zoomed = draw(
            &[dot(&WHITE, 5, 4)],
            &post(&[EffectKind::Zoom { amount: 2.0 }]),
            &mut History::default(),
        );
        assert_eq!(pixel(&zoomed, 8, 4, 4)[0], 0);
        assert!(pixel(&zoomed, 8, 7, 5)[0] > 150);
    }

    #[test]
    fn the_history_should_be_forgotten_when_the_size_changes_or_the_stack_empties() {
        let post = post(&[feedback(0.9, 1.0)]);
        let mut history = History::default();
        draw(&[dot(&WHITE, 2, 2)], &post, &mut history);
        // Another size: nothing of the old frame is drawn again.
        let black = [0, 0, 0, 255];
        let other = composite_post(
            &[picture(&black, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)],
            4,
            4,
            &post,
            &mut history,
        );
        assert!(other.chunks(4).all(|p| p[0] == 0));
        assert!(history.holds(4, 4) && !history.holds(8, 8));
        // The stack emptied for a frame: the old frame is gone when it comes back.
        draw(&[dot(&WHITE, 2, 2)], &post, &mut history);
        draw(&[dot(&WHITE, 2, 2)], &Post::default(), &mut history);
        let back = draw(&[dot(&WHITE, 6, 6)], &post, &mut history);
        assert_eq!(pixel(&back, 8, 2, 2)[0], 0);
    }

    fn near(a: [f32; 3], b: [f32; 3]) {
        assert!(
            a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4),
            "{a:?} != {b:?}"
        );
    }

    /// `blend_light` for a picture shown at full opacity, displacing nothing.
    fn mixed(blend: Blend, src: [f32; 3], cover: f32, below: [f32; 3]) -> [f32; 3] {
        blend_light(blend, src, cover, 1.0, below, below)
    }

    #[test]
    fn the_keys_should_cut_the_picture_or_what_is_below_by_its_luma() {
        let (below, white, black) = ([0.2, 0.5, 0.9], [1.0; 3], [0.0; 3]);
        // Luma Key shows where it is bright, Luma Key I where it is dark.
        near(mixed(Blend::LumaKey, white, 1.0, below), white);
        near(mixed(Blend::LumaKey, black, 1.0, below), below);
        near(mixed(Blend::LumaKeyInverted, white, 1.0, below), below);
        near(mixed(Blend::LumaKeyInverted, black, 1.0, below), black);
        near(mixed(Blend::LumaKey, [0.5; 3], 1.0, [0.0; 3]), [0.25; 3]);
        // Luma is Alpha takes the opacity, not the picture's alpha: half covered by alpha only,
        // at full opacity, a white pixel is still opaque.
        near(
            blend_light(Blend::LumaIsAlpha, white, 0.5, 1.0, below, below),
            white,
        );
        near(
            blend_light(Blend::LumaIsAlpha, [0.5; 3], 0.5, 0.5, [0.0; 3], [0.0; 3]),
            [0.125; 3],
        );
        // 50Mask darkens what is below by the mask's luma and does not draw the mask.
        near(mixed(Blend::Mask50, white, 1.0, below), below);
        near(mixed(Blend::Mask50, black, 1.0, below), black);
        near(mixed(Blend::Mask50, [0.5; 3], 1.0, [0.8; 3]), [0.4; 3]);
        // AlphaCut is a hard switch at half cover.
        near(mixed(Blend::AlphaCut, [0.3; 3], 0.49, below), below);
        near(mixed(Blend::AlphaCut, [0.3; 3], 0.5, below), [0.3; 3]);
        // Difference I is the inverse of difference.
        near(
            mixed(Blend::DifferenceInverted, [0.3; 3], 1.0, [0.8; 3]),
            [0.5; 3],
        );
    }

    #[test]
    fn rgb_should_take_over_one_channel_at_a_time_as_the_cover_rises() {
        let (below, src) = ([0.0; 3], [1.0; 3]);
        near(mixed(Blend::Rgb, src, 1.0 / 3.0, below), [0.0, 0.0, 1.0]);
        near(mixed(Blend::Rgb, src, 2.0 / 3.0, below), [0.0, 1.0, 1.0]);
        near(mixed(Blend::Rgb, src, 1.0, below), src);
    }

    #[test]
    fn displace_should_read_the_light_below_sideways_by_the_picture_luma() {
        // White pushes the light below one tenth of the width.
        assert_eq!(displace_shift([0.5; 3], 100), 0);
        assert_eq!(displace_shift([1.0; 3], 100), 10);
        assert_eq!(displace_shift([0.0; 3], 100), -10);
        let ramp = [200, 0, 0, 255, 0, 0, 0, 255];
        let rgba = [255; 4];
        let below = Picture {
            width: 2,
            ..picture(&ramp, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha)
        };
        let white = picture(&rgba, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Displace);
        let out = composite(&[below, white], 20, 1);
        // The left half is the red ramp's bright side; displaced right by 4 pixels, the pixels
        // near the middle show the dark side's light.
        assert!(out[8 * 4] < out[4], "{:?}", &out[..80]);
    }

    #[test]
    fn every_blend_should_leave_what_is_below_where_the_picture_is_clear() {
        let below = [0.2, 0.5, 0.9];
        for blend in Blend::ALL {
            near(mixed(blend, [0.7, 0.1, 0.4], 0.0, below), below);
        }
    }

    #[test]
    fn a_blend_should_fade_from_what_is_below_to_the_blended_light_by_its_cover() {
        // Lighten of 0.2 and 0.8 is 0.8; half covered, it is half way there.
        near(mixed(Blend::Lighten, [0.8; 3], 0.5, [0.2; 3]), [0.5; 3]);
        near(mixed(Blend::Darken, [0.2; 3], 0.5, [0.8; 3]), [0.5; 3]);
        near(mixed(Blend::Multiply, [0.5; 3], 0.5, [0.8; 3]), [0.6; 3]);
        near(mixed(Blend::Difference, [0.3; 3], 1.0, [0.8; 3]), [0.5; 3]);
    }

    #[test]
    fn the_separable_blends_should_follow_the_w3c_formulas() {
        let at = |blend, below: f32, picture: f32| separable(blend, below, picture);
        assert!((at(Blend::Exclusion, 0.8, 0.3) - 0.62).abs() < 1e-5);
        assert!((at(Blend::Overlay, 0.25, 0.8) - 0.4).abs() < 1e-5);
        assert!((at(Blend::HardLight, 0.25, 0.8) - 0.7).abs() < 1e-5);
        assert!((at(Blend::HardLight, 0.8, 0.25) - 0.4).abs() < 1e-5);
        assert!((at(Blend::SoftLight, 0.5, 0.5) - 0.5).abs() < 1e-5);
        assert!((at(Blend::ColorDodge, 0.25, 0.5) - 0.5).abs() < 1e-5);
        assert_eq!(at(Blend::ColorDodge, 0.0, 1.0), 0.0);
        assert!((at(Blend::ColorBurn, 0.75, 0.5) - 0.5).abs() < 1e-5);
        assert_eq!(at(Blend::ColorBurn, 1.0, 0.0), 1.0);
        assert!((at(Blend::LinearLight, 0.4, 0.7) - 0.8).abs() < 1e-5);
        assert!((at(Blend::VividLight, 0.75, 0.25) - 0.5).abs() < 1e-5);
        assert!((at(Blend::PinLight, 0.5, 0.2) - 0.4).abs() < 1e-5);
        assert!((at(Blend::PinLight, 0.5, 0.9) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn the_non_separable_blends_should_take_their_parts_from_the_right_layer() {
        let (below, picture) = ([0.8, 0.2, 0.2], [0.1, 0.5, 0.9]);
        // Luminosity is the picture's luminosity, and colour keeps the luminosity below.
        let l = mixed(Blend::Luminosity, picture, 1.0, below);
        assert!((lum(l) - lum(picture)).abs() < 1e-4);
        let c = mixed(Blend::Color, picture, 1.0, below);
        assert!((lum(c) - lum(below)).abs() < 1e-4);
        // A grey picture has no saturation to give.
        let g = mixed(Blend::Saturation, [0.5; 3], 1.0, below);
        assert!(sat(g) < 1e-4);
        // The picture over itself changes nothing.
        for blend in [
            Blend::Hue,
            Blend::Saturation,
            Blend::Color,
            Blend::Luminosity,
        ] {
            near(mixed(blend, below, 1.0, below), below);
        }
    }

    #[test]
    fn a_text_over_an_empty_layer_over_a_video_should_blend_only_where_the_text_is() {
        // Two pixels of text, the first black and solid and the second clear (white, alpha 0),
        // over a flat video; the empty layer between draws no picture at all.
        let text = [0, 0, 0, 255, 255, 255, 255, 0];
        let video = [200, 120, 40, 255];
        let layer = |rgba, width, blend| Picture {
            width,
            ..picture(rgba, [0.0, 0.0, 1.0, 1.0], 1.0, blend)
        };
        let plain = composite(&[layer(&video, 1, Blend::Alpha)], 2, 1);
        for blend in Blend::ALL.into_iter().filter(|b| *b != Blend::Alpha) {
            let out = composite(
                &[layer(&video, 1, Blend::Alpha), layer(&text, 2, blend)],
                2,
                1,
            );
            assert_eq!(pixel(&out, 2, 1, 0), pixel(&plain, 2, 1, 0), "{blend:?}");
        }
        let multiplied = composite(
            &[
                layer(&video, 1, Blend::Alpha),
                layer(&text, 2, Blend::Multiply),
            ],
            2,
            1,
        );
        assert_eq!(pixel(&multiplied, 2, 0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn a_blend_at_half_opacity_should_fade_between_what_is_below_and_the_blend() {
        let (video, light) = ([100, 100, 100, 255], [255, 255, 255, 255]);
        let at = |opacity| {
            let layer = picture(&light, [0.0, 0.0, 1.0, 1.0], opacity, Blend::Difference);
            let below = picture(&video, [0.0, 0.0, 1.0, 1.0], 1.0, Blend::Alpha);
            pixel(&composite(&[below, layer], 1, 1), 1, 0, 0)[0]
        };
        // White against the video is brighter than the video, and half of it sits between.
        let (none, half, full) = (at(0.0), at(0.5), at(1.0));
        assert!(
            none == 100 && none < half && half < full,
            "{none} {half} {full}"
        );
    }

    fn group<'a>(pictures: &[Picture<'a>], opacity: f32, blend: Blend) -> Item<'a> {
        Item::Group(crate::Group {
            pictures: pictures.to_vec(),
            opacity,
            blend,
            effects: Default::default(),
        })
    }

    fn mix(items: &[Item], width: u32) -> Vec<u8> {
        composite_items(items, width, 1, &Post::default(), &mut History::default())
    }

    #[test]
    fn a_blend_transition_should_go_from_the_old_clip_to_the_new_one_through_the_blend() {
        let (old, new) = ([200, 200, 200, 255], [100, 100, 100, 255]);
        let whole = [0.0, 0.0, 1.0, 1.0];
        let at = |t: f32, blend| {
            let pictures = [
                picture(&old, whole, 1.0 - t, Blend::Alpha),
                picture(&new, whole, t, blend),
            ];
            mix(&[group(&pictures, 1.0, Blend::Alpha)], 1)[0]
        };
        for blend in [Blend::Multiply, Blend::Add, Blend::Screen] {
            let (start, mid, end) = (at(0.0, blend), at(0.5, blend), at(1.0, blend));
            assert_eq!((start, end), (200, 100), "{blend:?}");
            assert!(mid != start && mid != end, "{blend:?} {mid}");
        }
        // Added, the middle is brighter than when multiplied.
        assert!(at(0.5, Blend::Multiply) < at(0.5, Blend::Add));
    }

    #[test]
    fn a_group_at_full_alpha_with_no_effects_should_draw_as_its_layers_flat() {
        let (back, mid, top) = ([90, 120, 200, 255], [200, 50, 50, 128], [20, 240, 90, 255]);
        let (whole, right) = ([0.0, 0.0, 1.0, 1.0], [0.5, 0.0, 0.5, 1.0]);
        let layers = [
            picture(&mid, whole, 1.0, Blend::Alpha),
            picture(&top, right, 0.6, Blend::Alpha),
        ];
        let below = picture(&back, whole, 1.0, Blend::Alpha);
        let flat = composite(&[below, layers[0], layers[1]], 4, 1);
        let grouped = mix(
            &[Item::Picture(below), group(&layers, 1.0, Blend::Alpha)],
            4,
        );
        for (a, b) in flat.iter().zip(&grouped) {
            assert!(a.abs_diff(*b) <= 2, "{flat:?} {grouped:?}");
        }
    }

    #[test]
    fn the_first_layer_of_a_group_should_be_drawn_over_clear_not_over_what_is_below() {
        let (back, grey) = ([255, 255, 255, 255], [128, 128, 128, 255]);
        let whole = [0.0, 0.0, 1.0, 1.0];
        let below = picture(&back, whole, 1.0, Blend::Alpha);
        let multiplied = picture(&grey, whole, 1.0, Blend::Multiply);
        let flat = composite(&[below, multiplied], 1, 1);
        let grouped = mix(
            &[
                Item::Picture(below),
                group(&[multiplied], 1.0, Blend::Alpha),
            ],
            1,
        );
        assert!(
            flat[0].abs_diff(128) <= 1,
            "multiplying white keeps the grey"
        );
        assert!(grouped[0].abs_diff(128) <= 1, "{}", grouped[0]);
        // Over a dark picture the flat multiply darkens, and the group's does not.
        let dark = [40, 40, 40, 255];
        let below = picture(&dark, whole, 1.0, Blend::Alpha);
        let flat = composite(&[below, multiplied], 1, 1);
        let grouped = mix(
            &[
                Item::Picture(below),
                group(&[multiplied], 1.0, Blend::Alpha),
            ],
            1,
        );
        assert!(
            flat[0] < 30 && grouped[0].abs_diff(128) <= 1,
            "{flat:?} {grouped:?}"
        );
    }

    #[test]
    fn a_group_should_mix_with_the_layers_below_by_its_opacity_and_blend() {
        let (back, white, grey) = ([0, 0, 0, 255], [255, 255, 255, 255], [100, 100, 100, 255]);
        let whole = [0.0, 0.0, 1.0, 1.0];
        let below = picture(&back, whole, 1.0, Blend::Alpha);
        let half = mix(
            &[
                Item::Picture(below),
                group(
                    &[picture(&white, whole, 1.0, Blend::Alpha)],
                    0.5,
                    Blend::Alpha,
                ),
            ],
            1,
        );
        assert!(half[0].abs_diff(128) <= 1, "{}", half[0]);
        let below = picture(&grey, whole, 1.0, Blend::Alpha);
        let added = mix(
            &[
                Item::Picture(below),
                group(&[picture(&grey, whole, 1.0, Blend::Alpha)], 1.0, Blend::Add),
            ],
            1,
        );
        assert!(added[0].abs_diff(200) <= 1, "{}", added[0]);
    }

    #[test]
    fn a_group_should_draw_its_effects_on_the_mix_of_its_layers() {
        let (red, blue) = ([255, 0, 0, 255], [0, 0, 255, 255]);
        let layers = [
            picture(&red, [0.0, 0.0, 0.5, 1.0], 1.0, Blend::Alpha),
            picture(&blue, [0.5, 0.0, 0.5, 1.0], 1.0, Blend::Alpha),
        ];
        let effects = Effects::new(
            &[Effect {
                id: "m".into(),
                bypass: false,
                modulation: Default::default(),
                kind: EffectKind::Mirror {
                    axis: MirrorAxis::Horizontal,
                },
            }],
            Time::default(),
        );
        let mirrored = Item::Group(crate::Group {
            pictures: layers.to_vec(),
            opacity: 1.0,
            blend: Blend::Alpha,
            effects,
        });
        let plain = mix(&[group(&layers, 1.0, Blend::Alpha)], 4);
        let out = mix(&[mirrored], 4);
        assert_ne!(plain, out, "the effect changed the mix");
        assert_eq!(pixel(&plain, 4, 0, 0), [255, 0, 0, 255]);
        assert_eq!(pixel(&plain, 4, 3, 0), [0, 0, 255, 255]);
    }
}
