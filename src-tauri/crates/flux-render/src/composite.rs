//! The composition drawn on the CPU, small, for a monitor in the editor: the same pictures at the
//! same places, opacities and blends the GPU draws on the screens, before their mapping.

use crate::{Blend, Picture};

/// Draws `pictures`, the first at the bottom, over black into `width` x `height` pixels of 8-bit
/// sRGB RGBA. Each output pixel takes the nearest pixel of each picture, and the light is mixed in
/// linear light as the GPU mixes it.
pub fn composite(pictures: &[Picture], width: u32, height: u32) -> Vec<u8> {
    let linear: Vec<f32> = (0..=255u8)
        .map(|v| to_linear(f32::from(v) / 255.0))
        .collect();
    let (w, h) = (width as usize, height as usize);
    let mut light = vec![[0.0f32; 3]; w * h];
    for picture in pictures {
        let [px, py, pw, ph] = picture.place;
        if pw <= 0.0 || ph <= 0.0 || picture.opacity <= 0.0 {
            continue;
        }
        let (sw, sh) = (picture.width as usize, picture.height as usize);
        if picture.rgba.len() < sw * sh * 4 {
            continue;
        }
        // The rows and columns of the output the picture covers.
        let span = |start: f32, size: f32, pixels: usize| {
            let from = ((start * pixels as f32).floor().max(0.0) as usize).min(pixels);
            let to = (((start + size) * pixels as f32).ceil().max(0.0) as usize).min(pixels);
            from..to
        };
        for y in span(py, ph, h) {
            let v = ((y as f32 + 0.5) / h as f32 - py) / ph;
            if !(0.0..1.0).contains(&v) {
                continue;
            }
            let row = ((v * sh as f32) as usize).min(sh - 1) * sw;
            for x in span(px, pw, w) {
                let u = ((x as f32 + 0.5) / w as f32 - px) / pw;
                if !(0.0..1.0).contains(&u) {
                    continue;
                }
                let at = (row + ((u * sw as f32) as usize).min(sw - 1)) * 4;
                let src = &picture.rgba[at..at + 4];
                let cover = f32::from(src[3]) / 255.0 * picture.opacity.min(1.0);
                let colour = [
                    linear[usize::from(src[0])],
                    linear[usize::from(src[1])],
                    linear[usize::from(src[2])],
                ];
                let dst = &mut light[y * w + x];
                for c in 0..3 {
                    dst[c] = mix(picture.blend, colour[c], cover, dst[c]).clamp(0.0, 1.0);
                }
            }
        }
    }
    light
        .iter()
        .flat_map(|[r, g, b]| [to_srgb(*r), to_srgb(*g), to_srgb(*b), 255])
        .collect()
}

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

fn to_linear(v: f32) -> f32 {
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
}
