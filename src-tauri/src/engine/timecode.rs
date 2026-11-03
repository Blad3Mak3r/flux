//! Timecode math: conversion between frame counts and `HH:MM:SS:FF` for the four supported rates.

use std::fmt;

use thiserror::Error;

use crate::project::FrameRate;

/// Frames in 10 minutes of 29.97 drop-frame: 2 frames are dropped in every minute except each tenth.
const DF_FRAMES_PER_10_MIN: u64 = 17_982;
/// Frames in a minute of 29.97 drop-frame that dropped frames 0 and 1.
const DF_FRAMES_PER_DROP_MINUTE: u64 = 1_798;

/// Timecode wraps after 24 hours, as LTC and MTC cannot express more.
const HOURS_PER_DAY: u64 = 24;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TimecodeError {
    #[error("{field} {value} is out of range (max {max})")]
    OutOfRange {
        field: &'static str,
        value: u8,
        max: u8,
    },
    #[error("frame {frame} does not exist at {minute} minutes in drop-frame timecode")]
    DroppedFrame { minute: u8, frame: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timecode {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
}

impl FrameRate {
    /// Frames counted per second on the display (30 for 29.97 drop-frame).
    pub fn nominal_fps(self) -> u8 {
        match self {
            FrameRate::Fps24 => 24,
            FrameRate::Fps25 => 25,
            FrameRate::Fps2997Drop | FrameRate::Fps30 => 30,
        }
    }

    /// The real frame rate as `(numerator, denominator)`: 30000 / 1001 for 29.97.
    pub fn fraction(self) -> (u64, u64) {
        match self {
            FrameRate::Fps24 => (24, 1),
            FrameRate::Fps25 => (25, 1),
            FrameRate::Fps2997Drop => (30_000, 1_001),
            FrameRate::Fps30 => (30, 1),
        }
    }

    pub fn is_drop_frame(self) -> bool {
        self == FrameRate::Fps2997Drop
    }

    /// Frames in one hour; the real rate is 29.97 for drop-frame, so it is 107 892, not 108 000.
    pub fn frames_per_hour(self) -> u64 {
        match self {
            FrameRate::Fps2997Drop => 6 * DF_FRAMES_PER_10_MIN,
            other => u64::from(other.nominal_fps()) * 3_600,
        }
    }

    pub fn frames_per_day(self) -> u64 {
        self.frames_per_hour() * HOURS_PER_DAY
    }
}

impl Timecode {
    /// Converts a frame count to timecode, wrapping after 24 hours.
    pub fn from_frames(frames: u64, rate: FrameRate) -> Self {
        let mut n = frames % rate.frames_per_day();
        if rate.is_drop_frame() {
            // Add back the dropped numbers so the count can be split as if it were 30 fps.
            let tens = n / DF_FRAMES_PER_10_MIN;
            let rest = n % DF_FRAMES_PER_10_MIN;
            n += 18 * tens;
            if rest >= 2 {
                n += 2 * ((rest - 2) / DF_FRAMES_PER_DROP_MINUTE);
            }
        }
        let fps = u64::from(rate.nominal_fps());
        // Every field is below 60 here, so the narrowing casts are lossless.
        Self {
            hours: (n / (fps * 3_600) % HOURS_PER_DAY) as u8,
            minutes: (n / (fps * 60) % 60) as u8,
            seconds: (n / fps % 60) as u8,
            frames: (n % fps) as u8,
        }
    }

    /// Converts timecode to a frame count, rejecting fields that do not exist at `rate`.
    pub fn to_frames(self, rate: FrameRate) -> Result<u64, TimecodeError> {
        let fps = rate.nominal_fps();
        check("hours", self.hours, HOURS_PER_DAY as u8 - 1)?;
        check("minutes", self.minutes, 59)?;
        check("seconds", self.seconds, 59)?;
        check("frames", self.frames, fps - 1)?;

        let fps = u64::from(fps);
        let total_minutes = u64::from(self.hours) * 60 + u64::from(self.minutes);
        let nominal =
            total_minutes * 60 * fps + u64::from(self.seconds) * fps + u64::from(self.frames);
        if !rate.is_drop_frame() {
            return Ok(nominal);
        }
        let first_second_of_dropping_minute = !self.minutes.is_multiple_of(10) && self.seconds == 0;
        if first_second_of_dropping_minute && self.frames < 2 {
            return Err(TimecodeError::DroppedFrame {
                minute: self.minutes,
                frame: self.frames,
            });
        }
        Ok(nominal - 2 * (total_minutes - total_minutes / 10))
    }

    pub fn display(self, rate: FrameRate) -> String {
        let separator = if rate.is_drop_frame() { ';' } else { ':' };
        format!(
            "{:02}:{:02}:{:02}{separator}{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

impl fmt::Display for Timecode {
    /// Non-drop notation; use [`Timecode::display`] to get the `;` separator for drop-frame.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}:{:02}",
            self.hours, self.minutes, self.seconds, self.frames
        )
    }
}

fn check(field: &'static str, value: u8, max: u8) -> Result<(), TimecodeError> {
    if value > max {
        return Err(TimecodeError::OutOfRange { field, value, max });
    }
    Ok(())
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

    fn tc(hours: u8, minutes: u8, seconds: u8, frames: u8) -> Timecode {
        Timecode {
            hours,
            minutes,
            seconds,
            frames,
        }
    }

    #[test]
    fn drop_frame_hour_has_107_892_frames() {
        assert_eq!(FrameRate::Fps2997Drop.frames_per_hour(), 107_892);
        assert_eq!(
            tc(1, 0, 0, 0).to_frames(FrameRate::Fps2997Drop),
            Ok(107_892)
        );
    }

    #[test]
    fn drop_frame_skips_frames_0_and_1_each_minute() {
        let rate = FrameRate::Fps2997Drop;
        assert_eq!(tc(0, 0, 59, 29).to_frames(rate), Ok(1799));
        assert_eq!(tc(0, 1, 0, 2).to_frames(rate), Ok(1800));
        assert_eq!(Timecode::from_frames(1800, rate), tc(0, 1, 0, 2));
    }

    #[test]
    fn drop_frame_keeps_frames_0_and_1_every_tenth_minute() {
        let rate = FrameRate::Fps2997Drop;
        assert_eq!(tc(0, 9, 59, 29).to_frames(rate), Ok(17_981));
        assert_eq!(tc(0, 10, 0, 0).to_frames(rate), Ok(17_982));
        assert_eq!(Timecode::from_frames(17_982, rate), tc(0, 10, 0, 0));
    }

    #[test]
    fn drop_frame_rejects_dropped_frames() {
        let rate = FrameRate::Fps2997Drop;
        assert_eq!(
            tc(0, 1, 0, 0).to_frames(rate),
            Err(TimecodeError::DroppedFrame {
                minute: 1,
                frame: 0
            })
        );
        assert_eq!(
            tc(0, 1, 0, 1).to_frames(rate),
            Err(TimecodeError::DroppedFrame {
                minute: 1,
                frame: 1
            })
        );
        assert!(tc(0, 10, 0, 0).to_frames(rate).is_ok());
    }

    #[test]
    fn non_drop_rates_use_their_nominal_rate() {
        assert_eq!(tc(0, 0, 1, 0).to_frames(FrameRate::Fps24), Ok(24));
        assert_eq!(tc(0, 0, 1, 0).to_frames(FrameRate::Fps25), Ok(25));
        assert_eq!(tc(1, 0, 0, 0).to_frames(FrameRate::Fps25), Ok(90_000));
        assert_eq!(tc(1, 0, 0, 0).to_frames(FrameRate::Fps30), Ok(108_000));
        assert_eq!(
            Timecode::from_frames(90_000, FrameRate::Fps25),
            tc(1, 0, 0, 0)
        );
    }

    #[test]
    fn rejects_fields_out_of_range() {
        assert_eq!(
            tc(24, 0, 0, 0).to_frames(FrameRate::Fps25),
            Err(TimecodeError::OutOfRange {
                field: "hours",
                value: 24,
                max: 23
            })
        );
        assert!(tc(0, 60, 0, 0).to_frames(FrameRate::Fps30).is_err());
        assert!(tc(0, 0, 60, 0).to_frames(FrameRate::Fps30).is_err());
        assert!(tc(0, 0, 0, 24).to_frames(FrameRate::Fps24).is_err());
        assert!(tc(0, 0, 0, 29).to_frames(FrameRate::Fps30).is_ok());
    }

    #[test]
    fn frames_round_trip_over_a_whole_day_for_every_rate() {
        for rate in RATES {
            for frame in 0..rate.frames_per_day() {
                let timecode = Timecode::from_frames(frame, rate);
                assert_eq!(
                    timecode.to_frames(rate),
                    Ok(frame),
                    "{rate:?} frame {frame}"
                );
            }
        }
    }

    #[test]
    fn counts_wrap_after_24_hours() {
        for rate in RATES {
            assert_eq!(
                Timecode::from_frames(rate.frames_per_day(), rate),
                tc(0, 0, 0, 0)
            );
            assert_eq!(
                Timecode::from_frames(rate.frames_per_day() - 1, rate).hours,
                23
            );
        }
    }

    #[test]
    fn display_uses_semicolon_only_for_drop_frame() {
        let timecode = tc(1, 2, 3, 4);
        assert_eq!(timecode.display(FrameRate::Fps2997Drop), "01:02:03;04");
        assert_eq!(timecode.display(FrameRate::Fps25), "01:02:03:04");
        assert_eq!(timecode.to_string(), "01:02:03:04");
    }
}
