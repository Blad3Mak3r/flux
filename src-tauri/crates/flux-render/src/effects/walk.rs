use super::*;

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

pub(super) fn grey(c: [f32; 3]) -> f32 {
    LUMA[0] * c[0] + LUMA[1] * c[1] + LUMA[2] * c[2]
}

/// Light to the perceptual scale the posterize, threshold and levels work on (a plain gamma 2.2).
pub(super) fn encode(v: f32) -> f32 {
    v.max(0.0).powf(1.0 / 2.2)
}

pub(super) fn decode(v: f32) -> f32 {
    v.max(0.0).powf(2.2)
}

pub(super) fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn rotate(v: [f32; 2], angle: f32) -> [f32; 2] {
    let (s, c) = angle.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// The right half of a mirror shows the left half turned over.
pub(super) fn fold(v: f32) -> f32 {
    if v > 0.5 { 1.0 - v } else { v }
}

/// `v` brought back into 0 to 1 by going round.
pub(super) fn wrap(v: f32) -> f32 {
    v - v.floor()
}

/// `p` brought into the picture by bouncing off its edges.
pub(super) fn mirror_repeat(p: [f32; 2]) -> [f32; 2] {
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
