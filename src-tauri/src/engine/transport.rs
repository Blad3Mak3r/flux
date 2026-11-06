//! The transport: a position counted in audio samples, and the maths that ties samples to frames.
//!
//! The audio device's sample clock is the master. The output callback advances the transport by
//! the number of samples it produces, and the LTC and MTC generators are driven by that same
//! position, so playback, LTC and MTC cannot drift apart. Nothing here allocates or locks.

use serde::Serialize;

use crate::project::FrameRate;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Play,
    Pause,
    /// Stops and returns to the start of the timeline.
    Stop,
    /// Moves to a timecode, given as a frame count like every position in the project. Positions
    /// before the timeline start go to the start.
    Seek(u64),
}

#[derive(Debug, Clone)]
pub struct Transport {
    clock: Clock,
    /// Frame count of the timeline origin (`project.timecode.start`).
    start: u64,
    state: TransportState,
    /// Samples played since the timeline origin.
    position: u64,
    /// Changes whenever the position jumps or playback starts, so generators know to resync.
    epoch: u64,
    /// Samples at which the show ends, if it has an end (see [`Transport::set_end`]).
    end: Option<u64>,
}

impl Transport {
    pub fn new(clock: Clock, start: u64) -> Self {
        Self {
            clock,
            start,
            state: TransportState::Stopped,
            position: 0,
            epoch: 0,
            end: None,
        }
    }

