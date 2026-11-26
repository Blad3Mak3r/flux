//! The composition drawn on the CPU, small, for a monitor in the editor: the same pictures at the
//! same places, opacities and blends the GPU draws on the screens, before their mapping.

use crate::post::{History, Post};
use crate::{Blend, Picture};

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
/// the light is mixed in linear light as the GPU mixes it.
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
    if post.is_empty() {
        history.clear();
    } else {
        crate::post::apply(post, &mut light, w, h, history);
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
}
