//! The effects of a picture, as the GPU and the CPU monitor both draw them.
//!
//! A chain of [`Effect`]s, in the order they apply, becomes an [`Effects`] value:
//!
//! * The colour effects (brightness, contrast, saturation, hue, tint, invert) are each affine on
//!   the picture's light, so the whole chain composes into one 3x4 matrix, working on linear
//!   light (the picture's colours are mixed that way everywhere) and clamped to 0 to 1 after.
//! * The effects that move the picture (mirror, zoom, shift) stay a list of steps. Drawing walks
//!   the list backwards from the output's coordinate to the picture's ([`Effects::source`]).
//!   Colour effects and these commute (a colour does not depend on where it sits), so all the
//!   steps apply, then the matrix.
//! * Blurs add up as the spread of independent blurs does, by the square root of the sum of their
//!   squares, up to a full blur; the picture is then blurred once, where the steps leave it.

use crate::composite::to_linear;

/// Most steps (mirror, zoom, shift) a picture draws: two full stacks of 8.
pub const MAX_STEPS: usize = 16;
/// The taps a blur averages: points of a disc, evenly spread.
pub const BLUR_TAPS: usize = 32;
/// The radius of a full blur, as a fraction of the picture's short side.
pub const MAX_BLUR: f32 = 0.05;
/// Mid grey as linear light (sRGB 0.5), which contrast pivots about.
const GREY: f32 = 0.214;
/// How much each colour counts in a grey (Rec. 709, for linear light).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Which side a mirror folds onto the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// The left half onto the right.
    Horizontal,
    /// The top half onto the bottom.
    Vertical,
    /// Both.
    Both,
}

/// One effect, as it is applied to a picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// Adds to every colour, -1 to 1.
    Brightness(f32),
    /// Spreads colours about mid grey, 1 is none.
    Contrast(f32),
    /// Spreads colours about their grey, 1 is none.
    Saturation(f32),
    /// Turns colours about the grey axis, in degrees.
    Hue(f32),
    /// Mixes the grey with an sRGB colour (0 to 1 each), `amount` 0 to 1 of the way.
    Tint {
        colour: [f32; 3],
        amount: f32,
    },
    /// Mixes with the negative, `amount` 0 to 1 of the way.
    Invert(f32),
    /// Softens over this much of a full blur, 0 to 1.
    Blur(f32),
    Mirror(Mirror),
    /// Draws larger about the centre; outside the picture is transparent.
    Zoom(f32),
    /// Moves right and down by fractions of the picture's size, wrapping around.
    Shift(f32, f32),
}

const STEP_MIRROR_HORIZONTAL: f32 = 0.0;
const STEP_MIRROR_VERTICAL: f32 = 1.0;
const STEP_MIRROR_BOTH: f32 = 2.0;
const STEP_ZOOM: f32 = 3.0;
const STEP_SHIFT: f32 = 4.0;

/// A colour matrix: each row gives a new red, green or blue as its first three numbers times
/// the old red, green and blue, plus the fourth.
pub type Colour = [[f32; 4]; 3];

const IDENTITY: Colour = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// What a picture's effects come to, ready for drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effects {
    /// All the colour effects, as one matrix.
    pub colour: Colour,
    /// The steps that move the picture, the first applied first: `[kind, a, b, 0]`.
    pub steps: [[f32; 4]; MAX_STEPS],
    pub count: u32,
    /// How much of a full blur, 0 to 1.
    pub blur: f32,
}

impl Default for Effects {
    /// No effect: the picture as it is.
    fn default() -> Self {
        Self {
            colour: IDENTITY,
            steps: [[0.0; 4]; MAX_STEPS],
            count: 0,
            blur: 0.0,
        }
    }
}

/// `a` then `b`, as one matrix.
fn then(a: &Colour, b: &Colour) -> Colour {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let through = (0..3).map(|k| b[i][k] * a[k][j]).sum::<f32>();
            through + if j == 3 { b[i][3] } else { 0.0 }
        })
    })
}

