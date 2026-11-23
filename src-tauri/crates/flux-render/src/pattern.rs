//! The test card shown on the output to line the slices of a mapping up on the real screen: a grid
//! of cells, the diagonals and circles that show how a slice leans, colour bars and a grey ramp to
//! check the picture, the corners told apart by colour (red top left, green top right, blue bottom
//! right, yellow bottom left), and a white bar that sweeps across it so every slice shows where
//! its part of the picture moves.

/// Cells across and down, as a 16:9 picture's.
const COLUMNS: u32 = 16;
const ROWS: u32 = 9;
/// Seconds the sweep takes to cross the picture.
pub const SWEEP_SECONDS: f32 = 3.0;

const BARS: [[u8; 3]; 8] = [
    [235, 235, 235],
    [235, 235, 16],
    [16, 235, 235],
    [16, 235, 16],
    [235, 16, 235],
    [235, 16, 16],
    [16, 16, 235],
    [16, 16, 16],
];

/// The card without the sweep: `width * height` RGBA pixels, rows top to bottom.
pub fn test_card(width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    let cell = (w / COLUMNS as f32, h / ROWS as f32);
    let line = (h / 540.0).max(1.0);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let corner = (w / 10.0, h / 6.0);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (column, row) = ((fx / cell.0) as u32, (fy / cell.1) as u32);
            // The cells alternate two greys, tinted by where they are, so a cell out of place shows.
            let shade = if (column + row) % 2 == 0 { 34.0 } else { 46.0 };
            let tint = |amount: f32| (shade + amount * 18.0) as u8;
            let mut colour = [
                tint(fx / w),
                tint(1.0 - (fx / w - fy / h).abs()),
                tint(fy / h),
            ];

            let in_corner = |left: bool, top: bool| {
                let dx = if left { fx } else { w - fx };
                let dy = if top { fy } else { h - fy };
                dx / corner.0 + dy / corner.1 < 1.0
            };
            for (left, top, paint) in [
                (true, true, [225, 45, 45]),
                (false, true, [45, 200, 70]),
                (false, false, [55, 95, 235]),
                (true, false, [235, 200, 40]),
            ] {
                if in_corner(left, top) {
                    colour = paint;
                }
            }

            // Colour bars and a grey ramp across the middle of the third row from the bottom.
            let band = (h * 0.62, h * 0.70, h * 0.74);
            let bars = (w * 0.25, w * 0.75);
            if fx >= bars.0 && fx < bars.1 && fy >= band.0 && fy < band.2 {
                let along = (fx - bars.0) / (bars.1 - bars.0);
                colour = if fy < band.1 {
                    BARS[((along * BARS.len() as f32) as usize).min(BARS.len() - 1)]
                } else {
                    let grey = (along * 255.0) as u8;
                    [grey, grey, grey]
                };
            }

            let near = |distance: f32, width: f32| distance.abs() < width / 2.0;
            let on_grid = near(fx - (column as f32 * cell.0), line * 2.0)
                || near(fy - (row as f32 * cell.1), line * 2.0);
            let on_frame =
                fx < line * 4.0 || fy < line * 4.0 || fx > w - line * 4.0 || fy > h - line * 4.0;
            let on_centre = near(fx - cx, line * 3.0) || near(fy - cy, line * 3.0);
            // The diagonals, measured across them: they cross at the centre and bend if a slice does.
            let diagonal =
                |sign: f32| (fy - cy - sign * (fx - cx) * h / w) * w / (w * w + h * h).sqrt();
            let on_diagonals = near(diagonal(1.0), line * 2.0) || near(diagonal(-1.0), line * 2.0);
            let radius = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let on_circles =
                near(radius - h * 0.45, line * 3.0) || near(radius - h * 0.22, line * 2.0);
            if on_frame || on_centre || on_circles {
                colour = [245, 245, 245];
            } else if on_grid || on_diagonals {
                colour = [190, 190, 190];
            }
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    rgba
}

/// Copies `card` into `out` with the sweep at `seconds` since it started: a white bar crossing
/// from left to right, with a soft trail, every [`SWEEP_SECONDS`].
pub fn sweep(card: &[u8], out: &mut Vec<u8>, width: u32, height: u32, seconds: f32) {
    out.clear();
    out.extend_from_slice(card);
    let w = width as f32;
    let core = (w / 160.0).max(2.0);
    let trail = w / 12.0;
    // The bar starts off the left edge and leaves off the right one, trail and all.
    let travel = w + core + trail;
    let at = (seconds / SWEEP_SECONDS).fract() * travel - core;
    let first = (at - trail).max(0.0) as u32;
    let last = ((at + core).ceil() as u32).min(width);
    for x in first..last {
        let behind = at - x as f32;
        let amount = if behind <= 0.0 {
            1.0
        } else {
            (1.0 - behind / trail).max(0.0).powi(2)
        };
        if amount <= 0.0 {
            continue;
        }
        for y in 0..height {
            let at = ((y * width + x) * 4) as usize;
            for channel in &mut out[at..at + 3] {
                *channel = (*channel as f32 + (255.0 - *channel as f32) * amount) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * width + x) * 4) as usize;
        [rgba[at], rgba[at + 1], rgba[at + 2]]
    }

    #[test]
    fn the_corners_should_be_told_apart_by_colour() {
        let (w, h) = (320, 180);
        let card = test_card(w, h);
        assert_eq!(card.len(), (w * h * 4) as usize);
        assert_eq!(pixel(&card, w, 8, 8), [225, 45, 45]);
        assert_eq!(pixel(&card, w, w - 9, 8), [45, 200, 70]);
        assert_eq!(pixel(&card, w, w - 9, h - 9), [55, 95, 235]);
        assert_eq!(pixel(&card, w, 8, h - 9), [235, 200, 40]);
    }

    #[test]
    fn the_sweep_should_whiten_where_it_is_and_leave_the_rest() {
        let (w, h) = (320, 180);
        let card = test_card(w, h);
        let mut out = Vec::new();
        // Halfway through, the bar is just past the middle of its travel.
        sweep(&card, &mut out, w, h, SWEEP_SECONDS / 2.0);
        let core = (w as f32 / 160.0).max(2.0);
        let middle = ((w as f32 + core + w as f32 / 12.0) / 2.0 - core).ceil() as u32;
        assert_eq!(pixel(&out, w, middle, h / 3), [255, 255, 255]);
        assert_eq!(
            pixel(&out, w, w - 20, h / 3),
            pixel(&card, w, w - 20, h / 3)
        );
        assert_eq!(out.len(), card.len());
    }
}
