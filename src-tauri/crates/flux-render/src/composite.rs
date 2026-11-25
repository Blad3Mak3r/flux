//! The composition drawn on the CPU, small, for a monitor in the editor: the same pictures at the
//! same places, opacities and blends the GPU draws on the screens, before their mapping.

use crate::{Blend, Picture, effects};

/// Where an output column or row samples a picture: between its pixels `a` and `b`, `t` of the
/// way to `b`.
#[derive(Clone, Copy)]
struct Tap {
    a: usize,
    b: usize,
    t: f32,
}

/// Where coordinate `at` (0 to 1 across a side of `pixels` pixels) samples: between pixels, the
/// edge ones repeating beyond.
fn tap(at: f32, pixels: usize) -> Tap {
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
/// the light is mixed in linear light as the GPU mixes it.
pub fn composite(pictures: &[Picture], width: u32, height: u32) -> Vec<u8> {
    let linear: Vec<f32> = (0..=255u8)
        .map(|v| to_linear(f32::from(v) / 255.0))
        .collect();
    let (w, h) = (width as usize, height as usize);
    let mut light = vec![[0.0f32; 3]; w * h];
    for picture in pictures {
        let [px, py, pw, ph] = picture.place;
        let (sw, sh) = (picture.width as usize, picture.height as usize);
        if pw <= 0.0
            || ph <= 0.0
            || sw == 0
            || sh == 0
            || picture.opacity <= 0.0
            || picture.rgba.len() < sw * sh * 4
        {
            continue;
        }
        let columns = taps(w, px, pw, sw);
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
        let mut put = |at: usize, [r, g, b, cover]: [f32; 4]| {
            if cover <= 0.0 {
                return;
            }
            for (c, premultiplied) in [r, g, b].into_iter().enumerate() {
                let colour = premultiplied / cover;
                let dst = &mut light[at][c];
                *dst = mix(picture.blend, colour, cover * opacity, *dst).clamp(0.0, 1.0);
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
            continue;
        }
        // With effects each pixel finds where the picture is read, then blurs and colours there.
        let sample = |uv: [f32; 2]| {
            let (column, row) = (tap(uv[0], sw), tap(uv[1], sh));
            let top = lerp(pixel(row.a, column.a), pixel(row.a, column.b), column.t);
            let bottom = lerp(pixel(row.b, column.a), pixel(row.b, column.b), column.t);
            lerp(top, bottom, row.t)
        };
        let reach = {
            let short = sw.min(sh) as f32 * fx.blur * effects::MAX_BLUR;
            [short / sw as f32, short / sh as f32]
        };
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
                let Some(at) = fx.source([u, v]) else {
                    continue;
                };
                let mut mixed = if fx.blur > 0.0 {
                    let mut sum = [0.0f32; 4];
                    for n in 0..effects::BLUR_TAPS {
                        let [dx, dy] = effects::blur_tap(n);
                        let t = sample([at[0] + dx * reach[0], at[1] + dy * reach[1]]);
                        sum.iter_mut().zip(t).for_each(|(s, t)| *s += t);
                    }
                    sum.map(|s| s / effects::BLUR_TAPS as f32)
                } else {
                    sample(at)
                };
                if mixed[3] > 0.0 {
                    let [r, g, b] = fx.colour_of([
                        mixed[0] / mixed[3],
                        mixed[1] / mixed[3],
                        mixed[2] / mixed[3],
                    ]);
                    mixed = [r * mixed[3], g * mixed[3], b * mixed[3], mixed[3]];
                }
                put(y * w + x, mixed);
            }
        }
    }
    // Linear light back to sRGB through a table, finer in the dark where sRGB is.
    let srgb: Vec<u8> = (0..=SRGB_STEPS)
        .map(|i| to_srgb((i as f32 / SRGB_STEPS as f32).powi(2)))
        .collect();
    let encode = |v: f32| srgb[(v.sqrt() * SRGB_STEPS as f32 + 0.5) as usize];
    let mut rgba = Vec::with_capacity(w * h * 4);
    for [r, g, b] in light {
        rgba.extend_from_slice(&[encode(r), encode(g), encode(b), 255]);
    }
    rgba
}

