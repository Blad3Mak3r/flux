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
        nearest: false,
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
