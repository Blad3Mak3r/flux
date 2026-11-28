use super::*;

/// Where a clip sounds, in samples of the timeline counted from its origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Where the first sample of the asset would sit. Negative when the clip is trimmed so far in
    /// that the asset would begin before the origin. Both halves of a split share it, which is
    /// what makes them read the very same samples as the clip they came from.
    pub origin: i64,
    /// First sample that sounds.
    pub from: u64,
    /// One past the last sample that sounds.
    pub to: u64,
    /// Samples the clip takes to rise from silence.
    pub fade_in: u64,
    /// Samples the clip takes to fall to silence.
    pub fade_out: u64,
    pub fade_in_curve: FadeCurve,
    pub fade_out_curve: FadeCurve,
}

/// Where `clip` sounds, given the samples of its asset: trimmed by its offset and length, and never
/// past the end of the asset. A frame is rarely a whole number of samples, so every boundary is
/// converted from frames on its own instead of being summed from shorter lengths.
pub fn span(clip: &AudioClip, clock: &Clock, asset_samples: u64) -> Span {
    let from = clock.frames_to_samples(clip.start);
    let origin = if clip.start >= clip.offset {
        clock.frames_to_samples(clip.start - clip.offset) as i64
    } else {
        -(clock.frames_to_samples(clip.offset - clip.start) as i64)
    };
    let asset_end = u64::try_from(origin.saturating_add(asset_samples as i64)).unwrap_or(0);
    let to = clip.length.map_or(asset_end, |length| {
        clock.frames_to_samples(clip.start + length).min(asset_end)
    });
    let to = to.max(from);
    let fade = |frames: u64| (clock.frames_to_samples(clip.start + frames) - from).min(to - from);
    Span {
        origin,
        from,
        to,
        fade_in: fade(clip.fade_in),
        fade_out: fade(clip.fade_out),
        fade_in_curve: clip.fade_in_curve,
        fade_out_curve: clip.fade_out_curve,
    }
}

/// The gain of a fade `x` of the way up from silence (0 is silent, 1 is full): the same shape
/// whether the clip is rising or falling, because a fade out is read from its end.
pub fn fade_shape(curve: FadeCurve, x: f32) -> f32 {
    match curve {
        FadeCurve::Linear => x,
        FadeCurve::EqualPower => (x * std::f32::consts::FRAC_PI_2).sin(),
        FadeCurve::SCurve => 0.5 - 0.5 * (x * std::f32::consts::PI).cos(),
        FadeCurve::Exponential => x * x,
    }
}

/// Samples a `Hold` segment takes to step to the next point (4 ms), so the step is never a click.
pub(super) const HOLD_RAMP: i64 = 192;

/// One breakpoint of an envelope, on the timeline.
#[derive(Debug, Clone, Copy)]
pub(super) struct Node {
    /// Where it is, in samples of the timeline.
    at: i64,
    db: f32,
    /// The gain at `db`; silence is exactly 0.
    gain: f32,
    curve: EnvCurve,
}

/// A clip's volume envelope placed on the timeline, ready for the audio callback: positions are in
/// samples and gains are linear, worked out once when the programme is built.
#[derive(Debug, Clone)]
pub struct Envelope {
    nodes: Vec<Node>,
}

impl Envelope {
    /// The envelope of `clip`, whose audio file would begin at the sample `origin`; `None` when it
    /// has no points.
    pub fn new(clip: &AudioClip, origin: i64, clock: &Clock) -> Option<Arc<Self>> {
        if clip.envelope.is_empty() {
            return None;
        }
        let nodes = clip
            .envelope
            .iter()
            .map(|point| Node {
                at: origin + clock.ticks_to_samples(point.at, ENVELOPE_TICKS_PER_FRAME) as i64,
                db: point.gain_db,
                gain: if point.gain_db <= ENVELOPE_MIN_DB {
                    0.0
                } else {
                    gain_from_db(point.gain_db)
                },
                curve: point.curve,
            })
            .collect();
        Some(Arc::new(Self { nodes }))
    }

    /// The linear gain at the sample `at`. Before the first point and after the last it is theirs.
    /// Between two points the gain runs in dB, so a fade sounds even, unless one of them is
    /// silence, which has no dB: that segment runs in amplitude.
    pub fn gain_at(&self, at: i64) -> f32 {
        let next = self.nodes.partition_point(|node| node.at <= at);
        let Some(from) = next.checked_sub(1).map(|index| self.nodes[index]) else {
            return self.nodes[0].gain;
        };
        let Some(to) = self.nodes.get(next).copied() else {
            return from.gain;
        };
        // `to.at > at >= from.at`, so the segment is not empty.
        let length = to.at - from.at;
        let t = ((at - from.at) as f64 / length as f64) as f32;
        let u = match from.curve {
            EnvCurve::Linear => t,
            EnvCurve::Smooth => t * t * (3.0 - 2.0 * t),
            EnvCurve::Fast => 1.0 - (1.0 - t) * (1.0 - t),
            EnvCurve::Slow => t * t,
            EnvCurve::Hold => {
                let ramp = HOLD_RAMP.min(length);
                ((at - (to.at - ramp)) as f64 / ramp as f64).clamp(0.0, 1.0) as f32
            }
        };
        if from.gain > 0.0 && to.gain > 0.0 {
            gain_from_db(from.db + (to.db - from.db) * u)
        } else {
            from.gain + (to.gain - from.gain) * u
        }
    }
}

/// One clip placed on the timeline.
#[derive(Debug, Clone)]
pub struct ClipPlayer {
    pub audio: Arc<Decoded>,
    pub span: Span,
    /// Linear gain.
    pub gain: f32,
    /// The volume drawn over the clip, if any.
    pub envelope: Option<Arc<Envelope>>,
    /// Index into the clip set's tracks.
    pub track: usize,
}

impl ClipPlayer {
    /// A clip that plays the whole of `audio` from the sample `start`.
    pub fn new(audio: Arc<Decoded>, start: u64, gain: f32, track: usize) -> Self {
        let span = Span {
            origin: start as i64,
            from: start,
            to: start + audio.frames() as u64,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
        };
        Self {
            audio,
            span,
            gain,
            envelope: None,
            track,
        }
    }

    /// The gain of the fades and the envelope at the sample `at`, which must be inside the span.
    pub(super) fn level(&self, at: u64) -> f32 {
        let envelope = self
            .envelope
            .as_ref()
            .map_or(1.0, |envelope| envelope.gain_at(at as i64));
        self.fade_gain(at) * envelope
    }

    /// The gain of the fades at the sample `at` of the timeline, which must be inside the span:
    /// silent at the first sample of a fade in and at the last of a fade out.
    fn fade_gain(&self, at: u64) -> f32 {
        let span = &self.span;
        let mut gain = 1.0;
        let since_start = at - span.from;
        if since_start < span.fade_in {
            gain = fade_shape(span.fade_in_curve, since_start as f32 / span.fade_in as f32);
        }
        let until_end = span.to - at - 1;
        if until_end < span.fade_out {
            gain *= fade_shape(span.fade_out_curve, until_end as f32 / span.fade_out as f32);
        }
        gain
    }
}
