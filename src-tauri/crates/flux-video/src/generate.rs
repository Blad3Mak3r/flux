//! The pictures of the VJ grid's generators, made on the CPU. They are small and stretched over the
//! composition when drawn, so making one costs little; a picture that does not change keeps its
//! index, so the screens upload it once.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use flux_project::generator::{Generator, rgb};

use crate::decode::Frame;

/// The size of a gradient's picture: the shape of a 16:9 composition, so its angle is the one drawn.
const GRADIENT: (u32, u32) = (256, 144);
/// The size of the noise's picture.
const NOISE: (u32, u32) = (320, 180);
/// The width of each colour bar: wide enough for the edges between bars to stay sharp when drawn.
const BAR: u32 = 256;

/// The colours of the bars, left to right: white, yellow, cyan, green, magenta, red and blue.
const BARS: [[u8; 3]; 7] = [
    [255, 255, 255],
    [255, 255, 0],
    [0, 255, 255],
    [0, 255, 0],
    [255, 0, 255],
    [255, 0, 0],
    [0, 0, 255],
];

/// Which picture `generator` shows `elapsed` after it was launched: the same index for the same
/// picture, and a new one each time it changes. `None` between a strobe's flashes, when it shows
/// nothing.
pub fn index(generator: &Generator, elapsed: Duration) -> Option<u64> {
    let step = |rate: f32| (elapsed.as_secs_f64() * f64::from(rate)) as u64;
    let phase = match *generator {
        Generator::Strobe { rate, width, .. } => {
            let at = elapsed.as_secs_f64() * f64::from(rate);
            if at.fract() >= f64::from(width) {
                return None;
            }
            // Every flash is the same picture.
            0
        }
        Generator::Noise { rate, .. } => step(rate),
        _ => 0,
    };
    let mut hash = DefaultHasher::new();
    format!("{generator:?}").hash(&mut hash);
    phase.hash(&mut hash);
    Some(hash.finish())
}

/// The picture `generator` shows for `index` (from [`index`]), opaque.
pub fn picture(generator: &Generator, index: u64) -> Frame {
    let colour = |hex: &str| rgb(hex).unwrap_or([0, 0, 0]);
    let (width, height, rgb) = match generator {
        Generator::Solid { colour: c } | Generator::Strobe { colour: c, .. } => {
            (1, 1, colour(c).to_vec())
        }
        Generator::Gradient { from, to, angle } => {
            let (w, h) = GRADIENT;
            (w, h, gradient(colour(from), colour(to), *angle, (w, h)))
        }
        Generator::Bars => {
            let row = BARS
                .iter()
                .flat_map(|bar| std::iter::repeat_n(*bar, BAR as usize))
                .flatten()
                .collect();
            (BAR * BARS.len() as u32, 1, row)
        }
        Generator::Noise { colour: c, .. } => {
            let (w, h) = NOISE;
            (w, h, noise(colour(c), index, (w, h)))
        }
    };
    let rgba = rgb
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    Frame {
        index,
        width,
        height,
        rgba,
        alpha: false,
    }
}

/// From `from` to `to` along `angle` degrees over a picture of `size`, as RGB.
fn gradient(from: [u8; 3], to: [u8; 3], angle: f32, size: (u32, u32)) -> Vec<u8> {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let (sin, cos) = angle.to_radians().sin_cos();
    // Half the length of the picture along the direction, so the ends are its corners.
    let reach = ((w * cos).abs() + (h * sin).abs()) / 2.0;
    let mut out = Vec::with_capacity((size.0 * size.1 * 3) as usize);
    for y in 0..size.1 {
        for x in 0..size.0 {
            let along = (x as f32 + 0.5 - w / 2.0) * cos + (y as f32 + 0.5 - h / 2.0) * sin;
            let t = (along / reach.max(f32::EPSILON) + 1.0) / 2.0;
            out.extend((0..3).map(|c| {
                (f32::from(from[c]) + (f32::from(to[c]) - f32::from(from[c])) * t)
                    .round()
                    .clamp(0.0, 255.0) as u8
            }));
        }
    }
    out
}

/// Random snow of `colour` over a picture of `size`, as RGB: the same for the same `seed`.
fn noise(colour: [u8; 3], seed: u64, size: (u32, u32)) -> Vec<u8> {
    // xorshift64*: fast, and never stuck at zero.
    let mut state = seed | 1;
    let mut out = Vec::with_capacity((size.0 * size.1 * 3) as usize);
    for _ in 0..size.0 * size.1 {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let level = (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u16;
        out.extend(colour.map(|c| (u16::from(c) * level / 255) as u8));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn a_solid_colour_should_be_one_pixel_that_never_changes() {
        let solid = Generator::Solid {
            colour: "#ff8000".into(),
        };
        let index = index(&solid, at(0)).unwrap();
        assert_eq!(super::index(&solid, at(5000)), Some(index));
        let frame = picture(&solid, index);
        assert_eq!((frame.width, frame.height), (1, 1));
        assert_eq!(frame.rgba, [255, 128, 0, 255]);
        let other = Generator::Solid {
            colour: "#ff8001".into(),
        };
        assert_ne!(
            super::index(&other, at(0)),
            Some(index),
            "new colour, new upload"
        );
    }

    #[test]
    fn a_strobe_should_flash_for_its_width_of_each_period() {
        let strobe = Generator::Strobe {
            colour: "#ffffff".into(),
            rate: 4.0,
            width: 0.25,
        };
        // A period of 250 ms: on for the first 62.5 ms of it.
        assert!(index(&strobe, at(0)).is_some());
        assert!(index(&strobe, at(60)).is_some());
        assert!(index(&strobe, at(100)).is_none());
        assert!(index(&strobe, at(260)).is_some());
    }

    #[test]
    fn a_gradient_should_run_from_one_colour_to_the_other() {
        let left_to_right = gradient([0, 0, 0], [255, 255, 255], 0.0, (4, 1));
        let reds: Vec<u8> = left_to_right.chunks(3).map(|p| p[0]).collect();
        assert!(reds.windows(2).all(|w| w[0] < w[1]), "{reds:?}");
        let top_to_bottom = gradient([0, 0, 0], [255, 0, 0], 90.0, (1, 4));
        assert!(top_to_bottom[0] < top_to_bottom[9]);
    }

    #[test]
    fn noise_should_change_with_its_rate() {
        let noise = Generator::Noise {
            colour: "#ffffff".into(),
            rate: 10.0,
        };
        let first = index(&noise, at(0));
        assert_eq!(index(&noise, at(90)), first);
        assert_ne!(index(&noise, at(110)), first);
        let a = picture(&noise, first.unwrap());
        assert_eq!(a.rgba.len(), (NOISE.0 * NOISE.1 * 4) as usize);
        assert!(
            a.rgba.chunks(4).any(|p| p[0] != a.rgba[0]),
            "not one flat grey"
        );
    }

    #[test]
    fn the_bars_should_have_sharp_edges() {
        let bars = picture(&Generator::Bars, 0);
        assert_eq!(bars.width, BAR * 7);
        let red = |x: u32| bars.rgba[(x * 4) as usize];
        assert_eq!((red(BAR - 1), red(BAR * 3)), (255, 0), "white then cyan");
    }
}
