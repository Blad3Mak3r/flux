//! The metronome: a click on every beat of the timeline.
//!
//! A click is a pure function of the transport position, so it needs no state of its own: whatever
//! buffer is asked for, the clicks inside it are where the beat grid puts them, a seek lands on the
//! grid at once, and a stream that plays on another device only has to know where the master is.
//! Beats are counted from the timeline origin (sample 0 of the transport), beat 0 of every bar
//! being the accent.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use flux_project::{MAX_BPM, MIN_BPM, Tempo};

/// How long a click rings, in seconds.
const CLICK_SECONDS: f64 = 0.03;
/// Time constant of the click's decay, in seconds.
const DECAY_SECONDS: f32 = 0.006;
const ACCENT_HZ: f32 = 1_600.0;
const BEAT_HZ: f32 = 1_000.0;
/// Peak of a click at 0 dB; a click is short and bright, so it sounds louder than its peak says.
const PEAK: f32 = 0.7;

/// The tempo and level the click is made from, kept in atomics: the control side changes the tempo
/// while the show plays and the audio callbacks read it without locking or allocating.
#[derive(Debug)]
pub struct SharedMetronome {
    bpm: AtomicU64,
    beats_per_bar: AtomicU32,
    enabled: AtomicBool,
    /// Linear amplitude, fixed while the output is open.
    gain: f32,
}

impl SharedMetronome {
    pub fn new(tempo: Tempo, gain_db: f32) -> Self {
        let shared = Self {
            bpm: AtomicU64::new(0),
            beats_per_bar: AtomicU32::new(1),
            enabled: AtomicBool::new(false),
            gain: if gain_db.is_finite() {
                10f32.powf(gain_db / 20.0)
            } else {
                1.0
            },
        };
        shared.set(tempo);
        shared
    }

    /// Follows an edit of the timeline's tempo or of its click switch. A tempo outside the range a
    /// timeline can hold is brought into it, so the callbacks never divide by something absurd.
    pub fn set(&self, tempo: Tempo) {
        let bpm = if tempo.bpm.is_finite() {
            tempo.bpm.clamp(MIN_BPM, MAX_BPM)
        } else {
            120.0
        };
        self.bpm.store(bpm.to_bits(), Ordering::Relaxed);
        self.beats_per_bar
            .store(u32::from(tempo.beats_per_bar.max(1)), Ordering::Relaxed);
        self.enabled.store(tempo.click, Ordering::Relaxed);
    }

    /// The grid to click on at `rate` samples per second, or `None` while the click is off.
    pub fn grid(&self, rate: u32) -> Option<Grid> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let bpm = f64::from_bits(self.bpm.load(Ordering::Relaxed));
        Some(Grid {
            beat_samples: f64::from(rate) * 60.0 / bpm,
            beats_per_bar: u64::from(self.beats_per_bar.load(Ordering::Relaxed)),
            rate,
            gain: self.gain * PEAK,
        })
    }
}

/// The beat grid at one sample rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    beat_samples: f64,
    beats_per_bar: u64,
    rate: u32,
    gain: f32,
}

impl Grid {
    /// Adds the clicks that sound in `out`, whose first sample is transport sample `position`.
    pub fn add(&self, position: u64, out: &mut [f32]) {
        let ring = (f64::from(self.rate) * CLICK_SECONDS) as u64;
        let decay = DECAY_SECONDS * self.rate as f32;
        for (at, sample) in out.iter_mut().enumerate() {
            let p = position + at as u64;
            // The beat a sample belongs to is the last one that started at or before it.
            let mut beat = (p as f64 / self.beat_samples) as u64;
            if self.start_of(beat) > p {
                beat = beat.saturating_sub(1);
            }
            let offset = p.saturating_sub(self.start_of(beat));
            if offset >= ring {
                continue;
            }
            let hz = if beat.is_multiple_of(self.beats_per_bar) {
                ACCENT_HZ
            } else {
                BEAT_HZ
            };
            let t = offset as f32;
            *sample += (TAU * hz * t / self.rate as f32).sin() * (-t / decay).exp() * self.gain;
        }
    }

    /// First sample of `beat`.
    fn start_of(&self, beat: u64) -> u64 {
        (beat as f64 * self.beat_samples).round() as u64
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
        let beat = shared.grid(RATE).unwrap().beat_samples;
        assert!(beat <= f64::from(RATE) * 60.0 / MIN_BPM + 1.0);
        shared.set(tempo(f64::NAN, 4, true));
        assert!(shared.grid(RATE).unwrap().beat_samples.is_finite());
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
}
