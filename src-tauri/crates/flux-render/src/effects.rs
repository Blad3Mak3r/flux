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

/// Most steps a picture's program holds: two full stacks of 16.
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

/// `a` then `b`, as one matrix.
fn then(a: &Colour, b: &Colour) -> Colour {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let through = (0..3).map(|k| b[i][k] * a[k][j]).sum::<f32>();
            through + if j == 3 { b[i][3] } else { 0.0 }
        })
    })
}

fn linear(a: [[f32; 3]; 3], b: [f32; 3]) -> Colour {
    std::array::from_fn(|i| [a[i][0], a[i][1], a[i][2], b[i]])
}

fn scale_and_add(scale: f32, add: [f32; 3]) -> Colour {
    linear(
        [[scale, 0.0, 0.0], [0.0, scale, 0.0], [0.0, 0.0, scale]],
        add,
    )
}

/// `keep` of each colour, and the rest of the grey scaled by `towards`.
fn grey_mix(keep: f32, towards: [f32; 3]) -> Colour {
    let a = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            (if i == j { keep } else { 0.0 }) + (1.0 - keep) * towards[i] * LUMA[j]
        })
    });
    linear(a, [0.0; 3])
}

/// The linear light of `#rrggbb`; white when it is not.
fn light(colour: &str) -> [f32; 3] {
    rgb(colour)
        .unwrap_or([255; 3])
        .map(|c| to_linear(f32::from(c) / 255.0))
}

