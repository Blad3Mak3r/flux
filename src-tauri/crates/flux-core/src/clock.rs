//! Samples against frames: the one clock every generator and scheduler counts with.

use crate::timecode::FrameRate;

/// Converts between samples and the frames, bit cells and quarter frames of one frame rate.
///
/// A frame rarely lasts a whole number of samples (1601.6 at 29.97 fps and 48 kHz), so every
/// boundary is the exact fraction rounded to the nearest sample. Boundaries never accumulate
/// error: 1000 frames of 29.97 fps are exactly 1 601 600 samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    sample_rate: u64,
    /// Frame rate as the fraction `fps_num / fps_den` (30000 / 1001 for 29.97).
    fps_num: u64,
    fps_den: u64,
}

impl Clock {
    pub fn new(sample_rate: u32, rate: FrameRate) -> Self {
        let (fps_num, fps_den) = rate.fraction();
        Self {
            sample_rate: u64::from(sample_rate),
            fps_num,
            fps_den,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }

    /// Sample at which tick number `ticks` starts, when each frame is divided into `per_frame`
    /// equal ticks (1 for frames, 4 for MTC quarter frames, 160 for LTC half cells).
    pub fn ticks_to_samples(&self, ticks: u64, per_frame: u64) -> u64 {
        let per_second = u128::from(per_frame * self.fps_num);
        let scaled = u128::from(ticks) * u128::from(self.sample_rate) * u128::from(self.fps_den);
        ((2 * scaled + per_second) / (2 * per_second)) as u64
    }

    /// The tick that is playing at `sample`: the last one whose start is not after it.
    pub fn samples_to_ticks(&self, sample: u64, per_frame: u64) -> u64 {
        let per_second = u128::from(per_frame * self.fps_num);
        let ticks = (u128::from(sample) * per_second
            / (u128::from(self.sample_rate) * u128::from(self.fps_den))) as u64;
        // The floor can be one short, because starts are rounded to the nearest sample.
        if self.ticks_to_samples(ticks + 1, per_frame) <= sample {
            ticks + 1
        } else {
            ticks
        }
    }

    pub fn frames_to_samples(&self, frames: u64) -> u64 {
        self.ticks_to_samples(frames, 1)
    }

    /// `ms` milliseconds as samples, negative when `ms` is.
    pub fn ms_to_samples(&self, ms: i32) -> i64 {
        i64::from(ms) * i64::from(self.sample_rate()) / 1000
    }

    pub fn samples_to_frames(&self, samples: u64) -> u64 {
        self.samples_to_ticks(samples, 1)
    }

    /// An upper bound for the samples of one frame, to size buffers up front.
    pub fn max_samples_per_frame(&self) -> usize {
        (self.sample_rate * self.fps_den).div_ceil(self.fps_num) as usize + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [FrameRate; 4] = [
        FrameRate::Fps24,
        FrameRate::Fps25,
        FrameRate::Fps2997Drop,
        FrameRate::Fps30,
    ];

    #[test]
    fn frames_should_not_drift_when_the_sample_count_per_frame_is_fractional() {
        let clock = Clock::new(48_000, FrameRate::Fps2997Drop);
        assert_eq!(clock.frames_to_samples(1_000), 1_601_600);
        assert_eq!(
            Clock::new(44_100, FrameRate::Fps24).frames_to_samples(24),
            44_100
        );
    }

    #[test]
    fn samples_and_frames_should_round_trip_at_every_boundary() {
        for rate in RATES {
            for sample_rate in [44_100, 48_000, 96_000] {
                let clock = Clock::new(sample_rate, rate);
                for frame in 1..3_000 {
                    let start = clock.frames_to_samples(frame);
                    assert_eq!(
                        clock.samples_to_frames(start),
                        frame,
                        "{rate:?} {sample_rate}"
                    );
                    assert_eq!(
                        clock.samples_to_frames(start - 1),
                        frame - 1,
                        "{rate:?} {sample_rate}"
                    );
                }
            }
        }
    }

    #[test]
    fn ticks_should_split_a_frame_into_equal_parts() {
        let clock = Clock::new(48_000, FrameRate::Fps25);
        // 1920 samples per frame.
        assert_eq!(clock.ticks_to_samples(1, 4), 480);
        assert_eq!(clock.ticks_to_samples(8, 4), 3_840);
        assert_eq!(clock.ticks_to_samples(160, 160), 1_920);
        assert_eq!(clock.samples_to_ticks(479, 4), 0);
        assert_eq!(clock.samples_to_ticks(480, 4), 1);
    }

    #[test]
    fn max_samples_per_frame_should_cover_every_frame() {
        for rate in RATES {
            let clock = Clock::new(48_000, rate);
            let longest = (0..2_000)
                .map(|f| clock.frames_to_samples(f + 1) - clock.frames_to_samples(f))
                .max()
                .unwrap();
            assert!(clock.max_samples_per_frame() as u64 >= longest, "{rate:?}");
        }
    }
}