    /// Sets where the show ends, in samples since the timeline origin: the latest end among the
    /// clips and cues. Once the position reaches it while playing, the transport stops by itself
    /// (see [`advance`](Self::advance)) and a seek past it goes to the end. A show with no end, or
    /// one that ends at the origin (nothing to play), never stops by itself, so the generators can
    /// still run without a programme. It can change while playing, as the clips are edited.
    pub fn set_end(&mut self, end: Option<u64>) {
        self.end = end.filter(|&end| end > 0);
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Play => {
                if self.state != TransportState::Playing {
                    self.state = TransportState::Playing;
                    self.epoch += 1;
                }
            }
            Command::Pause => {
                if self.state == TransportState::Playing {
                    self.state = TransportState::Paused;
                }
            }
            Command::Stop => {
                self.state = TransportState::Stopped;
                self.position = 0;
                self.epoch += 1;
            }
            Command::Seek(frame) => {
                let position = self
                    .clock
                    .frames_to_samples(frame.saturating_sub(self.start));
                self.position = self.end.map_or(position, |end| position.min(end));
                self.epoch += 1;
            }
        }
    }

    /// Called by the output callback with the number of samples it just produced. Reaching the end
    /// of the show stops the transport like [`Command::Stop`]: it goes back to the start, ready to
    /// run again, and the epoch changes so the generators resync. Returns whether this call was the
    /// one that ended the show, which a stop from the user is not.
    pub fn advance(&mut self, samples: u64) -> bool {
        if self.state != TransportState::Playing {
            return false;
        }
        self.position += samples;
        let ended = self.end.is_some_and(|end| self.position >= end);
        if ended {
            self.apply(Command::Stop);
        }
        ended
    }

    pub fn state(&self) -> TransportState {
        self.state
    }

    pub fn clock(&self) -> Clock {
        self.clock
    }

    /// Frame count of the timeline origin.
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Samples played since the timeline origin.
    pub fn position_samples(&self) -> u64 {
        self.position
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The frame count of the timecode being played, including the start offset.
    pub fn frame(&self) -> u64 {
        self.start + self.clock.samples_to_frames(self.position)
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

    fn transport(start: u64) -> Transport {
        Transport::new(Clock::new(48_000, FrameRate::Fps25), start)
    }

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

    #[test]
    fn transport_should_only_advance_while_playing() {
        let mut t = transport(0);
        t.advance(1_000);
        assert_eq!(t.position_samples(), 0);

        t.apply(Command::Play);
        t.advance(1_000);
        assert_eq!(t.position_samples(), 1_000);

        t.apply(Command::Pause);
        t.advance(1_000);
        assert_eq!(t.position_samples(), 1_000);
        assert_eq!(t.state(), TransportState::Paused);
    }

    #[test]
    fn stop_should_return_to_the_start() {
        let mut t = transport(0);
        t.apply(Command::Play);
        t.advance(48_000);
        t.apply(Command::Stop);
        assert_eq!(t.state(), TransportState::Stopped);
        assert_eq!(t.position_samples(), 0);
    }

    #[test]
    fn seek_should_move_to_a_timecode_and_keep_the_state() {
        let mut t = transport(90_000);
        t.apply(Command::Play);
        t.apply(Command::Seek(90_000 + 25));
        assert_eq!(t.position_samples(), 48_000);
        assert_eq!(t.frame(), 90_025);
        assert_eq!(t.state(), TransportState::Playing);
    }

    #[test]
    fn seek_before_the_start_should_go_to_the_start() {
        let mut t = transport(90_000);
        t.apply(Command::Seek(10));
        assert_eq!(t.position_samples(), 0);
        assert_eq!(t.frame(), 90_000);
    }

    #[test]
    fn transport_should_stop_by_itself_when_the_position_reaches_the_end() {
        let mut t = transport(0);
        t.set_end(Some(48_000));
        t.apply(Command::Play);
        let epoch = t.epoch();
        t.advance(47_999);
        assert_eq!(t.state(), TransportState::Playing);
        t.advance(1);
        assert_eq!(t.state(), TransportState::Stopped);
        assert_eq!(t.position_samples(), 0);
        assert_ne!(t.epoch(), epoch);
    }

    #[test]
    fn transport_should_not_stop_while_paused_or_stopped_at_the_end() {
        let mut t = transport(0);
        t.set_end(Some(1_000));
        t.apply(Command::Play);
        t.advance(500);
        t.apply(Command::Pause);
        t.advance(10_000);
        assert_eq!(t.state(), TransportState::Paused);
        assert_eq!(t.position_samples(), 500);
    }

    #[test]
    fn transport_should_keep_playing_when_the_end_moves_away_while_playing() {
        let mut t = transport(0);
        t.set_end(Some(1_000));
        t.apply(Command::Play);
        t.advance(900);
        t.set_end(Some(5_000));
        t.advance(900);
        assert_eq!(t.state(), TransportState::Playing);
        assert_eq!(t.position_samples(), 1_800);
        t.set_end(Some(1_000));
        t.advance(1);
        assert_eq!(
            t.state(),
            TransportState::Stopped,
            "an end that came closer"
        );
    }

    #[test]
    fn transport_should_run_forever_when_the_show_has_no_end() {
        let mut t = transport(0);
        t.apply(Command::Play);
        t.advance(u32::MAX.into());
        assert_eq!(t.state(), TransportState::Playing);

        t.set_end(Some(0));
        t.advance(1);
        assert_eq!(
            t.state(),
            TransportState::Playing,
            "an empty show has no end"
        );
    }

    #[test]
    fn seek_past_the_end_should_go_to_the_end() {
        let mut t = transport(0);
        t.set_end(Some(48_000));
        t.apply(Command::Seek(25 * 10));
        assert_eq!(t.position_samples(), 48_000);
        assert_eq!(t.frame(), 25);

        t.apply(Command::Play);
        t.advance(1_920);
        assert_eq!(
            t.state(),
            TransportState::Stopped,
            "playing from the end ends at once"
        );
    }

    #[test]
    fn frame_should_include_the_start_offset() {
        let mut t = transport(90_000);
        t.apply(Command::Play);
        t.advance(48_000 + 1_919);
        assert_eq!(t.frame(), 90_000 + 25);
        t.advance(1);
        assert_eq!(t.frame(), 90_000 + 26);
    }

    #[test]
    fn epoch_should_change_on_start_stop_and_seek_but_not_on_pause_or_repeats() {
        let mut t = transport(0);
        let mut epoch = t.epoch();
        let mut changed = |t: &Transport| {
            let moved = t.epoch() != epoch;
            epoch = t.epoch();
            moved
        };

        t.apply(Command::Play);
        assert!(changed(&t));
        t.apply(Command::Play);
        assert!(!changed(&t));
        t.apply(Command::Pause);
        assert!(!changed(&t));
        t.apply(Command::Play);
        assert!(changed(&t));
        t.apply(Command::Seek(100));
        assert!(changed(&t));
        t.apply(Command::Stop);
        assert!(changed(&t));
    }

    #[test]
    fn advance_should_say_so_when_it_ends_the_show() {
        let mut t = transport(0);
        t.set_end(Some(1_000));
        t.apply(Command::Play);

        assert!(!t.advance(999));
        assert!(t.advance(1));
    }

    #[test]
    fn advance_should_not_say_it_ended_a_show_that_was_stopped_by_hand() {
        let mut t = transport(0);
        t.set_end(Some(1_000));
        t.apply(Command::Play);
        t.apply(Command::Stop);

        assert!(!t.advance(5_000));
    }
}