/// The matrix of a colour effect that is affine on the picture's light, or `None` for the others.
fn matrix(kind: &EffectKind) -> Option<Colour> {
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
fn cycles(pace: &Pace, time: Time) -> f64 {
    match pace.sync {
        Sync::Free => time.seconds * f64::from(pace.rate),
        Sync::Beats => time.beats / f64::from(pace.beats.max(0.0625)),
    }
}

/// A seed from an effect's id, small enough for an f32 to hold exactly.
pub fn seed_of(id: &str) -> u32 {
    // FNV-1a.
    let hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash & 0x00ff_ffff
}

/// A well mixed number from `x` (the "lowbias32" hash).
pub fn lowbias(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A well mixed number from three: the seed of an effect, a step of time, a pixel or a row.
pub fn hash3(a: u32, b: u32, c: u32) -> u32 {
    lowbias(a ^ lowbias(b ^ lowbias(c)))
}

/// 0 up to but not including 1 from a hash.
pub fn unit(hash: u32) -> f32 {
    (hash >> 8) as f32 / 16_777_216.0
}

/// Smooth noise of `p`, 0 to 1: the hashes of the corners of its cell, blended.
pub fn value_noise(p: [f32; 2], seed: u32) -> f32 {
    let (ix, iy) = (p[0].floor(), p[1].floor());
    let (fx, fy) = (p[0] - ix, p[1] - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let corner = |x: f32, y: f32| unit(hash3(seed, x as i32 as u32, y as i32 as u32));
    let top = mix(corner(ix, iy), corner(ix + 1.0, iy), ux);
    let bottom = mix(corner(ix, iy + 1.0), corner(ix + 1.0, iy + 1.0), ux);
    mix(top, bottom, uy)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// What a stack comes to while it is built.
#[derive(Default)]
struct Builder {
    uv: Vec<Step>,
    sample: Vec<Step>,
    colour: Vec<Step>,
    /// The affine colour effects so far since the last step that is not one.
    matrix: Option<Colour>,
    /// The blurs so far, added as independent blurs are.
    blur: f32,
}

impl Builder {
    fn affine(&mut self, m: Colour) {
        self.matrix = Some(match &self.matrix {
            Some(so_far) => then(so_far, &m),
            None => m,
        });
    }

    /// Ends a run of affine colour effects with its one matrix.
    fn flush(&mut self) {
        if let Some(m) = self.matrix.take()
            && m != IDENTITY
        {
            let mut p = [0.0; 12];
            for (row, values) in m.iter().enumerate() {
                p[row * 4..row * 4 + 4].copy_from_slice(values);
            }
            self.colour.push(Step {
                p,
                ..Step::new(MATRIX, 0, &[])
            });
        }
    }

    fn colour(&mut self, step: Step) {
        self.flush();
        self.colour.push(step);
    }

    fn add(&mut self, effect: &Effect, time: Time) {
        let seed = seed_of(&effect.id);
        let paced = |pace: &Pace| cycles(pace, time);
        let angle = |degrees: f32, turns: f32, cycles: f64| -> f32 {
            let total = f64::from(degrees) + f64::from(turns) * cycles * 360.0;
            total.rem_euclid(360.0).to_radians() as f32
        };
        let kind = &effect.kind;
        if let Some(m) = matrix(kind) {
            self.affine(m);
            return;
        }
        match kind {
            EffectKind::Levels {
                black,
                white,
                gamma,
            } => self.colour(Step::new(LEVELS, seed, &[*black, *white, *gamma])),
            EffectKind::Colorize {
                dark,
                light: bright,
                amount,
            } => {
                let (d, l) = (light(dark), light(bright));
                let p = [
                    *amount, 0.0, 0.0, 0.0, d[0], d[1], d[2], 0.0, l[0], l[1], l[2],
                ];
                self.colour(Step::new(COLORIZE, seed, &p));
            }
            EffectKind::Solarize { threshold } => {
                self.colour(Step::new(SOLARIZE, seed, &[*threshold]));
            }
            EffectKind::Posterize { levels } => {
                self.colour(Step::new(POSTERIZE, seed, &[*levels]));
            }
            EffectKind::Threshold {
                level,
                softness,
                dark,
                light: bright,
            } => {
                let (d, l) = (light(dark), light(bright));
                let p = [
                    *level, *softness, 0.0, 0.0, d[0], d[1], d[2], 0.0, l[0], l[1], l[2],
                ];
                self.colour(Step::new(THRESHOLD, seed, &p));
            }
            EffectKind::EdgeDetect { strength, mode } => {
                let mode = f32::from(*mode == EdgeMode::Glow);
                self.sample.push(Step::new(EDGE, seed, &[*strength, mode]));
            }
            EffectKind::Vignette {
                amount,
                softness,
                roundness,
            } => self.colour(Step::new(VIGNETTE, seed, &[*amount, *softness, *roundness])),
            EffectKind::Noise {
                amount,
                size,
                animated,
            } => {
                let step = if *animated {
                    (time.seconds * 24.0).floor().rem_euclid(INDEX_RANGE) as f32
                } else {
                    0.0
                };
                self.colour(Step::new(NOISE, seed, &[*amount, *size, step]));
            }
            EffectKind::Scanlines {
                count,
                intensity,
                pace,
            } => {
                let c = paced(pace);
                let phase = (c - c.floor()) as f32;
                self.colour(Step::new(SCANLINES, seed, &[*count, *intensity, phase]));
            }
            EffectKind::Blur { radius } => self.blur = self.blur.hypot(*radius),
            EffectKind::RadialBlur { amount, x, y } => {
                self.sample
                    .push(Step::new(RADIAL, seed, &[*amount, *x, *y]));
            }
            EffectKind::ZoomBlur { amount, x, y } => {
                self.sample
                    .push(Step::new(ZOOM_BLUR, seed, &[*amount * 0.5, *x, *y]));
            }
            EffectKind::Mirror { axis } => {
                let kind = match axis {
                    MirrorAxis::Horizontal => MIRROR_HORIZONTAL,
                    MirrorAxis::Vertical => MIRROR_VERTICAL,
                    MirrorAxis::Both => MIRROR_BOTH,
                };
                self.uv.push(Step::new(kind, seed, &[]));
            }
            EffectKind::Zoom { amount } => self.uv.push(Step::new(ZOOM, seed, &[*amount])),
            EffectKind::Shift { x, y } => {
                self.uv.push(Step::new(TRANSLATE, seed, &[*x, *y, 1.0]));
            }
            EffectKind::Rotate {
                angle: degrees,
                spin,
                pace,
            } => {
                let a = angle(*degrees, *spin, paced(pace));
                self.uv.push(Step::new(ROTATE, seed, &[a]));
            }
            EffectKind::Kaleidoscope {
                segments,
                rotation,
                spin,
                x,
                y,
                pace,
            } => {
                let a = angle(*rotation, *spin, paced(pace));
                self.uv
                    .push(Step::new(KALEIDOSCOPE, seed, &[*segments, a, *x, *y]));
            }
            EffectKind::Polar { amount } => self.uv.push(Step::new(POLAR, seed, &[*amount])),
            EffectKind::Wave {
                amplitude,
                frequency,
                direction,
                pace,
            } => {
                let direction = match direction {
                    MirrorAxis::Horizontal => 0.0,
                    MirrorAxis::Vertical => 1.0,
                    MirrorAxis::Both => 2.0,
                };
                let step = Step::new(WAVE, seed, &[*amplitude, *frequency, 0.0, direction]);
                self.uv.push(step.at(paced(pace)));
            }
            EffectKind::Twirl {
                angle: degrees,
                radius,
                x,
                y,
            } => {
                let p = [degrees.to_radians(), *radius, *x, *y];
                self.uv.push(Step::new(TWIRL, seed, &p));
            }
            EffectKind::Bulge {
                amount,
                radius,
                x,
                y,
            } => self
                .uv
                .push(Step::new(BULGE, seed, &[*amount, *radius, *x, *y])),
            EffectKind::Tile {
                columns,
                rows,
                mirror,
            } => {
                let p = [*columns, *rows, f32::from(*mirror)];
                self.uv.push(Step::new(TILE, seed, &p));
            }
            EffectKind::Pixelate { size } => self.uv.push(Step::new(PIXELATE, seed, &[*size])),
            EffectKind::Displace {
                amplitude,
                scale,
                pace,
            } => {
                let step = Step::new(DISPLACE, seed, &[*amplitude, *scale]);
                self.uv.push(step.at(paced(pace)));
            }
            EffectKind::Slide { x, y, wrap, pace } => {
                let c = paced(pace);
                let offset = |speed: f32| {
                    let moved = f64::from(speed) * c;
                    if *wrap {
                        moved.rem_euclid(1.0) as f32
                    } else {
                        ((moved + 1.0).rem_euclid(2.0) - 1.0) as f32
                    }
                };
                let p = [offset(*x), offset(*y), f32::from(*wrap)];
                self.uv.push(Step::new(TRANSLATE, seed, &p));
            }
            EffectKind::ShiftGlitch {
                amount,
                block,
                probability,
                pace,
            } => {
                let step = Step::new(GLITCH, seed, &[*amount, *block, *probability]);
                self.uv.push(step.at(paced(pace)));
            }
            EffectKind::Twitch {
                amount,
                probability,
                pace,
            } => self.twitch(seed, *amount, *probability, paced(pace)),
            EffectKind::Roll { jitter, pace } => {
                let mut step = Step::new(ROLL, seed, &[*jitter]).at(paced(pace));
                step.index = (time.seconds * 25.0).floor().rem_euclid(INDEX_RANGE) as u32;
                self.uv.push(step);
            }
            EffectKind::RgbSplit {
                amount,
                angle: degrees,
                animate,
                pace,
            } => {
                let turns = if *animate { 1.0 } else { 0.0 };
                let a = angle(*degrees, turns, paced(pace));
                self.sample
                    .push(Step::new(SPLIT, seed, &[*amount * SPLIT_REACH, a]));
            }
            EffectKind::Strobe {
                colour,
                duty,
                mode,
                pace,
            } => {
                let c = paced(pace);
                if ((c - c.floor()) as f32) < *duty {
                    let on = match mode {
                        StrobeMode::Flash => light(colour),
                        StrobeMode::Blackout => [0.0; 3],
                    };
                    self.affine(scale_and_add(0.0, on));
                }
            }
            // The affine colour effects went through `matrix`.
            _ => {}
        }
    }

    /// A twitch: in some cycles, a jump of the picture's place and size and a flash of light, each
    /// from a hash of the cycle, held until the next.
    fn twitch(&mut self, seed: u32, amount: f32, probability: f32, cycles: f64) {
        let index = cycles.floor().rem_euclid(INDEX_RANGE) as u32;
        let roll = |k: u32| unit(hash3(seed, index, k));
        if roll(0) >= probability {
            return;
        }
        let signed = |k: u32| (roll(k) * 2.0 - 1.0) * amount;
        let p = [signed(1) * 0.25, signed(2) * 0.25, 1.0 + signed(3) * 0.5];
        self.uv.push(Step::new(TRANSFORM, seed, &p));
        self.affine(scale_and_add(1.0, [signed(4) * 0.5; 3]));
    }
}

impl Effects {
    /// What `chain` comes to at `time`, applied from the first; a bypassed effect is left out.
    pub fn new(chain: &[Effect], time: Time) -> Self {
        let mut builder = Builder::default();
        for effect in chain.iter().filter(|effect| !effect.bypass) {
            builder.add(effect, time);
        }
        builder.flush();
        if builder.blur > 0.0 {
            let blur = builder.blur.min(1.0);
            builder.sample.insert(0, Step::new(BLUR, 0, &[blur]));
        }
        let mut effects = Self::default();
        let stages = [&builder.uv, &builder.sample, &builder.colour];
        let mut counts = [0u32; 3];
        let mut next = 0;
        for (stage, steps) in stages.into_iter().enumerate() {
            for step in steps.iter().take(MAX_STEPS - next) {
                effects.steps[next] = *step;
                next += 1;
                counts[stage] += 1;
            }
        }
        [effects.uv, effects.sample, effects.colour] = counts;
        effects
    }

    /// Whether drawing with them is the same as drawing without.
    pub fn is_identity(&self) -> bool {
        self.uv + self.sample + self.colour == 0
    }

    fn stage(&self, first: u32, count: u32) -> &[Step] {
        &self.steps[first as usize..(first + count) as usize]
    }

    /// The steps that move the picture, the first applied first.
    pub fn uv_steps(&self) -> &[Step] {
        self.stage(0, self.uv)
    }

    /// The steps that change how the picture is read.
    pub fn sample_steps(&self) -> &[Step] {
        self.stage(self.uv, self.sample)
    }

    /// The colour steps, the first applied first.
    pub fn colour_steps(&self) -> &[Step] {
        self.stage(self.uv + self.sample, self.colour)
    }

    /// Where the picture's coordinate `uv` (0 to 1 across and down) is read from, for a picture of
    /// `size` pixels, and the offset (a fraction of the width, along it) that a glitch gives its
    /// red and blue: the steps walked back from the last. `None` where a step leaves the picture,
    /// which is transparent.
    pub fn walk(&self, uv: [f32; 2], size: [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
        let asp = size[0] / size[1].max(1.0);
        let sc = [asp, 1.0];
        let mut p = uv;
        let mut split = [0.0f32; 2];
        let inside = |p: [f32; 2]| p.iter().all(|v| (0.0..=1.0).contains(v));
        for step in self.uv_steps().iter().rev() {
            let s = &step.p;
            match step.kind {
                MIRROR_HORIZONTAL => p[0] = fold(p[0]),
                MIRROR_VERTICAL => p[1] = fold(p[1]),
                MIRROR_BOTH => p = p.map(fold),
                ZOOM => {
                    p = p.map(|v| (v - 0.5) / s[0] + 0.5);
                    if !inside(p) {
                        return None;
                    }
                }
                TRANSLATE => {
                    p = [p[0] - s[0], p[1] - s[1]];
                    if s[2] > 0.5 {
                        p = p.map(wrap);
                    } else if !inside(p) {
                        return None;
                    }
                }
                ROTATE => {
                    let q = rotate([(p[0] - 0.5) * sc[0], p[1] - 0.5], -s[0]);
                    p = [q[0] / sc[0] + 0.5, q[1] + 0.5];
                    if !inside(p) {
                        return None;
                    }
                }
                KALEIDOSCOPE => {
                    let (c, rotation) = ([s[2], s[3]], s[1]);
                    let q = [(p[0] - c[0]) * sc[0], p[1] - c[1]];
                    let r = q[0].hypot(q[1]);
                    let segment = TAU / s[0];
                    let mut a = q[1].atan2(q[0]) - rotation;
                    a -= segment * (a / segment).floor();
                    a = (a - segment * 0.5).abs() + rotation;
                    p = mirror_repeat([r * a.cos() / sc[0] + c[0], r * a.sin() + c[1]]);
                }
                POLAR => {
                    let q = [(p[0] - 0.5) * sc[0], p[1] - 0.5];
                    let theta = q[1].atan2(q[0]) / TAU + 0.5;
                    let r = q[0].hypot(q[1]) * 2.0;
                    let polar = mirror_repeat([theta, r]);
                    p = [mix(p[0], polar[0], s[0]), mix(p[1], polar[1], s[0])];
                }
                WAVE => {
                    let wave = |along: f32| s[0] * (TAU * (s[1] * along + step.phase)).sin();
                    let o = p;
                    if s[3] != 1.0 {
                        p[0] += wave(o[1]);
                    }
                    if s[3] >= 1.0 {
                        p[1] += wave(o[0]);
                    }
                    p = mirror_repeat(p);
                }
                TWIRL => {
                    let c = [s[2], s[3]];
                    let q = [(p[0] - c[0]) * sc[0], p[1] - c[1]];
                    let t = (1.0 - q[0].hypot(q[1]) / s[1].max(1e-4)).clamp(0.0, 1.0);
                    let q = rotate(q, s[0] * t * t);
                    p = mirror_repeat([q[0] / sc[0] + c[0], q[1] + c[1]]);
                }
                BULGE => {
                    let c = [s[2], s[3]];
                    let q = [(p[0] - c[0]) * sc[0], p[1] - c[1]];
                    let n = q[0].hypot(q[1]) / s[1].max(1e-4);
                    let k = if n < 1.0 {
                        1.0 - 0.9 * s[0] * (1.0 - n * n)
                    } else {
                        1.0
                    };
                    p = mirror_repeat([q[0] * k / sc[0] + c[0], q[1] * k + c[1]]);
                }
                TILE => {
                    let m = [p[0] * s[0], p[1] * s[1]];
                    p = [0, 1].map(|i| {
                        let cell = m[i].floor();
                        let f = m[i] - cell;
                        let odd = (cell - 2.0 * (cell / 2.0).floor()) > 0.5;
                        if s[2] > 0.5 && odd { 1.0 - f } else { f }
                    });
                }
                PIXELATE => {
                    if s[0] > 0.0 {
                        let cell = [s[0] / asp, s[0]];
                        p = [0, 1].map(|i| (((p[i] / cell[i]).floor() + 0.5) * cell[i]).min(1.0));
                    }
                }
                DISPLACE => {
                    let t = (step.index % 4096) as f32 + step.phase;
                    let q = [p[0] * sc[0] * s[1], p[1] * s[1]];
                    let dx = value_noise([q[0] + t, q[1] + 0.37 * t], step.seed) - 0.5;
                    let dy = value_noise([q[0] + 17.3, q[1] + 11.1 + 0.83 * t], step.seed) - 0.5;
                    p = mirror_repeat([p[0] + dx * 2.0 * s[0], p[1] + dy * 2.0 * s[0]]);
                }
                GLITCH => {
                    let block = (p[1] / s[1].max(0.001)).floor() as i32 as u32;
                    let h = hash3(step.seed, step.index, block);
                    if unit(h) < s[2] {
                        let offset = (unit(lowbias(h ^ 0x68bc_21eb)) * 2.0 - 1.0) * s[0];
                        p[0] = wrap(p[0] + offset);
                        if unit(lowbias(h ^ 0x02e5_be93)) < 0.35 {
                            split[0] += offset * 0.3;
                        }
                    }
                }
                TRANSFORM => {
                    p = mirror_repeat([
                        (p[0] - 0.5) / s[2] + 0.5 - s[0],
                        (p[1] - 0.5) / s[2] + 0.5 - s[1],
                    ]);
                }
                ROLL => {
                    let row = (p[1] * size[1]).floor() as i32 as u32;
                    let jitter = (unit(hash3(step.seed, step.index, row)) - 0.5) * s[0] * 0.05;
                    p = mirror_repeat([p[0] + jitter, wrap(p[1] + step.phase)]);
                }
                _ => {}
            }
        }
        Some((p, split))
    }

    /// Where the picture is read for `uv`, as [`Self::walk`] gives it.
    pub fn source(&self, uv: [f32; 2], size: [f32; 2]) -> Option<[f32; 2]> {
        self.walk(uv, size).map(|(p, _)| p)
    }

    /// The picture at `p` as the sample stage reads it, with the colour offset `glitch` a glitch
    /// asked for: straight (not premultiplied) linear colour and its cover. `read` gives the
    /// picture's light times its cover, and its cover, at a coordinate.
    pub fn sample(
        &self,
        read: &impl Fn([f32; 2]) -> [f32; 4],
        p: [f32; 2],
        glitch: [f32; 2],
        size: [f32; 2],
    ) -> [f32; 4] {
        let asp = size[0] / size[1].max(1.0);
        let steps = self.sample_steps();
        let mut split = glitch;
        let (mut edge, mut glow, mut smears) = (0.0f32, false, false);
        for step in steps {
            match step.kind {
                SPLIT => {
                    split[0] += step.p[0] * step.p[1].cos() / asp;
                    split[1] += step.p[0] * step.p[1].sin();
                }
                EDGE => {
                    edge += step.p[0];
                    glow = step.p[1] > 0.5;
                }
                _ => smears = true,
            }
        }
        // The picture at `q`, its red and blue taken from either side when they are split.
        let at = |q: [f32; 2]| -> [f32; 4] {
            if split == [0.0; 2] {
                return read(q);
            }
            let (a, g, b) = (
                read([q[0] + split[0], q[1] + split[1]]),
                read(q),
                read([q[0] - split[0], q[1] - split[1]]),
            );
            [a[0], g[1], b[2], (a[3] + g[3] + b[3]) / 3.0]
        };
        let mut sum = [0.0f32; 4];
        if smears {
            let reach = {
                let short = size[0].min(size[1]);
                [short / size[0] * MAX_BLUR, short / size[1] * MAX_BLUR]
            };
            for n in 0..CPU_TAPS {
                let t = (n as f32 + 0.5) / CPU_TAPS as f32;
                let mut q = p;
                for step in steps {
                    let s = &step.p;
                    match step.kind {
                        BLUR => {
                            let [dx, dy] = blur_tap(n, CPU_TAPS);
                            q[0] += dx * s[0] * reach[0];
                            q[1] += dy * s[0] * reach[1];
                        }
                        RADIAL => {
                            let c = [s[1], s[2]];
                            let d = [(p[0] - c[0]) * asp, p[1] - c[1]];
                            let turned = rotate(d, s[0] * (t - 0.5));
                            q[0] += (turned[0] - d[0]) / asp;
                            q[1] += turned[1] - d[1];
                        }
                        ZOOM_BLUR => {
                            q[0] += (p[0] - s[1]) * s[0] * (t - 0.5);
                            q[1] += (p[1] - s[2]) * s[0] * (t - 0.5);
                        }
                        _ => {}
                    }
                }
                sum.iter_mut().zip(at(q)).for_each(|(s, v)| *s += v);
            }
            sum = sum.map(|s| s / CPU_TAPS as f32);
        } else {
            sum = at(p);
        }
        let mut out = [0.0, 0.0, 0.0, sum[3]];
        if sum[3] > 0.0 {
            out = [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3], sum[3]];
        }
        if edge > 0.0 {
            let grey = |q: [f32; 2]| {
                let t = read(q);
                LUMA[0] * t[0] + LUMA[1] * t[1] + LUMA[2] * t[2]
            };
            let o = [1.0 / size[0], 1.0 / size[1]];
            let l = |dx: f32, dy: f32| grey([p[0] + dx * o[0], p[1] + dy * o[1]]);
            let gx = l(1.0, -1.0) + 2.0 * l(1.0, 0.0) + l(1.0, 1.0)
                - l(-1.0, -1.0)
                - 2.0 * l(-1.0, 0.0)
                - l(-1.0, 1.0);
            let gy = l(-1.0, 1.0) + 2.0 * l(0.0, 1.0) + l(1.0, 1.0)
                - l(-1.0, -1.0)
                - 2.0 * l(0.0, -1.0)
                - l(1.0, -1.0);
            let e = (gx.hypot(gy) * edge).clamp(0.0, 1.0);
            for c in &mut out[..3] {
                *c = if glow { *c + e } else { e };
            }
        }
        out
    }

    /// `rgb` (linear light) of the picture at `at` through the colour steps, kept within 0 to 1;
    /// `size` is the picture's size in pixels.
    pub fn colour_of(&self, rgb: [f32; 3], at: [f32; 2], size: [f32; 2]) -> [f32; 3] {
        let mut c = rgb;
        for step in self.colour_steps() {
            let s = &step.p;
            match step.kind {
                MATRIX => {
                    c = std::array::from_fn(|i| {
                        let row = &s[i * 4..i * 4 + 4];
                        row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3]
                    });
                }
                POSTERIZE => {
                    let n = (s[0] - 1.0).max(1.0);
                    c = c.map(|v| decode(((encode(v) * n + 0.5).floor() / n).clamp(0.0, 1.0)));
                }
                THRESHOLD => {
                    let l = encode(grey(c));
                    let t = if s[1] <= 1e-4 {
                        f32::from(l >= s[0])
                    } else {
                        smoothstep(s[0] - s[1] * 0.5, s[0] + s[1] * 0.5, l)
                    };
                    c = std::array::from_fn(|i| mix(s[4 + i], s[8 + i], t));
                }
                COLORIZE => {
                    let e = encode(grey(c));
                    c = std::array::from_fn(|i| mix(c[i], mix(s[4 + i], s[8 + i], e), s[0]));
                }
                SOLARIZE => {
                    c = c.map(|v| {
                        let e = encode(v.clamp(0.0, 1.0));
                        decode(if e > s[0] { 1.0 - e } else { e })
                    });
                }
                LEVELS => {
                    let range = (s[1] - s[0]).max(1e-3);
                    c = c.map(|v| {
                        let e = ((encode(v.clamp(0.0, 1.0)) - s[0]) / range).clamp(0.0, 1.0);
                        decode(e.powf(1.0 / s[2].max(0.01)))
                    });
                }
                VIGNETTE => {
                    let (u, v) = ((at[0] - 0.5) * 2.0, (at[1] - 0.5) * 2.0);
                    let square = u.abs().max(v.abs());
                    let round = u.hypot(v) / std::f32::consts::SQRT_2;
                    let d = mix(square, round, s[2]);
                    let t = ((d - (1.0 - s[1])) / s[1].max(0.01)).clamp(0.0, 1.0);
                    let dark = 1.0 - s[0] * t * t * (3.0 - 2.0 * t);
                    c = c.map(|v| v * dark);
                }
                NOISE => {
                    let cell = 1.0 + s[1] * 15.0;
                    let gx = (at[0] * size[0] / cell).floor() as i32 as u32;
                    let gy = (at[1] * size[1] / cell).floor() as i32 as u32;
                    let seed = step.seed ^ (s[2] as u32).wrapping_mul(0x9e37_79b1);
                    let n = (unit(hash3(seed, gx, gy)) - 0.5) * 2.0 * s[0] * 0.5;
                    c = c.map(|v| decode((encode(v.clamp(0.0, 1.0)) + n).clamp(0.0, 1.0)));
                }
                SCANLINES => {
                    let y = at[1] * s[0] - s[2];
                    let line = 0.5 - 0.5 * (TAU * y).cos();
                    let dark = 1.0 - s[1] * line;
                    c = c.map(|v| v * dark);
                }
                _ => {}
            }
            c = c.map(|v| v.clamp(0.0, 1.0));
        }
        c.map(|v| v.clamp(0.0, 1.0))
    }

    /// The numbers the shader reads: a header of the three counts, then the steps, each its kind,
    /// seed, phase and index and its twelve settings.
    pub(crate) fn pack(&self) -> [f32; FX_FLOATS] {
        let mut out = [0.0; FX_FLOATS];
        out[..3].copy_from_slice(&[self.uv as f32, self.sample as f32, self.colour as f32]);
        for (i, step) in self.steps.iter().enumerate() {
            let at = 4 + i * STEP_FLOATS;
            out[at..at + 4].copy_from_slice(&[
                step.kind as f32,
                step.seed as f32,
                step.phase,
                step.index as f32,
            ]);
            out[at + 4..at + STEP_FLOATS].copy_from_slice(&step.p);
        }
        out
    }
}

fn grey(c: [f32; 3]) -> f32 {
    LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2]
}

/// Light to the perceptual scale the posterize, threshold and levels work on (a plain gamma 2.2).
fn encode(v: f32) -> f32 {
    v.max(0.0).powf(1.0 / 2.2)
}

fn decode(v: f32) -> f32 {
    v.max(0.0).powf(2.2)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rotate(v: [f32; 2], angle: f32) -> [f32; 2] {
    let (s, c) = angle.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// The right half of a mirror shows the left half turned over.
fn fold(v: f32) -> f32 {
    if v > 0.5 { 1.0 - v } else { v }
}

/// `v` brought back into 0 to 1 by going round.
fn wrap(v: f32) -> f32 {
    v - v.floor()
}

/// `p` brought into the picture by bouncing off its edges.
fn mirror_repeat(p: [f32; 2]) -> [f32; 2] {
    p.map(|v| {
        let m = v - 2.0 * (v * 0.5).floor();
        if m > 1.0 { 2.0 - m } else { m }
    })
}

/// Where tap `i` of `taps` is, in a disc of radius 1 (a sunflower spiral: equal areas).
pub fn blur_tap(i: usize, taps: usize) -> [f32; 2] {
    let radius = ((i as f32 + 0.5) / taps as f32).sqrt();
    let angle = i as f32 * 2.399_963;
    [radius * angle.cos(), radius * angle.sin()]
}

#[cfg(test)]
mod tests {
    use flux_project::vj_effects::{EffectKind as K, Pace, Sync};

    use super::*;

    const SQUARE: [f32; 2] = [100.0, 100.0];

    fn effect(kind: K) -> Effect {
        Effect {
            id: "e1".into(),
            bypass: false,
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
}
