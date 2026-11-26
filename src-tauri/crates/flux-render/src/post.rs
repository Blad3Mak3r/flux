//! The composition's effects: what is done to the final mix of the layers before it is mapped to
//! the screens, as the GPU ([`crate::Screen`]) and the CPU monitor ([`crate::composite_post`]) both
//! draw it.
//!
//! A stack of effects (`flux_project::vj_effects`) becomes one [`Post`] each frame. Its effects
//! run in three groups, each keeping the order of the stack:
//!
//! 1. **History.** Feedback and trails read the previous output (the history) and combine it with
//!    the new frame.
//! 2. **Program.** Every other effect, as the one program a picture's effects are
//!    ([`Effects`]), with the whole composition as the picture.
//! 3. **Bloom.** A bright pass, a chain of blurs and the glow added back.
//!
//! The order of the stack is kept inside each group, but a bloom always comes after the program,
//! and the program after the history effects, whatever their places in the stack.
//!
//! What the history holds is the result of the first two: a bloom is only for the eye, and a
//! feedback that drew it again would pile its light up until the frame is white.
//! Time is a frame step: the amounts of the history effects are shares kept every 60th of a second
//! and the movements are per second, both scaled by the seconds since the last frame (`dt`), so a
//! tunnel is the same at 30 and at 120 frames a second.

use flux_project::vj_effects::{Effect, EffectKind, FeedbackBlend, TrailMode};

use crate::effects::{Effects, Time};

/// Most blooms drawn; each is a bright pass and a chain of blurs.
pub const MAX_BLOOMS: usize = 2;
/// The frame rate the shares kept by a history effect are counted at.
const STEPS_PER_SECOND: f32 = 60.0;
/// The longest step of time one frame takes into account: a stall does not throw the history.
pub const MAX_DT: f32 = 0.25;
/// The levels of a bloom's chain of blurs, each half the size of the one before.
pub const BLOOM_LEVELS: usize = 4;

/// An effect that draws the previous output again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stage {
    Feedback(Feedback),
    Trails(Trails),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Feedback {
    pub amount: f32,
    /// Times larger the old frame is drawn every 60th of a second.
    pub zoom: f32,
    /// Degrees a second.
    pub rotate: f32,
    /// Fractions of the frame a second, right and down.
    pub shift: [f32; 2],
    /// Degrees a second.
    pub hue: f32,
    pub blend: FeedbackBlend,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trails {
    pub decay: f32,
    pub mode: TrailMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloom {
    pub threshold: f32,
    pub intensity: f32,
    pub radius: f32,
}

/// What the composition's effects come to for one frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Post {
    /// The effects that need no history, as a program on the whole composition.
    pub program: Effects,
    pub stages: Vec<Stage>,
    pub blooms: Vec<Bloom>,
    /// Seconds since the last frame, 0 to [`MAX_DT`].
    pub dt: f32,
}

impl Post {
    /// What `chain` comes to at `time` (the seconds and beats of the show's clock), `dt` seconds
    /// after the last frame; a bypassed effect is left out.
    pub fn new(chain: &[Effect], time: Time, dt: f32) -> Self {
        let mut program = Vec::new();
        let mut stages = Vec::new();
        let mut blooms = Vec::new();
        for effect in chain.iter().filter(|effect| !effect.bypass) {
            match effect.kind {
                EffectKind::Feedback {
                    amount,
                    zoom,
                    rotate,
                    x,
                    y,
                    hue,
                    blend,
                } => stages.push(Stage::Feedback(Feedback {
                    amount,
                    zoom,
                    rotate,
                    shift: [x, y],
                    hue,
                    blend,
                })),
                EffectKind::Trails { decay, mode } => {
                    stages.push(Stage::Trails(Trails { decay, mode }));
                }
                EffectKind::Bloom {
                    threshold,
                    intensity,
                    radius,
                } => {
                    if blooms.len() < MAX_BLOOMS {
                        blooms.push(Bloom {
                            threshold,
                            intensity,
                            radius,
                        });
                    }
                }
                _ => program.push(effect.clone()),
            }
        }
        Self {
            program: Effects::new(&program, time),
            stages,
            blooms,
            dt: if dt.is_finite() {
                dt.clamp(0.0, MAX_DT)
            } else {
                0.0
            },
        }
    }

    /// Whether drawing with it is the same as drawing without: the composition goes straight to
    /// the screen then.
    pub fn is_empty(&self) -> bool {
        self.program.is_identity() && self.stages.is_empty() && self.blooms.is_empty()
    }
}

/// What a history effect does in one frame of `dt` seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    /// The share of the old frame that is kept.
    pub keep: f32,
    /// Times larger the old frame is drawn.
    pub scale: f32,
    /// Radians the old frame is turned by.
    pub angle: f32,
    /// How far the old frame moves, as fractions of the frame.
    pub shift: [f32; 2],
    /// Radians its hue turns by.
    pub hue: f32,
    /// How it meets the new frame: [`Feedback::blend`] or [`Trails::mode`], as the shader numbers
    /// them.
    pub mode: u32,
}

