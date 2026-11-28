//! The effects of a picture, as the GPU and the CPU monitor both draw them.
//!
//! A stack of effects (`flux_project::vj_effects`), with the time it is drawn at, becomes one
//! [`Effects`]: a short program that the fragment shader interprets (`shader.wgsl`) and the
//! monitor interprets on the CPU ([`Effects::walk`], [`Effects::sample`], [`Effects::colour_of`]),
//! step for step the same. Each step is a kind and twelve numbers, and the program runs in three
//! stages, each keeping the order of the stack:
//!
//! 1. **Where.** The geometric steps (mirror, zoom, shift, rotate, kaleidoscope, polar, wave, twirl,
//!    bulge, tile, pixelate, displace, shift glitch, twitch, roll) walk backwards from the last to
//!    the first, mapping the coordinate being drawn to the coordinate the picture is read at.
//! 2. **How.** The picture is read at that coordinate: plain, or smeared by the blurs (a blur, a
//!    radial blur and a zoom blur share one pass of taps, and their displacements add), with its
//!    red and blue pulled apart by the RGB split and the glitches' colour offsets, and with the
//!    edge detect looking at the result. Their order does not matter: they combine.
//! 3. **What colour.** The colour steps apply from the first to the last, on linear light.
//!    Consecutive affine ones (brightness, contrast, saturation, hue, tint, invert, a lit strobe, a
//!    twitch's flash) fold into one matrix; posterize, threshold, colorize, solarize, levels,
//!    vignette, noise and scanlines run in their place.
//!
//! The effects of the stages do not interleave: a blur below a threshold in the stack still blurs
//! first. Everything that moves with time is worked out here, per frame, from the seconds since the
//! clip was launched and the beats of the show's tempo (`Time`), so the shader only ever sees
//! numbers; its randomness is a hash of the effect's seed, the step of time and the pixel or the
//! row ([`hash3`]), never a generator, so a frame is the same wherever and whenever it is drawn.

use flux_project::generator::rgb;
use flux_project::vj_effects::{EdgeMode, Effect, EffectKind, MirrorAxis, Pace, StrobeMode, Sync};

use crate::composite::to_linear;

mod builder;
mod hash;
mod matrix;
mod walk;

use builder::*;
pub use hash::*;
use matrix::*;
pub use walk::*;

#[cfg(test)]
mod tests;

/// Most steps a picture's program holds: two full stacks of 16, one step each. A twitch can take two
/// (its jump and its flash) and the blurs together one, so a program past this leaves its last out.
pub const MAX_STEPS: usize = 32;
/// The numbers of one step in the shader: its kind, seed, phase and index, then its settings.
pub const STEP_FLOATS: usize = 16;
/// The floats of one picture's program in the shader: a header and the steps.
pub(crate) const FX_FLOATS: usize = 4 + STEP_FLOATS * MAX_STEPS;
/// The taps the GPU's blur pass averages: points of a disc, evenly spread.
pub const BLUR_TAPS: usize = 32;
/// The taps the monitor averages: half, a cheaper approximation of the same pass.
pub const CPU_TAPS: usize = 16;
/// The radius of a full blur, as a fraction of the picture's short side.
pub const MAX_BLUR: f32 = 0.05;
/// Mid grey as linear light (sRGB 0.5), which contrast pivots about.
const GREY: f32 = 0.214;
/// How much each colour counts in a grey (Rec. 709, for linear light).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const TAU: f32 = std::f32::consts::TAU;
/// How far an RGB split at full amount pulls the colours apart, as a fraction of the height.
const SPLIT_REACH: f32 = 0.05;
/// The most steps of time a cycle count can name before it starts again; f32 holds it exactly.
const INDEX_RANGE: f64 = 16_777_216.0;

// The kinds of step, as the shader knows them.
pub const MIRROR_HORIZONTAL: u32 = 0;
pub const MIRROR_VERTICAL: u32 = 1;
pub const MIRROR_BOTH: u32 = 2;
pub const ZOOM: u32 = 3;
pub const TRANSLATE: u32 = 4;
pub const ROTATE: u32 = 5;
pub const KALEIDOSCOPE: u32 = 6;
pub const POLAR: u32 = 7;
pub const WAVE: u32 = 8;
pub const TWIRL: u32 = 9;
pub const BULGE: u32 = 10;
pub const TILE: u32 = 11;
pub const PIXELATE: u32 = 12;
pub const DISPLACE: u32 = 13;
pub const GLITCH: u32 = 14;
pub const TRANSFORM: u32 = 15;
pub const ROLL: u32 = 16;
pub const BLUR: u32 = 20;
pub const RADIAL: u32 = 21;
pub const ZOOM_BLUR: u32 = 22;
pub const SPLIT: u32 = 23;
pub const EDGE: u32 = 24;
pub const MATRIX: u32 = 30;
pub const POSTERIZE: u32 = 31;
pub const THRESHOLD: u32 = 32;
pub const COLORIZE: u32 = 33;
pub const SOLARIZE: u32 = 34;
pub const LEVELS: u32 = 35;
pub const VIGNETTE: u32 = 36;
pub const NOISE: u32 = 37;
pub const SCANLINES: u32 = 38;

/// When a picture is drawn: the seconds since its clip was launched and the beats of the show's
/// tempo since its origin.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Time {
    pub seconds: f64,
    pub beats: f64,
}

/// A colour matrix: each row gives a new red, green or blue as its first three numbers times
/// the old red, green and blue, plus the fourth.
pub type Colour = [[f32; 4]; 3];

const IDENTITY: Colour = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// One step of a program: what it is, the seed of its randomness, where in its cycle it is (a
/// fraction, and the count of whole cycles before) and its settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub kind: u32,
    pub seed: u32,
    pub phase: f32,
    pub index: u32,
    pub p: [f32; 12],
}

impl Step {
    const NONE: Self = Self {
        kind: 0,
        seed: 0,
        phase: 0.0,
        index: 0,
        p: [0.0; 12],
    };

    fn new(kind: u32, seed: u32, p: &[f32]) -> Self {
        let mut step = Self {
            kind,
            seed,
            ..Self::NONE
        };
        step.p[..p.len()].copy_from_slice(p);
        step
    }

    /// At `cycles` whole and fractional cycles.
    fn at(mut self, cycles: f64) -> Self {
        let whole = cycles.floor();
        self.phase = (cycles - whole) as f32;
        self.index = whole.rem_euclid(INDEX_RANGE) as u32;
        self
    }
}

/// What a picture's effects come to, ready for drawing: the steps of the three stages, one after
/// the other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effects {
    pub steps: [Step; MAX_STEPS],
    /// How many steps each stage has, in the order of the stages.
    pub uv: u32,
    pub sample: u32,
    pub colour: u32,
}

impl Default for Effects {
    /// No effect: the picture as it is.
    fn default() -> Self {
        Self {
            steps: [Step::NONE; MAX_STEPS],
            uv: 0,
            sample: 0,
            colour: 0,
        }
    }
}
