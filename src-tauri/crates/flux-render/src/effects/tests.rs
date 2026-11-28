use flux_project::vj_effects::{EffectKind as K, Pace, Sync};

use super::*;

const SQUARE: [f32; 2] = [100.0, 100.0];

fn effect(kind: K) -> Effect {
    Effect {
        id: "e1".into(),
        bypass: false,
        modulation: Default::default(),
        kind,
    }
}

fn named(name: &str) -> K {
    K::default_of(name).unwrap()
}

fn build(kinds: &[K]) -> Effects {
    build_at(kinds, Time::default())
}

fn build_at(kinds: &[K], time: Time) -> Effects {
    let chain: Vec<Effect> = kinds.iter().cloned().map(effect).collect();
    Effects::new(&chain, time)
}

fn free(rate: f32) -> Pace {
    Pace {
        sync: Sync::Free,
        rate,
        beats: 1.0,
    }
}

fn close(a: [f32; 3], b: [f32; 3]) {
    assert!(
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4),
        "{a:?} != {b:?}"
    );
}

fn close2(a: [f32; 2], b: [f32; 2]) {
    assert!(
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4),
        "{a:?} != {b:?}"
    );
}

fn one(kind: K, rgb: [f32; 3]) -> [f32; 3] {
    build(&[kind]).colour_of(rgb, [0.5, 0.5], SQUARE)
}

fn source(kind: K, uv: [f32; 2]) -> Option<[f32; 2]> {
    build(&[kind]).source(uv, SQUARE)
}

#[test]
fn no_effect_should_leave_the_picture_as_it_is() {
    let none = build(&[]);
    assert!(none.is_identity());
    assert_eq!(none, Effects::default());
    close(
        none.colour_of([0.1, 0.5, 0.9], [0.5; 2], SQUARE),
        [0.1, 0.5, 0.9],
    );
    assert_eq!(none.source([0.3, 0.8], SQUARE), Some([0.3, 0.8]));
    // Effects that change nothing are the same as none.
    let neutral = [
        K::Brightness { amount: 0.0 },
        K::Contrast { amount: 1.0 },
        K::Saturation { amount: 1.0 },
        K::Hue { degrees: 0.0 },
        K::Invert { amount: 0.0 },
    ];
    assert!(build(&neutral).is_identity());
}

#[test]
fn a_bypassed_effect_should_be_left_out() {
    let mut off = effect(K::Invert { amount: 1.0 });
    off.bypass = true;
    assert!(Effects::new(&[off], Time::default()).is_identity());
}

#[test]
fn brightness_should_add_and_contrast_should_spread_about_grey() {
    close(
        one(K::Brightness { amount: 0.25 }, [0.1, 0.2, 0.9]),
        [0.35, 0.45, 1.0],
    );
    close(
        one(K::Brightness { amount: -0.25 }, [0.1, 0.2, 0.9]),
        [0.0, 0.0, 0.65],
    );
    close(
        one(K::Contrast { amount: 2.0 }, [GREY, 0.3, 0.1]),
        [GREY, 0.3 * 2.0 - GREY, 0.0],
    );
    close(one(K::Contrast { amount: 0.0 }, [0.9, 0.0, 0.3]), [GREY; 3]);
}

#[test]
fn saturation_should_go_to_grey_at_0_and_leave_grey_alone() {
    let grey = LUMA[0] * 0.8 + LUMA[1] * 0.2 + LUMA[2] * 0.4;
    close(
        one(K::Saturation { amount: 0.0 }, [0.8, 0.2, 0.4]),
        [grey; 3],
    );
    close(
        one(K::Saturation { amount: 3.0 }, [0.5, 0.5, 0.5]),
        [0.5; 3],
    );
    let more = one(K::Saturation { amount: 2.0 }, [0.6, 0.4, 0.4]);
    assert!(more[0] > 0.6 && more[1] < 0.4);
}