fn per_step(share: f32, dt: f32) -> f32 {
    share.clamp(0.0, 1.0).powf(dt * STEPS_PER_SECOND)
}

impl Stage {
    /// The stage over `dt` seconds.
    pub fn step(&self, dt: f32) -> Step {
        match *self {
            Self::Feedback(f) => Step {
                keep: per_step(f.amount, dt),
                scale: f.zoom.max(0.01).powf(dt * STEPS_PER_SECOND),
                angle: f.rotate.to_radians() * dt,
                shift: [f.shift[0] * dt, f.shift[1] * dt],
                hue: f.hue.to_radians() * dt,
                mode: match f.blend {
                    FeedbackBlend::Mix => 0,
                    FeedbackBlend::Add => 1,
                    FeedbackBlend::Lighten => 2,
                },
            },
            Self::Trails(t) => Step {
                keep: per_step(t.decay, dt),
                scale: 1.0,
                angle: 0.0,
                shift: [0.0; 2],
                hue: 0.0,
                // Offset from the feedback's, which the shader tells apart by the entry point.
                mode: match t.mode {
                    TrailMode::Blend => 0,
                    TrailMode::Lighten => 1,
                    TrailMode::Add => 2,
                },
            },
        }
    }
}

/// How a bloom's chain of blurs adds up for `radius` (0 to 1): the gain each level has when it is
/// added back into the one above it, from the first level up (so the last is the smallest), and how
/// many levels the chain needs.
pub fn bloom_gains(radius: f32) -> ([f32; BLOOM_LEVELS - 1], usize) {
    let spread = radius.clamp(0.0, 1.0) * (BLOOM_LEVELS - 1) as f32;
    let gains = std::array::from_fn(|up| (spread - up as f32).clamp(0.0, 1.0));
    let levels = 1 + gains.iter().take_while(|g| **g > 0.0).count();
    (gains, levels)
}

/// The weight of the bright pass for a pixel of `luma` (linear light): 0 below `threshold` (on the
/// scale of the colour steps, the light to the power of 1/2.2), rising to 1 at white.
pub fn bright(luma: f32, threshold: f32) -> f32 {
    let seen = luma.max(0.0).powf(1.0 / 2.2);
    ((seen - threshold) / (1.0 - threshold).max(0.05)).clamp(0.0, 1.0)
}

/// How a bloom's glow of `radius` is added: `rgb` of the picture plus the glow times `intensity`.
pub fn glow(rgb: [f32; 3], glow: [f32; 3], intensity: f32) -> [f32; 3] {
    std::array::from_fn(|i| (rgb[i] + glow[i] * intensity).clamp(0.0, 1.0))
}

