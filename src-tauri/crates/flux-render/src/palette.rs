//! The colours of a test card, all made from one base colour in OKLCH (a space where equal steps
//! of lightness look equal): the cabinet checker, the grid lines, the accent of the circle, the
//! diagonals and the frame, the label plate and the text. Text is always checked against what it
//! sits on and pushed towards black or white until it reads at WCAG 4.5:1.

use flux_project::mapping::Scheme;

use crate::composite::{to_linear, to_srgb};

/// The smallest contrast ratio text on the card has against its background.
pub const MIN_CONTRAST: f32 = 4.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The two cabinet tones, alternating like a checker.
    pub cabinet: [[u8; 3]; 2],
    /// Lines on the cabinet boundaries.
    pub line: [u8; 3],
    /// The slice's outline and the cabinet corner marks.
    pub frame: [u8; 3],
    /// The circle, the diagonals and the crosshair.
    pub accent: [u8; 3],
    /// What the label sits on.
    pub plate: [u8; 3],
    /// Main text, readable on the plate.
    pub ink: [u8; 3],
    /// Secondary text, readable on the plate.
    pub muted: [u8; 3],
    /// The hue and chroma the palette came from, for text made later on other backgrounds.
    pub hue: f32,
    pub chroma: f32,
}

/// The colour a slice takes when its card names none: the hue turns by the golden angle with each
/// slice, so neighbours are told apart however many there are.
pub fn default_base(index: usize) -> [u8; 3] {
    let hue = (230.0 + index as f32 * 137.508) % 360.0;
    from_oklch(0.72, 0.13, hue)
}

pub fn palette(base: [u8; 3], scheme: Scheme) -> Palette {
    let (_, chroma, hue) = to_oklch(base);
    // A grey base still gets a trace of colour, and a vivid one is tamed so the checker stays quiet.
    let c = chroma.clamp(0.05, 0.16);
    let make = |l: f32, k: f32| from_oklch(l, c * k, hue);
    let (cabinet, line, frame, accent, plate) = match scheme {
        Scheme::Vivid => {
            let c = chroma.clamp(0.10, 0.17);
            let vivid = |l: f32, k: f32| from_oklch(l, c * k, hue);
            (
                [vivid(0.64, 1.0), vivid(0.52, 1.0)],
                vivid(0.82, 0.8),
                vivid(0.97, 0.10),
                vivid(0.98, 0.06),
                vivid(0.14, 0.30),
            )
        }
        Scheme::Dark => (
            [make(0.18, 0.30), make(0.235, 0.35)],
            make(0.50, 0.70),
            make(0.80, 1.0),
            make(0.80, 1.0),
            make(0.13, 0.25),
        ),
        Scheme::Light => (
            [make(0.95, 0.12), make(0.89, 0.18)],
            make(0.66, 0.60),
            make(0.46, 1.0),
            make(0.50, 1.0),
            make(0.98, 0.06),
        ),
        Scheme::Contrast => (
            [[0, 0, 0], [255, 255, 255]],
            make(0.62, 1.0),
            make(0.62, 1.0),
            make(0.70, 1.2),
            [0, 0, 0],
        ),
    };
    let ink = readable(plate, hue, 0.03);
    let muted = readable_at(
        plate,
        hue,
        c * 0.5,
        if lightness(plate) > 0.5 { 0.40 } else { 0.76 },
    );
    Palette {
        cabinet,
        line,
        frame,
        accent,
        plate,
        ink,
        muted,
        hue,
        chroma: c,
    }
}

/// Text colour for `background`: near white or near black in `hue`, whichever side has the room,
/// moved to the extreme until the contrast is at least [`MIN_CONTRAST`].
pub fn readable(background: [u8; 3], hue: f32, chroma: f32) -> [u8; 3] {
    let start = if lightness(background) > 0.6 {
        0.2
    } else {
        0.96
    };
    readable_at(background, hue, chroma, start)
}

pub fn readable_at(background: [u8; 3], hue: f32, chroma: f32, start: f32) -> [u8; 3] {
    let dark = start < 0.6;
    let mut l = start;
    loop {
        let ink = from_oklch(l, chroma, hue);
        if contrast(ink, background) >= MIN_CONTRAST {
            return ink;
        }
        l = if dark { l - 0.04 } else { l + 0.04 };
        if !(0.0..=1.0).contains(&l) {
            return if dark { [0, 0, 0] } else { [255, 255, 255] };
        }
    }
}

/// WCAG contrast ratio of two colours, 1 to 21.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn luminance(c: [u8; 3]) -> f32 {
    let [r, g, b] = c.map(|v| to_linear(v as f32 / 255.0));
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// One text colour readable on every one of `backgrounds`: near white or near black, whichever
/// reads better on the worst of them, moved to the extreme if it must.
pub fn readable_over(backgrounds: &[[u8; 3]], hue: f32, chroma: f32) -> [u8; 3] {
    let worst = |ink: [u8; 3]| {
        backgrounds
            .iter()
            .map(|b| contrast(ink, *b))
            .fold(f32::MAX, f32::min)
    };
    let mut best = [255, 255, 255];
    for (start, step) in [(0.90, 0.02), (0.20, -0.02)] {
        let mut l: f32 = start;
        loop {
            let ink = from_oklch(l, chroma, hue);
            if worst(ink) >= MIN_CONTRAST {
                if worst(ink) > worst(best) || worst(best) < MIN_CONTRAST {
                    best = ink;
                }
                break;
            }
            l += step;
            if !(0.0..=1.0).contains(&l) {
                break;
            }
        }
    }
    best
}

