use super::*;

/// An `f32` the audio callback and the other threads share without locks. Only non-negative
/// values go through [`AtomicF32::fetch_max`]: their bit patterns order like the numbers.
#[derive(Debug, Default)]
pub(super) struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub(super) fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub(super) fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub(super) fn fetch_max(&self, value: f32) {
        self.0.fetch_max(value.to_bits(), Ordering::Relaxed);
    }

    fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

#[derive(Debug)]
pub(super) struct TrackMeter {
    pub(super) id: String,
    /// Where the fader is, linear. Written by the control side.
    pub(super) target: AtomicF32,
    /// The gain the last buffer ended on, so a change is ramped over the next buffer, not stepped.
    pub(super) applied: AtomicF32,
    /// Largest sample the track contributed since it was last read, after its gain.
    pub(super) peak: AtomicF32,
}

/// The peak of one track since the previous reading, as a linear amplitude.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackPeak {
    pub id: String,
    pub peak: f32,
}

/// What the control side shares with the audio callback about each track: the fader going in,
/// the peak coming out.
#[derive(Debug, Default)]
pub struct TrackLevels {
    pub(super) tracks: Vec<TrackMeter>,
}

impl TrackLevels {
    /// One entry per track, with its linear gain.
    pub fn new(tracks: impl IntoIterator<Item = (String, f32)>) -> Self {
        Self {
            tracks: tracks
                .into_iter()
                .map(|(id, gain)| TrackMeter {
                    id,
                    target: AtomicF32::new(gain),
                    applied: AtomicF32::new(gain),
                    peak: AtomicF32::default(),
                })
                .collect(),
        }
    }

    /// Moves a track's fader. Returns whether the track is in this set.
    pub fn set_gain_db(&self, id: &str, db: f32) -> bool {
        let Some(track) = self.tracks.iter().find(|track| track.id == id) else {
            return false;
        };
        if db.is_finite() {
            track.target.store(track_gain_from_db(db));
        }
        true
    }

    /// The peak of every track since the last call, which starts the next measurement.
    pub fn take_peaks(&self) -> Vec<TrackPeak> {
        self.tracks
            .iter()
            .map(|track| TrackPeak {
                id: track.id.clone(),
                peak: track.peak.take(),
            })
            .collect()
    }
}