/// The matrix of a colour effect, or `None` for the others.
fn matrix(effect: Effect) -> Option<Colour> {
    let linear = |a: [[f32; 3]; 3], b: [f32; 3]| -> Colour {
        std::array::from_fn(|i| [a[i][0], a[i][1], a[i][2], b[i]])
    };
    let grey_mix = |keep: f32, towards: [f32; 3]| -> Colour {
        // `keep` of each colour, and the rest of the grey scaled by `towards`.
        let a = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (if i == j { keep } else { 0.0 }) + (1.0 - keep) * towards[i] * LUMA[j]
            })
        });
        linear(a, [0.0; 3])
    };
    Some(match effect {
        Effect::Brightness(amount) => linear(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [amount; 3],
        ),
        Effect::Contrast(amount) => linear(
            [[amount, 0.0, 0.0], [0.0, amount, 0.0], [0.0, 0.0, amount]],
            [(1.0 - amount) * GREY; 3],
        ),
        Effect::Saturation(amount) => grey_mix(amount, [1.0; 3]),
        Effect::Hue(degrees) => {
            let (sin, cos) = degrees.to_radians().sin_cos();
            // A turn about (1, 1, 1): cos of the colour, the rest of the grey, and the cross
            // product with the axis.
            let third = (1.0 - cos) / 3.0;
            let side = sin / 3.0_f32.sqrt();
            let a = [
                [cos + third, third - side, third + side],
                [third + side, cos + third, third - side],
                [third - side, third + side, cos + third],
            ];
            linear(a, [0.0; 3])
        }
        Effect::Tint { colour, amount } => grey_mix(1.0 - amount, colour.map(to_linear)),
        Effect::Invert(amount) => linear(
            [
                [1.0 - 2.0 * amount, 0.0, 0.0],
                [0.0, 1.0 - 2.0 * amount, 0.0],
                [0.0, 0.0, 1.0 - 2.0 * amount],
            ],
            [amount; 3],
        ),
        _ => return None,
    })
}

impl Effects {
    /// What the effects of `chain` come to, applied from the first.
    pub fn new(chain: &[Effect]) -> Self {
        let mut effects = Self::default();
        let mut blur = 0.0f32;
        for effect in chain {
            if let Some(colour) = matrix(*effect) {
                effects.colour = then(&effects.colour, &colour);
                continue;
            }
            let step = match *effect {
                Effect::Blur(radius) => {
                    blur = blur.hypot(radius);
                    continue;
                }
                Effect::Mirror(Mirror::Horizontal) => [STEP_MIRROR_HORIZONTAL, 0.0, 0.0, 0.0],
                Effect::Mirror(Mirror::Vertical) => [STEP_MIRROR_VERTICAL, 0.0, 0.0, 0.0],
                Effect::Mirror(Mirror::Both) => [STEP_MIRROR_BOTH, 0.0, 0.0, 0.0],
                Effect::Zoom(amount) => [STEP_ZOOM, amount, 0.0, 0.0],
                Effect::Shift(x, y) => [STEP_SHIFT, x, y, 0.0],
                _ => continue,
            };
            if (effects.count as usize) < MAX_STEPS {
                effects.steps[effects.count as usize] = step;
                effects.count += 1;
            }
        }
        effects.blur = blur.min(1.0);
        effects
    }

    /// Whether drawing with them is the same as drawing without.
    pub fn is_identity(&self) -> bool {
        self.colour == IDENTITY && self.count == 0 && self.blur <= 0.0
    }

