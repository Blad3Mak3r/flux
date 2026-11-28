use super::*;

/// Everything that sounds: the clips of the tracks that are not muted.
#[derive(Debug, Default, Clone)]
pub struct ClipSet {
    pub(super) clips: Vec<ClipPlayer>,
    levels: Arc<TrackLevels>,
    /// Samples at which the show ends; `None` while it has no end (see [`ClipSet::with_end`]).
    end: Option<u64>,
    pub(super) operator_tracks: Vec<usize>,
}

/// A clip set plus what could not be included in it.
#[derive(Debug, Default)]
pub struct Programme {
    pub set: ClipSet,
    /// One line per audio file that could not be used, for the user.
    pub warnings: Vec<String>,
}

impl ClipSet {
    /// `tracks` lists each track's id and linear gain; a clip's `track` indexes into it.
    pub fn new(clips: Vec<ClipPlayer>, tracks: Vec<(String, f32)>) -> Self {
        Self {
            clips,
            levels: Arc::new(TrackLevels::new(tracks)),
            end: None,
            operator_tracks: Vec::new(),
        }
    }

    /// Sets where the show ends, in samples since the timeline origin. The clip set holds only the
    /// clips that sound, so the caller, who knows every clip and cue (muted ones too), supplies it.
    pub fn with_end(mut self, end: Option<u64>) -> Self {
        self.end = end;
        self
    }

    /// Where the show ends, in samples since the timeline origin, if it has an end.
    pub fn end(&self) -> Option<u64> {
        self.end
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn levels(&self) -> Arc<TrackLevels> {
        Arc::clone(&self.levels)
    }

    /// Adds the clips' audio for the timeline samples `position..position + left.len()` to `left`
    /// and `right`, which must have the same length.
    pub fn mix(&self, position: u64, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len());
        for (index, (left, right)) in left
            .chunks_mut(MIX_CHUNK)
            .zip(right.chunks_mut(MIX_CHUNK))
            .enumerate()
        {
            self.mix_chunk(
                position + (index * MIX_CHUNK) as u64,
                left,
                right,
                |i, _| !self.operator_tracks.contains(&i),
            );
        }
    }

    /// One chunk of at most [`MIX_CHUNK`] frames.
    pub fn mix_operator(&self, position: u64, left: &mut [f32], right: &mut [f32]) {
        for (index, (left, right)) in left
            .chunks_mut(MIX_CHUNK)
            .zip(right.chunks_mut(MIX_CHUNK))
            .enumerate()
        {
            self.mix_chunk(
                position + (index * MIX_CHUNK) as u64,
                left,
                right,
                |i, _| self.operator_tracks.contains(&i),
            );
        }
    }

    /// Mixes a frozen routing membership. Newly added tracks stay silent until routes are applied.
    pub fn mix_tracks(
        &self,
        position: u64,
        left: &mut [f32],
        right: &mut [f32],
        tracks: &[String],
    ) {
        for (index, (left, right)) in left
            .chunks_mut(MIX_CHUNK)
            .zip(right.chunks_mut(MIX_CHUNK))
            .enumerate()
        {
            self.mix_chunk(
                position + (index * MIX_CHUNK) as u64,
                left,
                right,
                |_, id| tracks.iter().any(|t| t == id),
            );
        }
    }

    fn mix_chunk(
        &self,
        position: u64,
        left: &mut [f32],
        right: &mut [f32],
        selected: impl Fn(usize, &str) -> bool,
    ) {
        let frames = left.len();
        let window_end = position + frames as u64;
        for (index, meter) in self.levels.tracks.iter().enumerate() {
            if !selected(index, &meter.id) {
                continue;
            }
            let mut track_left = [0.0f32; MIX_CHUNK];
            let mut track_right = [0.0f32; MIX_CHUNK];
            for clip in self.clips.iter().filter(|clip| clip.track == index) {
                let from = position.max(clip.span.from);
                let to = window_end.min(clip.span.to);
                for at in from..to {
                    let source = ((at as i64 - clip.span.origin) * 2) as usize;
                    let out = (at - position) as usize;
                    let gain = SCALE * clip.gain * clip.level(at);
                    track_left[out] += f32::from(clip.audio.samples[source]) * gain;
                    track_right[out] += f32::from(clip.audio.samples[source + 1]) * gain;
                }
            }

            let (from_gain, to_gain) = (meter.applied.load(), meter.target.load());
            let mut peak = 0.0f32;
            for out in 0..frames {
                let gain = from_gain + (to_gain - from_gain) * ((out + 1) as f32 / frames as f32);
                let (l, r) = (track_left[out] * gain, track_right[out] * gain);
                left[out] += l;
                right[out] += r;
                peak = peak.max(l.abs()).max(r.abs());
            }
            meter.applied.store(to_gain);
            meter.peak.fetch_max(peak);
        }
    }
}