/// The colours turned about the grey axis by `angle` radians.
pub fn hue_turn(rgb: [f32; 3], angle: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    let k = (1.0 - c) / 3.0;
    let d = s / 3.0f32.sqrt();
    let [r, g, b] = rgb;
    [
        (c + k) * r + (k - d) * g + (k + d) * b,
        (k + d) * r + (c + k) * g + (k - d) * b,
        (k - d) * r + (k + d) * g + (c + k) * b,
    ]
}

/// Where the old frame is read for the pixel at `uv` (0 to 1 across and down) of a frame of
/// `aspect` (width over height) that `step` draws it from: it is drawn `scale` times as large,
/// turned and moved about the centre of the frame.
pub fn history_at(uv: [f32; 2], aspect: f32, step: &Step) -> [f32; 2] {
    let (x, y) = (
        (uv[0] - 0.5) * aspect / step.scale,
        (uv[1] - 0.5) / step.scale,
    );
    let (s, c) = (-step.angle).sin_cos();
    let (x, y) = (c * x - s * y, s * x + c * y);
    [x / aspect + 0.5 - step.shift[0], y + 0.5 - step.shift[1]]
}

/// The new frame `cur` and the old one `old` (already moved) of a history effect that is a
/// feedback (`feedback`) or a trail, put together.
pub fn meet(feedback: bool, mode: u32, keep: f32, cur: [f32; 3], old: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| {
        let (c, o) = (cur[i], old[i]);
        let v = if feedback {
            match mode {
                0 => c + (o - c) * keep,
                1 => c + o * keep,
                _ => c.max(o * keep),
            }
        } else {
            match mode {
                0 => c + (o - c) * keep,
                1 => c.max(o * keep),
                _ => c + o * keep,
            }
        };
        v.clamp(0.0, 1.0)
    })
}

/// What the CPU monitor remembers between frames: the previous output, as linear light.
#[derive(Debug, Clone, Default)]
pub struct History {
    size: (usize, usize),
    light: Vec<[f32; 3]>,
}

impl History {
    /// Forgets the old frames.
    pub fn clear(&mut self) {
        self.size = (0, 0);
        self.light.clear();
    }

    /// Whether it holds a frame of `w` x `h` pixels.
    pub fn holds(&self, w: usize, h: usize) -> bool {
        self.size == (w, h) && self.light.len() == w * h
    }
}

/// A plane of linear light of `w` x `h` pixels.
struct Plane {
    w: usize,
    h: usize,
    light: Vec<[f32; 3]>,
}

impl Plane {
    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.light[y.min(self.h - 1) * self.w + x.min(self.w - 1)]
    }

    /// The light at `uv`, between pixels; the edge pixels repeat beyond.
    fn bilinear(&self, uv: [f32; 2]) -> [f32; 3] {
        let (a, b) = (
            crate::composite::tap(uv[0], self.w),
            crate::composite::tap(uv[1], self.h),
        );
        let row = |y: usize| -> [f32; 3] {
            let (l, r) = (self.at(a.a, y), self.at(a.b, y));
            std::array::from_fn(|i| l[i] + (r[i] - l[i]) * a.t)
        };
        let (top, bottom) = (row(b.a), row(b.b));
        std::array::from_fn(|i| top[i] + (bottom[i] - top[i]) * b.t)
    }

    /// Half the size, each pixel the mean of `weigh` of the sixteen pixels round the four it
    /// covers (four bilinear taps, as the GPU takes them).
    fn halved(&self, weigh: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let (tx, ty) = (1.0 / self.w as f32, 1.0 / self.h as f32);
        let mut light = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                let mut sum = [0.0; 3];
                for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let p = weigh(self.bilinear([uv[0] + dx * tx, uv[1] + dy * ty]));
                    sum.iter_mut().zip(p).for_each(|(s, v)| *s += v * 0.25);
                }
                light.push(sum);
            }
        }
        Self { w, h, light }
    }

    /// `other`, smaller, added into this one times `gain` (four bilinear taps half a pixel of
    /// `other` round each pixel).
    fn add_smaller(&mut self, other: &Self, gain: f32) {
        let (tx, ty) = (0.5 / other.w as f32, 0.5 / other.h as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let uv = [
                    (x as f32 + 0.5) / self.w as f32,
                    (y as f32 + 0.5) / self.h as f32,
                ];
                let at = &mut self.light[y * self.w + x];
                for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let p = other.bilinear([uv[0] + dx * tx, uv[1] + dy * ty]);
                    at.iter_mut()
                        .zip(p)
                        .for_each(|(a, v)| *a += v * 0.25 * gain);
                }
            }
        }
    }
}