    /// Where the picture's coordinate `uv` (0 to 1 across and down) is drawn from: the steps
    /// walked back from the last. `None` where a zoom out leaves the picture, which is
    /// transparent.
    pub fn source(&self, uv: [f32; 2]) -> Option<[f32; 2]> {
        let mut p = uv;
        for step in self.steps[..self.count as usize].iter().rev() {
            let [kind, a, b, _] = *step;
            if kind == STEP_MIRROR_HORIZONTAL || kind == STEP_MIRROR_BOTH {
                p[0] = fold(p[0]);
            }
            if kind == STEP_MIRROR_VERTICAL || kind == STEP_MIRROR_BOTH {
                p[1] = fold(p[1]);
            }
            if kind == STEP_ZOOM {
                p = p.map(|v| (v - 0.5) / a + 0.5);
                if p.iter().any(|v| !(0.0..=1.0).contains(v)) {
                    return None;
                }
            }
            if kind == STEP_SHIFT {
                p = [wrap(p[0] - a), wrap(p[1] - b)];
            }
        }
        Some(p)
    }

    /// `rgb` (linear light) through the colour matrix, kept within 0 to 1.
    pub fn colour_of(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            let row = &self.colour[i];
            (row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2] + row[3]).clamp(0.0, 1.0)
        })
    }

    /// The numbers the shader reads, in the order of its `Fx` struct: the matrix's three rows,
    /// the number of steps and the blur, then the steps.
    pub(crate) fn pack(&self) -> [f32; FX_FLOATS] {
        let mut out = [0.0; FX_FLOATS];
        for (i, row) in self.colour.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(row);
        }
        out[12] = self.count as f32;
        out[13] = self.blur;
        for (i, step) in self.steps.iter().enumerate() {
            out[16 + i * 4..20 + i * 4].copy_from_slice(step);
        }
        out
    }
}

/// The floats of one picture's effects in the shader.
pub(crate) const FX_FLOATS: usize = 16 + 4 * MAX_STEPS;

/// The right half of a mirror shows the left half turned over.
fn fold(v: f32) -> f32 {
    if v > 0.5 { 1.0 - v } else { v }
}

/// `v` brought back into 0 to 1 by going round.
fn wrap(v: f32) -> f32 {
    v - v.floor()
}