fn lightness(c: [u8; 3]) -> f32 {
    to_oklch(c).0
}

/// `(lightness 0..1, chroma, hue in degrees)` of an sRGB colour.
pub fn to_oklch(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|v| to_linear(v as f32 / 255.0));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let lab_l = 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s;
    let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
    let bb = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
    let hue = bb.atan2(a).to_degrees().rem_euclid(360.0);
    (lab_l, a.hypot(bb), hue)
}

/// The sRGB colour of an OKLCH one; a colour out of the gamut keeps its lightness and hue and
/// loses chroma until it fits.
pub fn from_oklch(lightness: f32, chroma: f32, hue: f32) -> [u8; 3] {
    let (mut low, mut high) = (0.0_f32, chroma.max(0.0));
    let rgb = |c: f32| {
        let (s, k) = hue.to_radians().sin_cos();
        let (a, b) = (c * k, c * s);
        let l = (lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m = (lightness - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s = (lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    };
    let inside = |v: [f32; 3]| v.iter().all(|x| (-0.0005..=1.0005).contains(x));
    let mut best = rgb(0.0);
    if inside(rgb(high)) {
        best = rgb(high);
    } else {
        for _ in 0..20 {
            let mid = (low + high) / 2.0;
            if inside(rgb(mid)) {
                low = mid;
                best = rgb(mid);
            } else {
                high = mid;
            }
        }
    }
    best.map(|v| (to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oklch_should_round_trip_and_know_white_and_black() {
        assert_eq!(from_oklch(1.0, 0.0, 0.0), [255, 255, 255]);
        assert_eq!(from_oklch(0.0, 0.0, 0.0), [0, 0, 0]);
        let (l, _, _) = to_oklch([255, 255, 255]);
        assert!((l - 1.0).abs() < 0.01);
        for c in [[200, 40, 90], [20, 160, 220], [90, 90, 90]] {
            let (l, ch, h) = to_oklch(c);
            let back = from_oklch(l, ch, h);
            for (a, b) in back.iter().zip(c) {
                assert!(a.abs_diff(b) <= 1, "{c:?} came back {back:?}");
            }
        }
    }

    #[test]
    fn the_vivid_palette_should_be_two_tones_of_the_base_with_white_guides() {
        let p = palette([40, 120, 230], Scheme::Vivid);
        let l = |c| to_oklch(c).0;
        assert!((l(p.cabinet[0]) - 0.64).abs() < 0.02 && (l(p.cabinet[1]) - 0.52).abs() < 0.02);
        assert!(l(p.accent) > 0.95);
        let (_, _, hue) = to_oklch([40, 120, 230]);
        assert!((to_oklch(p.cabinet[0]).2 - hue).abs() < 6.0);
    }

    #[test]
    fn a_palette_from_a_known_colour_should_be_the_same_every_time() {
        let a = palette([60, 140, 230], Scheme::Dark);
        assert_eq!(a, palette([60, 140, 230], Scheme::Dark));
        // The checker is dark, its two tones differ, and the lines and frame step up in light.
        let l = |c| to_oklch(c).0;
        assert!((l(a.cabinet[0]) - 0.18).abs() < 0.02);
        assert!(l(a.cabinet[1]) > l(a.cabinet[0]));
        assert!(l(a.line) > l(a.cabinet[1]) && l(a.frame) > l(a.line));
        // Everything keeps the hue of the base (blue here), but for the greys.
        let (_, _, hue) = to_oklch([60, 140, 230]);
        let (_, _, frame_hue) = to_oklch(a.frame);
        assert!((frame_hue - hue).abs() < 6.0, "{frame_hue} against {hue}");
    }

    #[test]
    fn text_should_read_at_4_5_to_1_on_every_scheme_and_base() {
        for scheme in [Scheme::Vivid, Scheme::Dark, Scheme::Light, Scheme::Contrast] {
            for base in [
                [255, 255, 0],
                [0, 0, 255],
                [128, 128, 128],
                [255, 0, 0],
                [0, 255, 255],
            ] {
                let p = palette(base, scheme);
                assert!(
                    contrast(p.ink, p.plate) >= MIN_CONTRAST,
                    "{scheme:?} {base:?} ink"
                );
                assert!(
                    contrast(p.muted, p.plate) >= MIN_CONTRAST,
                    "{scheme:?} {base:?} muted"
                );
                for tone in p.cabinet {
                    let text = readable(tone, p.hue, p.chroma * 0.5);
                    assert!(
                        contrast(text, tone) >= MIN_CONTRAST,
                        "{scheme:?} {base:?} on {tone:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn neighbouring_slices_should_get_different_hues() {
        let hues: Vec<f32> = (0..4).map(|i| to_oklch(default_base(i)).2).collect();
        for pair in hues.windows(2) {
            let gap = (pair[0] - pair[1]).abs();
            assert!(gap.min(360.0 - gap) > 40.0, "{hues:?}");
        }
    }
}