/// Applies `post` to `light` (`w` x `h` pixels of linear light, the layers' mix), and remembers the
/// result in `history`. The monitor's twin of the GPU's passes, step for step, except that the bloom
/// averages 2x2 pixels for each level where the GPU filters by taps.
pub(crate) fn apply(
    post: &Post,
    light: &mut [[f32; 3]],
    w: usize,
    h: usize,
    history: &mut History,
) {
    if !history.holds(w, h) {
        history.size = (w, h);
        history.light = vec![[0.0; 3]; w * h];
    }
    let aspect = w as f32 / h as f32;
    let old = Plane {
        w,
        h,
        light: std::mem::take(&mut history.light),
    };
    for stage in &post.stages {
        let step = stage.step(post.dt);
        let feedback = matches!(stage, Stage::Feedback(_));
        for y in 0..h {
            for x in 0..w {
                let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
                let at = history_at(uv, aspect, &step);
                let inside = (0.0..=1.0).contains(&at[0]) && (0.0..=1.0).contains(&at[1]);
                let mut seen = if inside { old.bilinear(at) } else { [0.0; 3] };
                if feedback {
                    seen = hue_turn(seen, step.hue);
                }
                light[y * w + x] = meet(feedback, step.mode, step.keep, light[y * w + x], seen);
            }
        }
    }
    if !post.program.is_identity() {
        program(&post.program, light, w, h);
    }
    // What the next frame draws again is what the history effects and the program made of this one:
    // a bloom is only for the eye.
    history.light = old.light;
    history.light.copy_from_slice(light);
    for bloom in &post.blooms {
        bloom_pass(bloom, light, w, h);
    }
}

/// The effects program `fx` drawn on `light` as a picture of its own size.
fn program(fx: &Effects, light: &mut [[f32; 3]], w: usize, h: usize) {
    let source = Plane {
        w,
        h,
        light: light.to_vec(),
    };
    let size = [w as f32, h as f32];
    let sample = |uv: [f32; 2]| {
        let [r, g, b] = source.bilinear(uv);
        [r, g, b, 1.0]
    };
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let Some((at, glitch)) = fx.walk([u, v], size) else {
                light[y * w + x] = [0.0; 3];
                continue;
            };
            let [r, g, b, cover] = fx.sample(&sample, at, glitch, size);
            let [r, g, b] = fx.colour_of([r, g, b], [u, v], size);
            light[y * w + x] = [r * cover, g * cover, b * cover];
        }
    }
}