#[test]
fn hue_should_turn_about_grey_and_come_full_circle() {
    let hue = |degrees| K::Hue { degrees };
    close(one(hue(120.0), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
    close(one(hue(-120.0), [1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]);
    close(one(hue(77.0), [0.3, 0.3, 0.3]), [0.3; 3]);
    let around = build(&vec![hue(90.0); 4]);
    close(
        around.colour_of([0.7, 0.2, 0.1], [0.5; 2], SQUARE),
        [0.7, 0.2, 0.1],
    );
}

#[test]
fn tint_and_invert_should_mix_towards_their_target() {
    let red = K::Tint {
        colour: "#ff0000".into(),
        amount: 1.0,
    };
    let grey = LUMA[0] * 0.2 + LUMA[1] * 0.4 + LUMA[2] * 0.6;
    close(one(red, [0.2, 0.4, 0.6]), [grey, 0.0, 0.0]);
    let none = K::Tint {
        colour: "#ff0000".into(),
        amount: 0.0,
    };
    close(one(none, [0.2, 0.4, 0.6]), [0.2, 0.4, 0.6]);
    close(
        one(K::Invert { amount: 1.0 }, [0.2, 0.4, 1.0]),
        [0.8, 0.6, 0.0],
    );
    close(one(K::Invert { amount: 0.5 }, [0.2, 0.4, 1.0]), [0.5; 3]);
}

#[test]
fn consecutive_affine_colour_effects_should_fold_into_one_matrix_in_order() {
    let a = build(&[K::Brightness { amount: 0.2 }, K::Contrast { amount: 2.0 }]);
    let b = build(&[K::Contrast { amount: 2.0 }, K::Brightness { amount: 0.2 }]);
    assert_eq!((a.colour, b.colour), (1, 1));
    let rgb = [0.3, 0.1, 0.2];
    let by_hand = |v: f32| ((v + 0.2 - GREY) * 2.0 + GREY).clamp(0.0, 1.0);
    close(a.colour_of(rgb, [0.5; 2], SQUARE), rgb.map(by_hand));
    let other = |v: f32| ((v - GREY) * 2.0 + GREY + 0.2).clamp(0.0, 1.0);
    close(b.colour_of(rgb, [0.5; 2], SQUARE), rgb.map(other));
    // A longer chain through the matrix is the chain applied one effect at a time.
    let chain = [
        K::Saturation { amount: 1.5 },
        K::Hue { degrees: 40.0 },
        K::Invert { amount: 0.3 },
        K::Brightness { amount: -0.1 },
    ];
    let whole = build(&chain);
    assert_eq!(whole.colour, 1);
    let mut by_step = [0.4, 0.3, 0.2];
    for kind in &chain {
        let m = matrix(kind).unwrap();
        by_step = std::array::from_fn(|i| {
            m[i][0] * by_step[0] + m[i][1] * by_step[1] + m[i][2] * by_step[2] + m[i][3]
        });
    }
    close(whole.colour_of([0.4, 0.3, 0.2], [0.5; 2], SQUARE), by_step);
}

#[test]
fn the_stages_should_keep_the_order_of_the_stack_within_each() {
    let program = build(&[
        K::Posterize { levels: 4.0 },
        K::Blur { radius: 0.2 },
        named("mirror"),
        K::Brightness { amount: 0.1 },
        K::Zoom { amount: 2.0 },
        named("threshold"),
        K::Contrast { amount: 2.0 },
        K::Hue { degrees: 30.0 },
        K::Blur { radius: 0.2 },
    ]);
    let kinds = |steps: &[Step]| steps.iter().map(|s| s.kind).collect::<Vec<_>>();
    assert_eq!(kinds(program.uv_steps()), [MIRROR_HORIZONTAL, ZOOM]);
    assert_eq!(kinds(program.sample_steps()), [BLUR]);
    // The two blurs are one, added as independent blurs are.
    assert!((program.sample_steps()[0].p[0] - 0.2f32.hypot(0.2)).abs() < 1e-6);
    // The brightness stands alone before the threshold; the contrast and hue fold after it.
    assert_eq!(
        kinds(program.colour_steps()),
        [POSTERIZE, MATRIX, THRESHOLD, MATRIX]
    );
    // The program lays the stages out one after the other.
    let all = kinds(&program.steps[..5]);
    assert_eq!(all, [MIRROR_HORIZONTAL, ZOOM, BLUR, POSTERIZE, MATRIX]);
}

#[test]
fn steps_beyond_the_most_should_be_left_out() {
    let program = build(&vec![K::Zoom { amount: 1.0 }; MAX_STEPS + 3]);
    assert_eq!(program.uv as usize, MAX_STEPS);
    let mixed = build(
        &[
            vec![K::Zoom { amount: 1.0 }; MAX_STEPS - 1],
            vec![named("blur"), named("posterize"), named("vignette")],
        ]
        .concat(),
    );
    assert_eq!(mixed.uv + mixed.sample + mixed.colour, MAX_STEPS as u32);
    assert_eq!(mixed.sample, 1);
}

#[test]
fn mirror_should_fold_one_half_onto_the_other() {
    let at = |axis, uv| source(K::Mirror { axis }, uv);
    assert_eq!(at(MirrorAxis::Horizontal, [0.75, 0.9]), Some([0.25, 0.9]));
    assert_eq!(at(MirrorAxis::Horizontal, [0.25, 0.9]), Some([0.25, 0.9]));
    assert_eq!(at(MirrorAxis::Vertical, [0.2, 0.75]), Some([0.2, 0.25]));
    assert_eq!(at(MirrorAxis::Both, [0.75, 0.75]), Some([0.25, 0.25]));
}

#[test]
fn zoom_should_scale_about_the_centre_and_leave_transparent_beyond() {
    let zoom = |amount, uv| source(K::Zoom { amount }, uv);
    assert_eq!(zoom(2.0, [0.5, 0.5]), Some([0.5, 0.5]));
    assert_eq!(zoom(2.0, [0.75, 0.5]), Some([0.625, 0.5]));
    assert_eq!(zoom(0.5, [0.75, 0.5]), Some([1.0, 0.5]));
    assert_eq!(zoom(0.5, [0.9, 0.5]), None);
}

#[test]
fn shift_should_move_the_picture_and_wrap_around() {
    let shift = K::Shift { x: 0.25, y: -0.5 };
    close2(source(shift.clone(), [0.5, 0.5]).unwrap(), [0.25, 0.0]);
    close2(source(shift, [0.1, 0.2]).unwrap(), [0.85, 0.7]);
}

#[test]
fn steps_should_walk_back_from_the_last_effect() {
    // Mirror, then zoom out: the zoom's transparent border is not mirrored into the picture.
    let out = build(&[named("mirror"), K::Zoom { amount: 0.5 }]);
    assert_eq!(out.source([0.95, 0.5], SQUARE), None);
    // Shift, then mirror: the mirror sees the shifted picture.
    let both = build(&[K::Shift { x: 0.25, y: 0.0 }, named("mirror")]);
    let [x, _] = both.source([0.75, 0.5], SQUARE).unwrap();
    assert!((x - 0.0).abs() < 1e-6, "{x}");
}

#[test]
fn rotate_should_turn_the_picture_and_leave_the_corners_transparent() {
    let turn = K::Rotate {
        angle: 90.0,
        spin: 0.0,
        pace: free(1.0),
    };
    close2(source(turn.clone(), [0.75, 0.5]).unwrap(), [0.5, 0.25]);
    // 45 degrees on a square: the corner goes outside the picture.
    let diagonal = K::Rotate {
        angle: 45.0,
        spin: 0.0,
        pace: free(1.0),
    };
    assert_eq!(source(diagonal, [0.02, 0.02]), None);
    // A spin of a turn a cycle, a quarter of a cycle in: 90 degrees.
    let spin = K::Rotate {
        angle: 0.0,
        spin: 1.0,
        pace: free(1.0),
    };
    let spun = build_at(
        &[spin],
        Time {
            seconds: 0.25,
            beats: 0.0,
        },
    );
    close2(spun.source([0.75, 0.5], SQUARE).unwrap(), [0.5, 0.25]);
}

#[test]
fn a_kaleidoscope_should_repeat_and_mirror_its_wedge() {
    let kaleidoscope = K::Kaleidoscope {
        segments: 4.0,
        rotation: 0.0,
        spin: 0.0,
        x: 0.5,
        y: 0.5,
        pace: free(0.0),
    };
    let at = |angle: f32| {
        let r = 0.2;
        source(
            kaleidoscope.clone(),
            [0.5 + r * angle.cos(), 0.5 + r * angle.sin()],
        )
        .unwrap()
    };
    let segment = std::f32::consts::FRAC_PI_2;
    // A wedge and the next one are the same, and each is its own mirror image.
    close2(at(0.3), at(0.3 + segment));
    close2(at(0.3), at(0.3 + 3.0 * segment));
    close2(at(0.3), at(segment - 0.3));
    // The middle of a wedge reads straight to the right of the centre.
    close2(at(segment / 2.0), [0.7, 0.5]);
    // The centre stays the centre, whatever the angle.
    close2(source(kaleidoscope, [0.5, 0.5]).unwrap(), [0.5, 0.5]);
}

#[test]
fn a_kaleidoscope_should_turn_with_its_rotation_and_spin() {
    let make = |rotation, spin| K::Kaleidoscope {
        segments: 6.0,
        rotation,
        spin,
        x: 0.5,
        y: 0.5,
        pace: free(1.0),
    };
    let still = build(&[make(60.0, 0.0)]);
    let spun = build_at(
        &[make(0.0, 1.0)],
        Time {
            seconds: 60.0 / 360.0,
            beats: 0.0,
        },
    );
    let p = [0.7, 0.62];
    close2(
        still.source(p, SQUARE).unwrap(),
        spun.source(p, SQUARE).unwrap(),
    );
}

#[test]
fn polar_should_bend_a_rectangle_into_a_disc() {
    let polar = |amount, uv| source(K::Polar { amount }, uv).unwrap();
    // To the right of the centre, half way out: angle 0 is the middle of the picture's width,
    // half the radius the middle of its height.
    close2(polar(1.0, [0.75, 0.5]), [0.5, 0.5]);
    close2(polar(0.0, [0.75, 0.5]), [0.75, 0.5]);
    close2(polar(0.5, [0.75, 0.5]), [0.625, 0.5]);
    // Straight above the centre is a quarter turn back.
    close2(polar(1.0, [0.5, 0.25]), [0.25, 0.5]);
}

#[test]
fn a_wave_should_push_each_line_by_its_place_in_the_wave() {
    let wave = |direction, seconds| {
        build_at(
            &[K::Wave {
                amplitude: 0.1,
                frequency: 1.0,
                direction,
                pace: free(1.0),
            }],
            Time {
                seconds,
                beats: 0.0,
            },
        )
    };
    let horizontal = wave(MirrorAxis::Horizontal, 0.0);
    close2(horizontal.source([0.4, 0.25], SQUARE).unwrap(), [0.5, 0.25]);
    close2(horizontal.source([0.4, 0.5], SQUARE).unwrap(), [0.4, 0.5]);
    // The wave travels: a quarter of a cycle on, the line at the top is at the crest.
    let later = wave(MirrorAxis::Horizontal, 0.25);
    close2(later.source([0.4, 0.0], SQUARE).unwrap(), [0.5, 0.0]);
    // Vertical waves push the columns.
    let vertical = wave(MirrorAxis::Vertical, 0.0);
    close2(vertical.source([0.25, 0.4], SQUARE).unwrap(), [0.25, 0.5]);
}

#[test]
fn a_twirl_should_turn_the_picture_about_its_centre_and_leave_the_rest() {
    let twirl = K::Twirl {
        angle: 90.0,
        radius: 0.3,
        x: 0.5,
        y: 0.5,
    };
    let [x, y] = source(twirl.clone(), [0.7, 0.5]).unwrap();
    // Still 0.2 from the centre, but turned.
    assert!(((x - 0.5).hypot(y - 0.5) - 0.2).abs() < 1e-4);
    assert!((y - 0.5).abs() > 0.01, "{y}");
    // The centre does not move, and outside the radius nothing does.
    close2(source(twirl.clone(), [0.5, 0.5]).unwrap(), [0.5, 0.5]);
    close2(source(twirl, [0.05, 0.5]).unwrap(), [0.05, 0.5]);
}

#[test]
fn a_bulge_should_magnify_the_centre_and_a_negative_one_pinch_it() {
    let bulge = |amount| K::Bulge {
        amount,
        radius: 1.0,
        x: 0.5,
        y: 0.5,
    };
    close2(source(bulge(0.0), [0.7, 0.5]).unwrap(), [0.7, 0.5]);
    let swollen = source(bulge(1.0), [0.7, 0.5]).unwrap();
    assert!(swollen[0] > 0.5 && swollen[0] < 0.7 - 0.05, "{swollen:?}");
    let pinched = source(bulge(-1.0), [0.7, 0.5]).unwrap();
    assert!(pinched[0] > 0.7, "{pinched:?}");
}

#[test]
fn tiles_should_repeat_the_picture_and_turn_alternate_ones_over() {
    let tile = |mirror| K::Tile {
        columns: 2.0,
        rows: 1.0,
        mirror,
    };
    close2(source(tile(false), [0.6, 0.3]).unwrap(), [0.2, 0.3]);
    close2(source(tile(true), [0.6, 0.3]).unwrap(), [0.8, 0.3]);
    close2(source(tile(true), [0.1, 0.3]).unwrap(), [0.2, 0.3]);
}

#[test]
fn pixelate_should_read_every_cell_at_its_centre() {
    let cells = K::Pixelate { size: 0.25 };
    close2(source(cells.clone(), [0.1, 0.3]).unwrap(), [0.125, 0.375]);
    close2(source(cells, [0.99, 0.99]).unwrap(), [0.875, 0.875]);
    close2(
        source(K::Pixelate { size: 0.0 }, [0.1, 0.3]).unwrap(),
        [0.1, 0.3],
    );
}

#[test]
fn a_slide_should_scroll_round_or_off_the_picture() {
    let slide = |wrap| K::Slide {
        x: 1.0,
        y: 0.0,
        wrap,
        pace: free(1.0),
    };
    let at = |wrap, seconds| {
        build_at(
            &[slide(wrap)],
            Time {
                seconds,
                beats: 0.0,
            },
        )
        .source([0.5, 0.5], SQUARE)
    };
    close2(at(true, 0.25).unwrap(), [0.25, 0.5]);
    close2(at(true, 1.25).unwrap(), [0.25, 0.5]);
    // Not wrapping, the picture leaves: at 0.75 it is a whole picture to the right.
    close2(at(false, 0.25).unwrap(), [0.25, 0.5]);
    assert_eq!(at(false, 1.0), None);
}

#[test]
fn a_displacement_should_move_the_picture_a_little_and_drift() {
    let displace = K::Displace {
        amplitude: 0.05,
        scale: 4.0,
        pace: free(1.0),
    };
    let at = |seconds| {
        build_at(
            std::slice::from_ref(&displace),
            Time {
                seconds,
                beats: 0.0,
            },
        )
        .source([0.31, 0.62], SQUARE)
        .unwrap()
    };
    let [x, y] = at(0.0);
    assert!((x - 0.31).abs() <= 0.05 + 1e-4 && (y - 0.62).abs() <= 0.05 + 1e-4);
    assert_ne!(at(0.0), at(0.4));
    assert_eq!(at(0.4), at(0.4));
}

#[test]
fn the_hash_should_be_deterministic_and_spread() {
    assert_eq!(hash3(7, 8, 9), hash3(7, 8, 9));
    assert_ne!(hash3(7, 8, 9), hash3(7, 8, 10));
    assert_ne!(hash3(7, 8, 9), hash3(8, 8, 9));
    assert_ne!(hash3(7, 8, 9), hash3(7, 9, 9));
    assert_eq!(lowbias(0), 0);
    // A fixed value, so the shader's copy of the hash can be held to it.
    assert_eq!(lowbias(1), 1_753_845_952);
    let values: Vec<f32> = (0..20_000).map(|i| unit(hash3(3, i, 5))).collect();
    assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    assert!((mean - 0.5).abs() < 0.01, "{mean}");
    // Noise is continuous: close points give close values, and it is a pure function.
    let n = |x| value_noise([x, 0.3], 9);
    assert!((n(1.5) - n(1.5001)).abs() < 0.01);
    assert_eq!(n(2.2), n(2.2));
}

#[test]
fn the_seed_of_an_effect_should_come_from_its_id_and_fit_an_f32() {
    assert_eq!(seed_of("g14"), seed_of("g14"));
    assert_ne!(seed_of("g14"), seed_of("g15"));
    assert!(seed_of("a long identifier of an effect") < (1 << 24));
}

#[test]
fn a_glitch_should_shift_the_same_bands_in_the_same_cycle_and_others_in_the_next() {
    let glitch = K::ShiftGlitch {
        amount: 0.5,
        block: 0.1,
        probability: 1.0,
        pace: free(1.0),
    };
    let bands = |seconds: f64| -> Vec<f32> {
        let program = build_at(
            std::slice::from_ref(&glitch),
            Time {
                seconds,
                beats: 0.0,
            },
        );
        (0..10)
            .map(|row| {
                program
                    .source([0.5, row as f32 * 0.1 + 0.05], SQUARE)
                    .unwrap()[0]
            })
            .collect()
    };
    // Within a cycle it is the same picture.
    assert_eq!(bands(0.1), bands(0.9));
    // The next cycle is another.
    assert_ne!(bands(0.1), bands(1.1));
    // Every band is shifted, each by its own amount, and the rows within a band move together.
    let program = build(std::slice::from_ref(&glitch));
    let a = program.source([0.5, 0.31], SQUARE).unwrap()[0];
    let b = program.source([0.5, 0.39], SQUARE).unwrap()[0];
    assert_eq!(a, b);
    assert!(bands(0.1).windows(2).any(|w| w[0] != w[1]));
    // With no probability, nothing moves.
    let calm = K::ShiftGlitch {
        amount: 0.5,
        block: 0.1,
        probability: 0.0,
        pace: free(1.0),
    };
    close2(source(calm, [0.5, 0.31]).unwrap(), [0.5, 0.31]);
}

#[test]
fn a_twitch_should_jump_with_its_probability_and_hold_until_the_next_cycle() {
    let twitch = |probability| K::Twitch {
        amount: 1.0,
        probability,
        pace: free(1.0),
    };
    let at = |probability: f32, seconds: f64| {
        build_at(
            &[twitch(probability)],
            Time {
                seconds,
                beats: 0.0,
            },
        )
    };
    assert!(at(0.0, 0.5).is_identity());
    let jump = at(1.0, 0.5);
    assert_eq!((jump.uv, jump.colour), (1, 1));
    assert_eq!(jump.uv_steps()[0].kind, TRANSFORM);
    // Held through the cycle, another in the next.
    assert_eq!(jump, at(1.0, 0.9));
    assert_ne!(jump.uv_steps()[0].p, at(1.0, 1.5).uv_steps()[0].p);
    // About half the cycles at a probability of a half.
    let jumps = (0..400)
        .filter(|s| !at(0.5, f64::from(*s) + 0.5).is_identity())
        .count();
    assert!((150..250).contains(&jumps), "{jumps}");
    // Whatever it draws keeps the picture on screen: the jump is a bounded move and size.
    let [x, y, scale] = [0, 1, 2].map(|i| jump.uv_steps()[0].p[i]);
    assert!(x.abs() <= 0.25 && y.abs() <= 0.25 && (0.5..=1.5).contains(&scale));
}

#[test]
fn a_strobe_should_be_on_for_its_duty_of_each_cycle() {
    let strobe = |mode| K::Strobe {
        colour: "#ff0000".into(),
        duty: 0.25,
        mode,
        pace: free(2.0),
    };
    let at = |mode, seconds: f64| {
        build_at(
            &[strobe(mode)],
            Time {
                seconds,
                beats: 0.0,
            },
        )
        .colour_of([0.2, 0.4, 0.6], [0.5; 2], SQUARE)
    };
    // 2 cycles a second: on for the first eighth of a second of each half second.
    close(at(StrobeMode::Flash, 0.05), [1.0, 0.0, 0.0]);
    close(at(StrobeMode::Flash, 0.2), [0.2, 0.4, 0.6]);
    close(at(StrobeMode::Flash, 0.55), [1.0, 0.0, 0.0]);
    close(at(StrobeMode::Blackout, 0.05), [0.0; 3]);
    close(at(StrobeMode::Blackout, 0.3), [0.2, 0.4, 0.6]);
}

#[test]
fn a_pace_should_count_seconds_or_beats() {
    let free = Pace {
        sync: Sync::Free,
        rate: 2.0,
        beats: 4.0,
    };
    let beats = Pace {
        sync: Sync::Beats,
        ..free
    };
    let time = Time {
        seconds: 3.0,
        beats: 8.0,
    };
    assert_eq!(cycles(&free, time), 6.0);
    assert_eq!(cycles(&beats, time), 2.0);
    let step = Step::new(WAVE, 0, &[]).at(6.25);
    assert_eq!((step.index, step.phase), (6, 0.25));
    // A long run keeps its phase exact: the index wraps, the phase does not drift.
    let long = Step::new(WAVE, 0, &[]).at(1.0e9 + 0.5);
    assert_eq!(long.phase, 0.5);
    assert!(f64::from(long.index) < INDEX_RANGE);
}

#[test]
fn posterize_threshold_and_levels_should_work_on_the_perceived_light() {
    let grey = |v: f32| decode(v);
    // Two levels: below half the perceived light is black, above it white.
    let two = K::Posterize { levels: 2.0 };
    close(one(two.clone(), [grey(0.4); 3]), [0.0; 3]);
    close(one(two, [grey(0.6); 3]), [1.0; 3]);
    // Four levels: the perceived light goes to a third of the way.
    close(
        one(K::Posterize { levels: 4.0 }, [grey(0.3); 3]),
        [grey(1.0 / 3.0); 3],
    );
    let threshold = K::Threshold {
        level: 0.5,
        softness: 0.0,
        dark: "#000000".into(),
        light: "#ff0000".into(),
    };
    close(one(threshold.clone(), [grey(0.4); 3]), [0.0; 3]);
    close(one(threshold, [grey(0.6); 3]), [1.0, 0.0, 0.0]);
    // A soft threshold is half way at its level.
    let soft = K::Threshold {
        level: 0.5,
        softness: 0.4,
        dark: "#000000".into(),
        light: "#ffffff".into(),
    };
    close(one(soft, [grey(0.5); 3]), [0.5; 3]);
    // Levels leave a picture alone at their defaults and stretch it from a raised black.
    close(one(named("levels"), [0.2, 0.4, 0.8]), [0.2, 0.4, 0.8]);
    let raised = K::Levels {
        black: 0.5,
        white: 1.0,
        gamma: 1.0,
    };
    close(one(raised, [grey(0.75); 3]), [grey(0.5); 3]);
    // Solarize turns what is above the threshold into its negative.
    let solarize = K::Solarize { threshold: 0.5 };
    close(one(solarize.clone(), [grey(0.25); 3]), [grey(0.25); 3]);
    close(one(solarize, [grey(0.75); 3]), [grey(0.25); 3]);
}

#[test]
fn colorize_should_paint_the_grey_between_two_colours() {
    let colorize = |amount| K::Colorize {
        dark: "#000000".into(),
        light: "#ff0000".into(),
        amount,
    };
    close(one(colorize(1.0), [0.0; 3]), [0.0; 3]);
    close(one(colorize(1.0), [1.0; 3]), [1.0, 0.0, 0.0]);
    close(one(colorize(0.0), [0.2, 0.4, 0.6]), [0.2, 0.4, 0.6]);
}

#[test]
fn a_vignette_should_darken_the_corners_and_leave_the_centre() {
    let vignette = build(&[named("vignette")]);
    let at = |x: f32, y: f32| vignette.colour_of([1.0; 3], [x, y], SQUARE)[0];
    assert_eq!(at(0.5, 0.5), 1.0);
    assert!(at(0.0, 0.0) < 0.5);
    assert!(at(0.0, 0.0) < at(0.1, 0.1));
}

#[test]
fn noise_should_be_the_same_for_a_pixel_in_a_step_of_time_and_move_with_it() {
    let noise = K::Noise {
        amount: 1.0,
        size: 0.0,
        animated: true,
    };
    let at = |seconds: f64, x: f32| {
        build_at(
            std::slice::from_ref(&noise),
            Time {
                seconds,
                beats: 0.0,
            },
        )
        .colour_of([0.3; 3], [x, 0.5], SQUARE)
    };
    assert_eq!(at(0.01, 0.2), at(0.02, 0.2));
    assert_ne!(at(0.01, 0.2), at(0.5, 0.2));
    assert_ne!(at(0.01, 0.2), at(0.01, 0.4));
    let still = K::Noise {
        amount: 1.0,
        size: 0.0,
        animated: false,
    };
    let a = build_at(
        std::slice::from_ref(&still),
        Time {
            seconds: 1.0,
            beats: 0.0,
        },
    );
    let b = build_at(
        &[still],
        Time {
            seconds: 9.0,
            beats: 0.0,
        },
    );
    assert_eq!(a, b);
}

#[test]
fn scanlines_should_darken_lines_and_scroll_with_the_cycle() {
    let lines = |seconds: f64| {
        build_at(
            &[K::Scanlines {
                count: 10.0,
                intensity: 1.0,
                pace: free(1.0),
            }],
            Time {
                seconds,
                beats: 0.0,
            },
        )
    };
    let at = |program: &Effects, y: f32| program.colour_of([1.0; 3], [0.5, y], SQUARE)[0];
    let still = lines(0.0);
    // Each line is lit at its top and darkest half way down it.
    assert!((at(&still, 0.0) - 1.0).abs() < 1e-4 && at(&still, 0.05) < 0.01);
    // Half a line later they have swapped.
    let moved = lines(0.5);
    assert!(at(&moved, 0.0) < 0.01 && (at(&moved, 0.05) - 1.0).abs() < 1e-4);
}

#[test]
fn the_split_the_blurs_and_the_edges_should_be_steps_of_the_sample_stage() {
    let program = build(&[
        K::RgbSplit {
            amount: 1.0,
            angle: 0.0,
            animate: false,
            pace: free(1.0),
        },
        named("radialBlur"),
        named("zoomBlur"),
        named("edgeDetect"),
        named("blur"),
    ]);
    let kinds: Vec<u32> = program.sample_steps().iter().map(|s| s.kind).collect();
    assert_eq!(kinds, [BLUR, SPLIT, RADIAL, ZOOM_BLUR, EDGE]);
    assert!((program.sample_steps()[1].p[0] - SPLIT_REACH).abs() < 1e-6);
}

#[test]
fn the_program_should_pack_into_the_floats_of_its_uniform() {
    let program = build(&[named("mirror"), named("blur"), named("posterize")]);
    let floats = program.pack();
    assert_eq!(floats.len(), 4 + 16 * MAX_STEPS);
    assert_eq!(&floats[..4], [1.0, 1.0, 1.0, 0.0]);
    assert_eq!(floats[4], MIRROR_HORIZONTAL as f32);
    // The second step is the blur, 16 floats on, with its radius first among its settings.
    assert_eq!(floats[4 + 16], BLUR as f32);
    assert_eq!(floats[4 + 16 + 4], 0.1);
    assert_eq!(Effects::default().pack()[..4], [0.0; 4]);
}

#[test]
fn the_blur_taps_should_lie_in_the_disc_without_repeating() {
    let taps: Vec<[f32; 2]> = (0..BLUR_TAPS).map(|i| blur_tap(i, BLUR_TAPS)).collect();
    assert!(taps.iter().all(|[x, y]| x.hypot(*y) < 1.0));
    for (i, a) in taps.iter().enumerate() {
        assert!(
            taps[i + 1..]
                .iter()
                .all(|b| (a[0] - b[0]).hypot(a[1] - b[1]) > 0.05)
        );
    }
}
