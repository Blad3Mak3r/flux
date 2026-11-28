//! The test card of the output, one for each slice, drawn in the slice's own pixels so that it can
//! line up with the physical LED cabinets behind it: a checker of cabinets, one pixel lines on the
//! boundaries (the first pixel of each cabinet), the cabinet ids, a circle and the diagonals that
//! show a stretch, the slice's name and resolution and cabinet count in the middle, colour bars
//! and a grey ramp, and a moving part (a 2 pixel marker on a slim strip, a clock and a binary frame counter) that
//! shows a frozen or stuttering output. Or one flat colour, for dead pixel checks.
//!
//! The pattern is made once per size and settings ([`layout`]); each refresh only copies it and
//! draws the moving part ([`SliceCard::frame`]). Nothing here is random or reads the clock.

use std::sync::Arc;

use flux_project::mapping::{CardMode, Numbering, TestCard};
use flux_text::{Align, BUNDLED_FAMILY, Block};

use crate::palette::{self, Palette};

mod canvas;
mod motion;
mod pieces;

use canvas::*;
use motion::*;
use pieces::*;

#[cfg(test)]
mod tests;

/// Seconds the marker takes to cross its strip.
pub const SWEEP_SECONDS: f32 = 3.0;

/// Frames a second the counter counts.
const COUNTER_RATE: f32 = 60.0;

/// The logo's picture, decoded: straight RGBA, `width` by `height`. It is the same logo as long
/// as its `id` (the project's asset) and its pixels (the same allocation) are.
#[derive(Debug, Clone)]
pub struct Logo {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl PartialEq for Logo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Arc::ptr_eq(&self.rgba, &other.rgba)
    }
}

/// A slice of the mapping as the card marks it: its name, `[x, y, width, height]` of the picture
/// (each 0 to 1), its settings and the logo they name, if it could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct CardSlice {
    pub name: String,
    pub input: [f32; 4],
    pub card: TestCard,
    pub logo: Option<Logo>,
}

/// The card of one slice, as pixels of the composition.
pub struct SliceCard {
    /// `[x, y, width, height]` in composition pixels.
    pub rect: [u32; 4],
    /// RGBA, `rect`'s width by its height, without the moving part.
    pub fixed: Vec<u8>,
    motion: Option<Motion>,
}

impl SliceCard {
    /// The card with its moving part at `seconds` into a minute and `clock` (`HH:MM:SS`) in `out`;
    /// `false` when it does not move (`out` is left alone, use [`Self::fixed`]).
    pub fn frame(&self, out: &mut Vec<u8>, seconds: f32, clock: &str) -> bool {
        let Some(motion) = &self.motion else {
            return false;
        };
        out.clear();
        out.extend_from_slice(&self.fixed);
        motion.draw(out, self.rect[2], self.rect[3], seconds, clock);
        true
    }
}

/// The pixels a slice's rectangle covers in a composition: whole pixels, at least one each way.
pub fn pixel_rect(input: [f32; 4], composition: (u32, u32)) -> [u32; 4] {
    let edge = |from: f32, side: f32, of: u32| {
        let a = (from * of as f32).round().clamp(0.0, of as f32) as u32;
        let b = ((from + side) * of as f32).round().clamp(0.0, of as f32) as u32;
        (a.min(of.saturating_sub(1)), b.max(a + 1).min(of))
    };
    let (x0, x1) = edge(input[0], input[2], composition.0);
    let (y0, y1) = edge(input[1], input[3], composition.1);
    [x0, y0, x1 - x0, y1 - y0]
}

/// The cards of `slices` in a composition of `composition` pixels; `caption` (what the screen is
/// called and its size) is written under the cabinet count.
pub fn layout(slices: &[CardSlice], composition: (u32, u32), caption: &str) -> Vec<SliceCard> {
    slices
        .iter()
        .enumerate()
        .map(|(index, slice)| {
            let rect = pixel_rect(slice.input, composition);
            let (fixed, motion) = render(slice, index, [rect[2], rect[3]], caption);
            SliceCard {
                rect,
                fixed,
                motion,
            }
        })
        .collect()
}

/// `cards` laid on a picture `width` pixels across (a box average, so a pixel line is not lost),
/// as `[width, height]` and RGBA: opaque where a card is, clear elsewhere.
pub fn preview(cards: &[SliceCard], composition: (u32, u32), width: u32) -> (u32, u32, Vec<u8>) {
    let width = width.clamp(1, composition.0);
    let height = ((composition.1 as u64 * width as u64) / composition.0 as u64).max(1) as u32;
    let mut sums = vec![[0u32; 4]; (width * height) as usize];
    for card in cards {
        let [x0, y0, w, h] = card.rect;
        // The moving part as it is at zero, so the clock has digits.
        let mut moving = Vec::new();
        let pixels = if card.frame(&mut moving, 0.0, "00:00:00") {
            &moving
        } else {
            &card.fixed
        };
        for y in 0..h {
            let dy = (u64::from(y0 + y) * u64::from(height) / u64::from(composition.1)) as u32;
            for x in 0..w {
                let dx = (u64::from(x0 + x) * u64::from(width) / u64::from(composition.0)) as u32;
                let at = ((y * w + x) * 4) as usize;
                let sum = &mut sums[(dy * width + dx) as usize];
                for (sum, byte) in sum.iter_mut().zip(&pixels[at..at + 3]) {
                    *sum += u32::from(*byte);
                }
                sum[3] += 1;
            }
        }
    }
    let rgba = sums
        .iter()
        .flat_map(|s| {
            let n = s[3].max(1);
            [
                (s[0] / n) as u8,
                (s[1] / n) as u8,
                (s[2] / n) as u8,
                if s[3] > 0 { 255 } else { 0 },
            ]
        })
        .collect();
    (width, height, rgba)
}

/// The letters of column `n` from 0: A to Z, then AA.
fn letters(mut n: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// The id of the cabinet at `column`, `row` (from 0) in a grid `columns` wide.
pub fn cabinet_id(numbering: Numbering, column: u32, row: u32, columns: u32) -> String {
    match numbering {
        Numbering::Alpha => format!("{}{}", letters(column), row + 1),
        Numbering::Dotted => format!("{}.{}", column + 1, row + 1),
        Numbering::Sequence => format!("{:02}", row * columns + column + 1),
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing
