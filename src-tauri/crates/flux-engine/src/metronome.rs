//! The metronome: a click on every beat of the timeline.
//!
//! A click is a pure function of the transport position, so it needs no state of its own: whatever
//! buffer is asked for, the clicks inside it are where the beat grid puts them, a seek lands on the
//! grid at once, and a stream that plays on another device only has to know where the master is.
//! Beats are counted from the timeline origin (sample 0 of the transport), beat 0 of every bar
//! being the accent.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flux_project::{Tempo, TempoMap, Timeline};

/// How long a click rings, in seconds.
const CLICK_SECONDS: f64 = 0.03;
/// Time constant of the click's decay, in seconds.
const DECAY_SECONDS: f32 = 0.006;
const ACCENT_HZ: f32 = 1_600.0;
const BEAT_HZ: f32 = 1_000.0;
/// Peak of a click at 0 dB; a click is short and bright, so it sounds louder than its peak says.
const PEAK: f32 = 0.7;

/// The tempo map and level the click is made from: the control side changes the tempo while the
/// show plays and the audio callbacks read it without allocating. The map sits behind a lock that
/// the callbacks only ever try: a buffer that finds it taken is silent, never late.
#[derive(Debug)]
pub struct SharedMetronome {
    map: Mutex<Arc<TempoMap>>,
    enabled: AtomicBool,
    /// Linear amplitude, fixed while the output is open.
    gain: f32,
}

impl SharedMetronome {
    pub fn new(tempo: Tempo, gain_db: f32) -> Self {
        Self {
            map: Mutex::new(Arc::new(TempoMap::single(tempo))),
            enabled: AtomicBool::new(tempo.click),
            gain: if gain_db.is_finite() {
                10f32.powf(gain_db / 20.0)
            } else {
                1.0
            },
        }
    }

    /// Follows an edit of the timeline's tempo or of its click switch, with no changes after the
    /// origin. A tempo outside the range a timeline can hold is brought into it by the map, so
    /// the callbacks never divide by something absurd.
    pub fn set(&self, tempo: Tempo) {
        self.set_map(TempoMap::single(tempo));
    }

    /// Follows an edit of the timeline's tempo map or of its click switch.
    pub fn set_timeline(&self, timeline: &Timeline) {
        self.set_map(TempoMap::of(timeline));
    }

    /// Follows an edit of the tempo map or of the click switch it carries.
    pub fn set_map(&self, map: TempoMap) {
        self.enabled.store(map.click, Ordering::Relaxed);
        *self.map.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(map);
    }

    /// Follows the map of the timeline the show is on. Meant for the audio callback: it takes
    /// nothing but a lock it can fail to get. The map it replaces is freed here only when the
    /// control side set it and nothing else holds it, once, when a show takes over.
    pub fn follow(&self, map: &Arc<TempoMap>) {
        if let Ok(mut current) = self.map.try_lock()
            && !Arc::ptr_eq(&current, map)
        {
            self.enabled.store(map.click, Ordering::Relaxed);
            *current = Arc::clone(map);
        }
    }

    /// The grid to click on at `rate` samples per second, or `None` while the click is off.
    pub fn grid(&self, rate: u32) -> Option<Grid> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let map = Arc::clone(&*self.map.try_lock().ok()?);
        Some(Grid {
            map,
            rate,
            gain: self.gain * PEAK,
        })
    }
}

/// The beat grid at one sample rate: where the beats are is the tempo map's to say.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    map: Arc<TempoMap>,
    rate: u32,
    gain: f32,
}

impl Grid {
    /// Adds the clicks that sound in `out`, whose first sample is transport sample `position`.
    pub fn add(&self, position: u64, out: &mut [f32]) {
        let rate = f64::from(self.rate);
        let ring = (rate * CLICK_SECONDS) as u64;
        let decay = DECAY_SECONDS * self.rate as f32;
        let end = position + out.len() as u64;
        // The first beat that can still be ringing at `position`.
        let mut beat = self
            .map
            .beat_index_at(position.saturating_sub(ring) as f64 / rate);
        loop {
            let start = self.start_of(beat);
            if start >= end {
                break;
            }
            if start + ring > position {
                let hz = if self.map.is_downbeat(beat) {
                    ACCENT_HZ
                } else {
                    BEAT_HZ
                };
                for p in start.max(position)..(start + ring).min(end) {
                    let t = (p - start) as f32;
                    out[(p - position) as usize] +=
                        (TAU * hz * t / self.rate as f32).sin() * (-t / decay).exp() * self.gain;
                }
            }
            beat += 1;
        }
    }

    /// First sample of the beat `index`, counted over the whole map.
    fn start_of(&self, index: u64) -> u64 {
        (self.map.seconds_at_beat(index) * f64::from(self.rate)).round() as u64
    }
}

/// The metronome as one audio stream plays it: the shared tempo and the interface channels it goes
/// to, which all receive the same click.
#[derive(Debug, Clone)]
pub struct ClickOut {
    pub shared: std::sync::Arc<SharedMetronome>,
    /// Zero-based indexes into each frame of the interleaved buffer.
    pub channels: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn tempo(bpm: f64, beats: u8, click: bool) -> Tempo {
        Tempo {
            bpm,
            beats_per_bar: beats,
            beat_unit: 4,
            click,
        }
    }

    fn grid(bpm: f64, beats: u8) -> Grid {
        SharedMetronome::new(tempo(bpm, beats, true), 0.0)
            .grid(RATE)
            .unwrap()
    }

    fn render(grid: &Grid, position: u64, len: usize) -> Vec<f32> {
        let mut out = vec![0.0; len];
        grid.add(position, &mut out);
        out
    }

    /// Samples at which a click starts: the first of a run of sound after silence.
    fn onsets(samples: &[f32]) -> Vec<usize> {
        let mut found = Vec::new();
        let mut quiet = 0;
        for (at, sample) in samples.iter().enumerate() {
            if sample.abs() > 1e-4 {
                if quiet > 100 || found.is_empty() {
                    found.push(at);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        found
    }

    #[test]
    fn should_click_on_every_beat_of_the_grid() {
        // 120 BPM at 48 kHz: a beat every 24 000 samples.
        let samples = render(&grid(120.0, 4), 0, 4 * 24_000);
        let found = onsets(&samples);
        assert_eq!(found.len(), 4);
        for (beat, at) in found.iter().enumerate() {
            // The sine starts at zero, so the first audible sample is just after the beat.
            assert!(
                (*at as i64 - (beat * 24_000) as i64).abs() <= 2,
                "beat {beat} at {at}"
            );
        }
    }

    #[test]
    fn should_not_be_audible_between_the_clicks() {
        let samples = render(&grid(120.0, 4), 0, 24_000);
        assert!(samples[2_000..24_000].iter().all(|s| s.abs() < 1e-3));
    }

    #[test]
    fn should_accent_the_first_beat_of_each_bar() {
        // The accent is higher: it crosses zero more often over the same stretch.
        let samples = render(&grid(120.0, 2), 0, 2 * 24_000);
        let crossings = |range: std::ops::Range<usize>| {
            samples[range]
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        assert!(crossings(0..1_000) > crossings(24_000..25_000));
    }

    #[test]
    fn should_give_the_same_samples_however_the_buffers_are_cut() {
        let grid = grid(133.3, 4);
        let whole = render(&grid, 10_000, 60_000);
        let mut cut = Vec::new();
        for start in (0..60_000).step_by(777) {
            cut.extend(render(
                &grid,
                10_000 + start as u64,
                777.min(60_000 - start),
            ));
        }
        assert_eq!(whole, cut);
    }

    #[test]
    fn should_land_on_the_grid_after_a_seek() {
        // Beat 3 of 120 BPM starts at sample 72 000: a buffer that starts there clicks at once.
        let samples = render(&grid(120.0, 4), 72_000, 2_000);
        assert!(samples[10..200].iter().any(|s| s.abs() > 0.1));
        // A buffer that starts in the middle of the ring continues it instead of restarting it.
        let samples = render(&grid(120.0, 4), 72_500, 100);
        assert!(samples.iter().any(|s| s.abs() > 0.01));
    }

    #[test]
    fn should_be_silent_while_the_click_is_off() {
        let shared = SharedMetronome::new(tempo(120.0, 4, false), 0.0);
        assert_eq!(shared.grid(RATE), None);
        shared.set(tempo(120.0, 4, true));
        assert!(shared.grid(RATE).is_some());
    }

    #[test]
    fn should_follow_a_new_tempo_without_being_rebuilt() {
        let shared = SharedMetronome::new(tempo(120.0, 4, true), 0.0);
        let slow = onsets(&render(&shared.grid(RATE).unwrap(), 0, 2 * 24_000));
        shared.set(tempo(240.0, 4, true));
        let fast = onsets(&render(&shared.grid(RATE).unwrap(), 0, 2 * 24_000));
        assert_eq!(slow.len(), 2);
        assert_eq!(fast.len(), 4);
    }

    #[test]
    fn should_keep_a_tempo_outside_the_range_in_it() {
        let shared = SharedMetronome::new(tempo(0.0, 4, true), 0.0);
        let bpm = |shared: &SharedMetronome| shared.grid(RATE).unwrap().map.bpm_at(0.0);
        assert_eq!(bpm(&shared), flux_project::MIN_BPM);
        shared.set(tempo(f64::NAN, 4, true));
        assert!(bpm(&shared).is_finite());
    }

    #[test]
    fn should_scale_with_the_level() {
        let loud = render(&grid(120.0, 4), 0, 2_000);
        let quiet = SharedMetronome::new(tempo(120.0, 4, true), -20.0)
            .grid(RATE)
            .unwrap();
        let quiet = render(&quiet, 0, 2_000);
        let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak(&quiet) / peak(&loud) - 0.1).abs() < 0.01);
    }

    #[test]
    fn should_never_clip_on_its_own() {
        let samples = render(&grid(400.0, 1), 0, RATE as usize);
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
    }

    /// A 25 fps timeline at 120 BPM in 4/4 that goes to 60 BPM in 3/4 after two seconds.
    fn changing() -> SharedMetronome {
        let mut timeline = Timeline::new("t", flux_project::FrameRate::Fps25);
        timeline.tempo = tempo(120.0, 4, true);
        timeline.tempo_changes.push(flux_project::TempoChange {
            id: "c".into(),
            at: 50,
            bpm: 60.0,
            beats_per_bar: 3,
            beat_unit: 4,
        });
        let shared = SharedMetronome::new(tempo(120.0, 4, true), 0.0);
        shared.set_timeline(&timeline);
        shared
    }

    #[test]
    fn should_click_on_the_beats_of_every_segment_of_the_map() {
        let grid = changing().grid(RATE).unwrap();
        // Four beats of 24 000 samples, then one every 48 000 from the change at 96 000.
        let found = onsets(&render(&grid, 0, 4 * RATE as usize));
        let expected = [0, 24_000, 48_000, 72_000, 96_000, 144_000];
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for (at, want) in found.iter().zip(expected) {
            assert!((*at as i64 - want).abs() <= 2, "{at} is not {want}");
        }
    }

    #[test]
    fn should_land_on_the_new_grid_after_a_seek_past_a_change() {
        let grid = changing().grid(RATE).unwrap();
        // Beat 2 of the second segment starts at 144 000.
        let found = onsets(&render(&grid, 143_000, 4_000));
        assert_eq!(found.len(), 1);
        assert!((found[0] as i64 - 1_000).abs() <= 2, "{found:?}");
        // Cut anywhere, the samples are the same.
        let whole = render(&grid, 90_000, 70_000);
        let mut cut = Vec::new();
        for start in (0..70_000).step_by(4_999) {
            cut.extend(render(
                &grid,
                90_000 + start as u64,
                4_999.min(70_000 - start),
            ));
        }
        assert_eq!(whole, cut);
    }

    #[test]
    fn should_accent_the_first_beat_of_the_bar_a_change_starts() {
        let grid = changing().grid(RATE).unwrap();
        let samples = render(&grid, 0, 4 * RATE as usize);
        let crossings = |from: usize| {
            samples[from..from + 1_000]
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // The change starts a bar at 96 000 although the old bar was complete; its second beat
        // at 144 000 is not accented.
        assert!(crossings(96_000) > crossings(144_000));
        assert!(crossings(0) > crossings(24_000));
    }
}