/// A bloom: the bright pass, the chain of blurs added up, and the glow added to `light`.
fn bloom_pass(bloom: &Bloom, light: &mut [[f32; 3]], w: usize, h: usize) {
    let (gains, levels) = bloom_gains(bloom.radius);
    let source = Plane {
        w,
        h,
        light: light.to_vec(),
    };
    let mut chain = vec![source.halved(|p| {
        let luma = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        p.map(|v| v * bright(luma, bloom.threshold))
    })];
    for _ in 1..levels {
        let next = chain[chain.len() - 1].halved(|p| p);
        chain.push(next);
    }
    // The smallest level is added into the one above it, and so on up to the first.
    for up in (0..levels - 1).rev() {
        let small = chain.pop().expect("a level");
        chain[up].add_smaller(&small, gains[up]);
    }
    let first = &chain[0];
    for y in 0..h {
        for x in 0..w {
            let uv = [(x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32];
            let at = &mut light[y * w + x];
            *at = glow(*at, first.bilinear(uv), bloom.intensity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feedback(amount: f32, zoom: f32) -> Feedback {
        Feedback {
            amount,
            zoom,
            rotate: 0.0,
            shift: [0.0; 2],
            hue: 0.0,
            blend: FeedbackBlend::Mix,
        }
    }

    #[test]
    fn the_amounts_should_not_depend_on_the_frame_rate() {
        // Two frames of 1/120 s keep as much as one of 1/60 s.
        let stage = Stage::Feedback(feedback(0.5, 1.5));
        let (long, short) = (stage.step(1.0 / 60.0), stage.step(1.0 / 120.0));
        assert!((long.keep - short.keep * short.keep).abs() < 1e-6);
        assert!((long.scale - short.scale * short.scale).abs() < 1e-5);
        let turned = Stage::Feedback(Feedback {
            rotate: 90.0,
            ..feedback(0.5, 1.0)
        });
        assert!((turned.step(0.5).angle - std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        // No time passes, nothing is kept or moved.
        assert_eq!(stage.step(0.0).keep, 1.0);
        assert_eq!(
            Stage::Trails(Trails {
                decay: 0.0,
                mode: TrailMode::Add
            })
            .step(0.1)
            .keep,
            0.0
        );
    }

    #[test]
    fn the_chain_of_a_bloom_should_widen_with_its_radius() {
        assert_eq!(bloom_gains(0.0), ([0.0; 3], 1));
        assert_eq!(bloom_gains(1.0), ([1.0; 3], BLOOM_LEVELS));
        let (gains, levels) = bloom_gains(0.5);
        assert_eq!(levels, 3);
        assert!(gains[0] == 1.0 && gains[1] > 0.0 && gains[1] < 1.0 && gains[2] == 0.0);
    }

    #[test]
    fn the_bright_pass_should_cut_the_dark_and_keep_the_white() {
        assert_eq!(bright(0.0, 0.5), 0.0);
        assert_eq!(bright(1.0, 0.5), 1.0);
        assert_eq!(bright(1.0, 1.0), 0.0);
        assert!(bright(0.3, 0.5) < bright(0.6, 0.5));
    }

    #[test]
    fn a_hue_turn_should_keep_grey_and_come_round() {
        let grey = hue_turn([0.4; 3], 1.0);
        assert!(grey.iter().all(|v| (v - 0.4).abs() < 1e-6));
        let back = hue_turn([1.0, 0.0, 0.0], std::f32::consts::TAU);
        assert!((back[0] - 1.0).abs() < 1e-5 && back[1].abs() < 1e-5);
        let moved = hue_turn([1.0, 0.0, 0.0], std::f32::consts::TAU / 3.0);
        assert!(moved[1] > 0.9 && moved[0] < 0.1);
    }

    #[test]
    fn the_old_frame_should_be_read_from_where_a_zoom_and_a_turn_put_it() {
        let still = Stage::Feedback(feedback(1.0, 1.0)).step(1.0 / 60.0);
        assert_eq!(history_at([0.25, 0.75], 2.0, &still), [0.25, 0.75]);
        // Drawn twice as large: the pixel at a quarter of the way reads from an eighth of it.
        let zoomed = Step {
            scale: 2.0,
            ..still
        };
        let at = history_at([0.25, 0.5], 1.0, &zoomed);
        assert!((at[0] - 0.375).abs() < 1e-6 && (at[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_history_should_hold_the_frame_without_its_bloom() {
        let (w, h) = (16, 16);
        let mut light = vec![[0.0; 3]; w * h];
        light[8 * w + 8] = [1.0; 3];
        let before = light.clone();
        let post = Post {
            blooms: vec![Bloom {
                threshold: 0.5,
                intensity: 1.0,
                radius: 0.5,
            }],
            dt: 1.0 / 60.0,
            ..Post::default()
        };
        let mut history = History::default();
        apply(&post, &mut light, w, h, &mut history);
        assert_ne!(light, before, "the bloom glows");
        assert_eq!(history.light, before, "and is not remembered");
    }
}
