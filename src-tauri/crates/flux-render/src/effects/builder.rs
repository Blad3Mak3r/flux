use super::*;

/// What a stack comes to while it is built.
#[derive(Default)]
pub(super) struct Builder {
    pub(super) uv: Vec<Step>,
    pub(super) sample: Vec<Step>,
    pub(super) colour: Vec<Step>,
    /// The affine colour effects so far since the last step that is not one.
    matrix: Option<Colour>,
    /// The blurs so far, added as independent blurs are.
    pub(super) blur: f32,
}

impl Builder {
    fn affine(&mut self, m: Colour) {
        self.matrix = Some(match &self.matrix {
            Some(so_far) => then(so_far, &m),
            None => m,
        });
    }

    /// Ends a run of affine colour effects with its one matrix.
    pub(super) fn flush(&mut self) {
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

    pub(super) fn add(&mut self, effect: &Effect, time: Time) {
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
