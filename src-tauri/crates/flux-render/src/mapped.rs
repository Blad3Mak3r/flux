//! What a screen shows, drawn on the CPU for a monitor in the editor: the composition cut by the
//! screen's slices onto their corners and darkened by its soft edges and masks, as the GPU draws
//! the output window.

use crate::composite::tap;
use crate::{Mask, Quad, vertices};

fn ramp(t: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 1.0;
    }
    let s = (t / width).clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// Even-odd rule: whether `p` is inside the outline.
fn inside(points: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut odd = false;
    let mut j = points.len().saturating_sub(1);
    for (i, a) in points.iter().enumerate() {
        let b = points[j];
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// The `src` picture (`sw` x `sh` pixels of RGBA) drawn through `quads`, the first at the bottom,
/// then hidden by `masks`, over black into `width` x `height` pixels of RGBA.
pub fn map_screen(
    src: &[u8],
    (sw, sh): (usize, usize),
    quads: &[Quad],
    masks: &[Mask],
    (width, height): (usize, usize),
) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    for pixel in out.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    if src.len() < sw * sh * 4 || sw == 0 || sh == 0 || width == 0 || height == 0 {
        return out;
    }
    let sample = |u: f32, v: f32| {
        let (x, y) = (tap(u, sw), tap(v, sh));
        let at = |px: usize, py: usize, c: usize| f32::from(src[(py * sw + px) * 4 + c]);
        let mut rgb = [0.0; 3];
        for (c, v) in rgb.iter_mut().enumerate() {
            let top = at(x.a, y.a, c) * (1.0 - x.t) + at(x.b, y.a, c) * x.t;
            let bottom = at(x.a, y.b, c) * (1.0 - x.t) + at(x.b, y.b, c) * x.t;
            *v = top * (1.0 - y.t) + bottom * y.t;
        }
        rgb
    };
    for quad in quads {
        let v = vertices(quad);
        let soft = v[0].soft;
        for tri in [[v[0], v[1], v[2]], [v[3], v[4], v[5]]] {
            // Window coordinates in pixels.
            let p = tri.map(|t| {
                [
                    (t.pos[0] + 1.0) / 2.0 * width as f32,
                    (1.0 - t.pos[1]) / 2.0 * height as f32,
                ]
            });
            let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
            if area.abs() < 1e-6 {
                continue;
            }
            let min = |i: usize, hi: usize| {
                p.iter()
                    .map(|q| q[i])
                    .fold(f32::MAX, f32::min)
                    .floor()
                    .clamp(0.0, hi as f32) as usize
            };
            let max = |i: usize, hi: usize| {
                p.iter()
                    .map(|q| q[i])
                    .fold(f32::MIN, f32::max)
                    .ceil()
                    .clamp(0.0, hi as f32) as usize
            };
            for y in min(1, height)..max(1, height) {
                for x in min(0, width)..max(0, width) {
                    let c = [x as f32 + 0.5, y as f32 + 0.5];
                    let w1 = ((c[0] - p[0][0]) * (p[2][1] - p[0][1])
                        - (p[2][0] - p[0][0]) * (c[1] - p[0][1]))
                        / area;
                    let w2 = ((p[1][0] - p[0][0]) * (c[1] - p[0][1])
                        - (c[0] - p[0][0]) * (p[1][1] - p[0][1]))
                        / area;
                    let w0 = 1.0 - w1 - w2;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let mix = |f: fn(&crate::Vertex) -> [f32; 3]| {
                        let a = f(&tri[0]);
                        let b = f(&tri[1]);
                        let d = f(&tri[2]);
                        [0, 1, 2].map(|i| a[i] * w0 + b[i] * w1 + d[i] * w2)
                    };
                    let uvq = mix(|t| t.uvq);
                    let local = mix(|t| [t.local[0], t.local[1], 0.0]);
                    let q = uvq[2];
                    if q.abs() < 1e-9 {
                        continue;
                    }
                    let (l, u) = ([local[0] / q, local[1] / q], [uvq[0] / q, uvq[1] / q]);
                    let edge = ramp(l[0], soft[0])
                        * ramp(1.0 - l[0], soft[1])
                        * ramp(l[1], soft[2])
                        * ramp(1.0 - l[1], soft[3]);
                    if edge <= 0.0 {
                        continue;
                    }
                    let shown = edge.powf(1.0 / quad.gamma.max(0.01));
                    let rgb = sample(u[0], u[1]);
                    let at = (y * width + x) * 4;
                    for (k, value) in rgb.iter().enumerate() {
                        let below = f32::from(out[at + k]);
                        out[at + k] = (below * (1.0 - shown) + value * shown).round() as u8;
                    }
                }
            }
        }
    }
    for mask in masks.iter().filter(|m| m.points.len() >= 3) {
        for y in 0..height {
            for x in 0..width {
                let p = [
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                ];
                if inside(&mask.points, p) != mask.invert {
                    out[(y * width + x) * 4..][..3].fill(0);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fitted_quad;

    fn picture(w: usize, h: usize) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                [
                    (i * 7 % 256) as u8,
                    (i * 13 % 256) as u8,
                    (i * 3 % 256) as u8,
                    255,
                ]
            })
            .collect()
    }

    #[test]
    fn a_full_slice_should_reproduce_the_picture() {
        let src = picture(16, 9);
        let quad = fitted_quad((16, 9), (16, 9));
        let out = map_screen(&src, (16, 9), &[quad], &[], (16, 9));
        let worst = src
            .iter()
            .zip(&out)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 1, "{worst}");
    }

    #[test]
    fn a_picture_fitted_to_a_taller_screen_should_leave_black_bars() {
        let src = vec![200u8; 16 * 9 * 4];
        let quad = fitted_quad((16, 9), (16, 16));
        let out = map_screen(&src, (16, 9), &[quad], &[], (16, 16));
        assert_eq!(out[0], 0);
        assert_eq!(out[(8 * 16 + 8) * 4], 200);
    }

    #[test]
    fn a_mask_should_black_out_its_shape() {
        let src = vec![200u8; 8 * 8 * 4];
        let quad = fitted_quad((8, 8), (8, 8));
        let mask = Mask {
            points: vec![[0.0, 0.0], [0.5, 0.0], [0.5, 1.0], [0.0, 1.0]],
            invert: false,
        };
        let out = map_screen(&src, (8, 8), &[quad], &[mask], (8, 8));
        assert_eq!(out[(4 * 8 + 1) * 4], 0);
        assert_eq!(out[(4 * 8 + 6) * 4], 200);
    }
}