/// Where tap `i` of a blur is, in a disc of radius 1 (a sunflower spiral: equal areas).
pub fn blur_tap(i: usize) -> [f32; 2] {
    let radius = ((i as f32 + 0.5) / BLUR_TAPS as f32).sqrt();
    let angle = i as f32 * 2.399_963;
    [radius * angle.cos(), radius * angle.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) {
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4),
            "{a:?} != {b:?}"
        );
    }

    fn one(effect: Effect, rgb: [f32; 3]) -> [f32; 3] {
        Effects::new(&[effect]).colour_of(rgb)
    }

    #[test]
    fn no_effect_should_leave_the_picture_as_it_is() {
        let none = Effects::new(&[]);
        assert!(none.is_identity());
        assert_eq!(none, Effects::default());
        close(none.colour_of([0.1, 0.5, 0.9]), [0.1, 0.5, 0.9]);
        assert_eq!(none.source([0.3, 0.8]), Some([0.3, 0.8]));
        // Effects that change nothing are the same as none.
        let neutral = [
            Effect::Brightness(0.0),
            Effect::Contrast(1.0),
            Effect::Saturation(1.0),
            Effect::Hue(0.0),
            Effect::Invert(0.0),
        ];
        close(
            Effects::new(&neutral).colour_of([0.2, 0.4, 0.6]),
            [0.2, 0.4, 0.6],
        );
    }

    #[test]
    fn brightness_should_add_and_contrast_should_spread_about_grey() {
        close(
            one(Effect::Brightness(0.25), [0.1, 0.2, 0.9]),
            [0.35, 0.45, 1.0],
        );
        close(
            one(Effect::Brightness(-0.25), [0.1, 0.2, 0.9]),
            [0.0, 0.0, 0.65],
        );
        close(
            one(Effect::Contrast(2.0), [GREY, 0.3, 0.1]),
            [GREY, 0.3 * 2.0 - GREY, 0.0],
        );
        close(one(Effect::Contrast(0.0), [0.9, 0.0, 0.3]), [GREY; 3]);
    }

    #[test]
    fn saturation_should_go_to_grey_at_0_and_leave_grey_alone() {
        let grey = LUMA[0] * 0.8 + LUMA[1] * 0.2 + LUMA[2] * 0.4;
        close(one(Effect::Saturation(0.0), [0.8, 0.2, 0.4]), [grey; 3]);
        close(one(Effect::Saturation(3.0), [0.5, 0.5, 0.5]), [0.5; 3]);
        // Above 1 it moves colours away from their grey.
        let more = one(Effect::Saturation(2.0), [0.6, 0.4, 0.4]);
        assert!(more[0] > 0.6 && more[1] < 0.4);
    }

    #[test]
    fn hue_should_turn_about_grey_and_come_full_circle() {
        close(one(Effect::Hue(120.0), [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        close(one(Effect::Hue(-120.0), [1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]);
        close(one(Effect::Hue(77.0), [0.3, 0.3, 0.3]), [0.3; 3]);
        close(
            one(Effect::Hue(180.0), [0.5, 0.2, 0.2]),
            one(Effect::Hue(-180.0), [0.5, 0.2, 0.2]),
        );
        let around = Effects::new(&[Effect::Hue(90.0); 4]);
        close(around.colour_of([0.7, 0.2, 0.1]), [0.7, 0.2, 0.1]);
    }

    #[test]
    fn tint_and_invert_should_mix_towards_their_target() {
        let red = Effect::Tint {
            colour: [1.0, 0.0, 0.0],
            amount: 1.0,
        };
        let grey = LUMA[0] * 0.2 + LUMA[1] * 0.4 + LUMA[2] * 0.6;
        close(one(red, [0.2, 0.4, 0.6]), [grey, 0.0, 0.0]);
        let none = Effect::Tint {
            colour: [1.0, 0.0, 0.0],
            amount: 0.0,
        };
        close(one(none, [0.2, 0.4, 0.6]), [0.2, 0.4, 0.6]);
        close(one(Effect::Invert(1.0), [0.2, 0.4, 1.0]), [0.8, 0.6, 0.0]);
        close(one(Effect::Invert(0.5), [0.2, 0.4, 1.0]), [0.5; 3]);
    }

    #[test]
    fn a_chain_should_apply_from_the_first_and_compose_into_one_matrix() {
        // Brightness then contrast is not contrast then brightness.
        let a = Effects::new(&[Effect::Brightness(0.2), Effect::Contrast(2.0)]);
        let b = Effects::new(&[Effect::Contrast(2.0), Effect::Brightness(0.2)]);
        let rgb = [0.3, 0.1, 0.2];
        let by_hand = |v: f32| ((v + 0.2 - GREY) * 2.0 + GREY).clamp(0.0, 1.0);
        close(a.colour_of(rgb), rgb.map(by_hand));
        let other = |v: f32| ((v - GREY) * 2.0 + GREY + 0.2).clamp(0.0, 1.0);
        close(b.colour_of(rgb), rgb.map(other));
        assert_ne!(a.colour, b.colour);
        // A longer chain, applied one effect at a time.
        let chain = [
            Effect::Saturation(1.5),
            Effect::Hue(40.0),
            Effect::Invert(0.3),
            Effect::Brightness(-0.1),
        ];
        let whole = Effects::new(&chain).colour;
        let mut step = [0.4, 0.3, 0.2];
        for effect in chain {
            let m = matrix(effect).unwrap();
            step = std::array::from_fn(|i| {
                m[i][0] * step[0] + m[i][1] * step[1] + m[i][2] * step[2] + m[i][3]
            });
        }
        let composed: [f32; 3] = std::array::from_fn(|i| {
            whole[i][0] * 0.4 + whole[i][1] * 0.3 + whole[i][2] * 0.2 + whole[i][3]
        });
        close(composed, step);
    }

    #[test]
    fn mirror_should_fold_one_half_onto_the_other() {
        let at = |axis, uv| Effects::new(&[Effect::Mirror(axis)]).source(uv);
        assert_eq!(at(Mirror::Horizontal, [0.75, 0.9]), Some([0.25, 0.9]));
        assert_eq!(at(Mirror::Horizontal, [0.25, 0.9]), Some([0.25, 0.9]));
        assert_eq!(at(Mirror::Vertical, [0.2, 0.75]), Some([0.2, 0.25]));
        assert_eq!(at(Mirror::Both, [0.75, 0.75]), Some([0.25, 0.25]));
    }

    #[test]
    fn zoom_should_scale_about_the_centre_and_leave_transparent_beyond() {
        let zoom = |amount| Effects::new(&[Effect::Zoom(amount)]);
        assert_eq!(zoom(2.0).source([0.5, 0.5]), Some([0.5, 0.5]));
        assert_eq!(zoom(2.0).source([0.75, 0.5]), Some([0.625, 0.5]));
        assert_eq!(zoom(0.5).source([0.75, 0.5]), Some([1.0, 0.5]));
        assert_eq!(zoom(0.5).source([0.9, 0.5]), None);
    }

    #[test]
    fn shift_should_move_the_picture_and_wrap_around() {
        let shift = Effects::new(&[Effect::Shift(0.25, -0.5)]);
        let [x, y] = shift.source([0.5, 0.5]).unwrap();
        assert!((x - 0.25).abs() < 1e-6 && (y - 0.0).abs() < 1e-6);
        let [x, y] = shift.source([0.1, 0.2]).unwrap();
        assert!((x - 0.85).abs() < 1e-6 && (y - 0.7).abs() < 1e-6);
    }

    #[test]
    fn steps_should_walk_back_from_the_last_effect() {
        // Mirror, then zoom out: the zoom's transparent border is not mirrored into the picture.
        let out = Effects::new(&[Effect::Mirror(Mirror::Horizontal), Effect::Zoom(0.5)]);
        assert_eq!(out.source([0.95, 0.5]), None);
        // Shift, then mirror: the mirror sees the shifted picture, so its centre line is the
        // shifted one's.
        let both = Effects::new(&[Effect::Shift(0.25, 0.0), Effect::Mirror(Mirror::Horizontal)]);
        let [x, _] = both.source([0.75, 0.5]).unwrap();
        assert!((x - 0.0).abs() < 1e-6, "{x}");
    }

    #[test]
    fn blurs_should_add_in_quadrature_up_to_a_full_blur() {
        let blur = |chain: &[Effect]| Effects::new(chain).blur;
        assert!((blur(&[Effect::Blur(0.3), Effect::Blur(0.4)]) - 0.5).abs() < 1e-6);
        assert_eq!(blur(&[Effect::Blur(1.0), Effect::Blur(1.0)]), 1.0);
        assert!(!Effects::new(&[Effect::Blur(0.1)]).is_identity());
    }

    #[test]
    fn the_blur_taps_should_lie_in_the_disc_without_repeating() {
        let taps: Vec<[f32; 2]> = (0..BLUR_TAPS).map(blur_tap).collect();
        assert!(taps.iter().all(|[x, y]| x.hypot(*y) < 1.0));
        for (i, a) in taps.iter().enumerate() {
            assert!(
                taps[i + 1..]
                    .iter()
                    .all(|b| (a[0] - b[0]).hypot(a[1] - b[1]) > 0.05)
            );
        }
    }

    #[test]
    fn steps_beyond_the_most_should_be_left_out() {
        let chain = vec![Effect::Zoom(1.0); MAX_STEPS + 3];
        assert_eq!(Effects::new(&chain).count as usize, MAX_STEPS);
    }
}
