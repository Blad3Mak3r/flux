use super::*;

/// `a` then `b`, as one matrix.
pub(super) fn then(a: &Colour, b: &Colour) -> Colour {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let through = (0..3).map(|k| b[i][k] * a[k][j]).sum::<f32>();
            through + if j == 3 { b[i][3] } else { 0.0 }
        })
    })
}

pub(super) fn linear(a: [[f32; 3]; 3], b: [f32; 3]) -> Colour {
    std::array::from_fn(|i| [a[i][0], a[i][1], a[i][2], b[i]])
}

pub(super) fn scale_and_add(scale: f32, add: [f32; 3]) -> Colour {
    linear(
        [[scale, 0.0, 0.0], [0.0, scale, 0.0], [0.0, 0.0, scale]],
        add,
    )
}

/// `keep` of each colour, and the rest of the grey scaled by `towards`.
pub(super) fn grey_mix(keep: f32, towards: [f32; 3]) -> Colour {
    let a = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            (if i == j { keep } else { 0.0 }) + (1.0 - keep) * towards[i] * LUMA[j]
        })
    });
    linear(a, [0.0; 3])
}

/// The linear light of `#rrggbb`; white when it is not.
pub(super) fn light(colour: &str) -> [f32; 3] {
    rgb(colour)
        .unwrap_or([255; 3])
        .map(|c| to_linear(f32::from(c) / 255.0))
}

/// The matrix of a colour effect that is affine on the picture's light, or `None` for the others.
pub(super) fn matrix(kind: &EffectKind) -> Option<Colour> {
    Some(match kind {
        EffectKind::Brightness { amount } => scale_and_add(1.0, [*amount; 3]),
        EffectKind::Contrast { amount } => scale_and_add(*amount, [(1.0 - amount) * GREY; 3]),
        EffectKind::Saturation { amount } => grey_mix(*amount, [1.0; 3]),
        EffectKind::Hue { degrees } => {
            let (sin, cos) = degrees.to_radians().sin_cos();
            // A turn about (1, 1, 1): cos of the colour, the rest of the grey, and the cross
            // product with the axis.
            let third = (1.0 - cos) / 3.0;
            let side = sin / 3.0_f32.sqrt();
            linear(
                [
                    [cos + third, third - side, third + side],
                    [third + side, cos + third, third - side],
                    [third - side, third + side, cos + third],
                ],
                [0.0; 3],
            )
        }
        EffectKind::Tint { colour, amount } => grey_mix(1.0 - amount, light(colour)),
        EffectKind::Invert { amount } => scale_and_add(1.0 - 2.0 * amount, [*amount; 3]),
        _ => return None,
    })
}

/// How many cycles of an effect have passed at `time`.
pub(super) fn cycles(pace: &Pace, time: Time) -> f64 {
    match pace.sync {
        Sync::Free => time.seconds * f64::from(pace.rate),
        Sync::Beats => time.beats / f64::from(pace.beats.max(0.0625)),
    }
}