/// Entries of the table that turns linear light back into sRGB.
const SRGB_STEPS: usize = 4096;

/// One channel of `src`, covering `cover` of the pixel, mixed into `dst`.
fn mix(blend: Blend, src: f32, cover: f32, dst: f32) -> f32 {
    let over_black = src * cover;
    let over_white = src * cover + (1.0 - cover);
    match blend {
        Blend::Alpha => over_black + dst * (1.0 - cover),
        Blend::Add => dst + over_black,
        Blend::Subtract => dst - over_black,
        Blend::Multiply => dst * over_white,
        Blend::Screen => over_black + dst * (1.0 - over_black),
        Blend::Lighten => dst.max(over_black),
        Blend::Darken => dst.min(over_white),
    }
}

pub(crate) fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> u8 {
    let v = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn opacity_and_blends_should_mix_in_linear_light() {
        let white = [255, 255, 255, 255];
        let grey = [188, 188, 188, 255]; // about half the light of white
        let whole = [0.0, 0.0, 1.0, 1.0];
        let half = composite(&[picture(&white, whole, 0.5, Blend::Alpha)], 1, 1);
        assert!(half[0].abs_diff(188) <= 1, "{}", half[0]);
        let added = composite(
            &[
                picture(&grey, whole, 1.0, Blend::Alpha),
                picture(&grey, whole, 1.0, Blend::Add),
            ],
            1,
            1,
        );
        assert_eq!(added[0], 255);
        let darker = composite(
            &[
                picture(&white, whole, 1.0, Blend::Alpha),
                picture(&grey, whole, 1.0, Blend::Multiply),
            ],
            1,
            1,
        );
        assert!(darker[0].abs_diff(188) <= 1, "{}", darker[0]);
    }

    /// A picture of `rgba` drawn over a whole output of its own size, with `chain` applied.
    fn effected(rgba: &[u8], (width, height): (u32, u32), chain: &[effects::Effect]) -> Vec<u8> {
        let picture = Picture {
            width,
            height,
            effects: effects::Effects::new(chain),
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

    #[test]
    fn mirror_and_shift_should_move_the_pixels_of_a_picture() {
        let (r, g, b, w) = ([255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]);
        let mirror = effects::Effect::Mirror(effects::Mirror::Horizontal);
        assert_eq!(colours(&effected(&FOUR, (4, 1), &[mirror])), [r, g, g, r]);
        let shift = effects::Effect::Shift(0.5, 0.0);
        assert_eq!(colours(&effected(&FOUR, (4, 1), &[shift])), [b, w, r, g]);
    }

    #[test]
    fn a_zoom_out_should_leave_the_picture_transparent_around_it() {
        let white = [255, 255, 255, 255];
        let zoom = effects::Effect::Zoom(0.5);
        let out = composite(
            &[Picture {
                effects: effects::Effects::new(&[zoom]),
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
        let invert = effects::Effect::Invert(1.0);
        assert_eq!(colours(&effected(&white, (1, 1), &[invert])), [[0, 0, 0]]);
        let black = [0, 0, 0, 255];
        let brighter = effected(&black, (1, 1), &[effects::Effect::Brightness(0.5)]);
        // Half the light of white, as in the blend test.
        assert!(brighter[0].abs_diff(188) <= 1, "{}", brighter[0]);
        // The chain applies in order: brightening then inverting is not the reverse.
        let chain = [effects::Effect::Brightness(0.5), invert];
        let reverse = [invert, effects::Effect::Brightness(0.5)];
        assert!(effected(&black, (1, 1), &chain)[0].abs_diff(188) <= 1);
        assert_eq!(effected(&black, (1, 1), &reverse)[0], 255);
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
        let blurred = effected(&edge, (40, 40), &[effects::Effect::Blur(1.0)]);
        let at = |out: &[u8], x: usize| out[(20 * 40 + x) * 4];
        assert_eq!((at(&sharp, 19), at(&sharp, 20)), (0, 255));
        assert!(at(&blurred, 19) > 0 && at(&blurred, 20) < 255);
        assert_eq!((at(&blurred, 2), at(&blurred, 37)), (0, 255));
    }
}
