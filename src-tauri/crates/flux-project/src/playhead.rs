//! Where a clip launched on the VJ grid is in its video, and how it moves: the clip's transport
//! (speed, direction, tempo, region and what it does at the end of the region) turned into a
//! position on the grid's clock. The picture ([`Playhead::at`]) and the sound ([`Course`]) fold
//! the same number the same way, so they never disagree.
//!
//! A [`Playhead`] is anchored: it holds where the clip was, in `u`, when it was last anchored, and
//! the clock then. `u` is counted in lengths of the clip's region from its start, before the
//! region's end is dealt with: 0 is the start of the region, 1 its end, and it goes on to 2, 3...
//! when the clip loops, or below 0 when it plays backwards past the start. [`fold`] turns it into
//! a place in the region by the clip's playback (loop, once, ping-pong). Any change of the clip's
//! transport anchors again from where the playhead is ([`Playhead::retime`]), so a change is
//! never a jump.

use std::time::Duration;

use crate::vj::{Clip, Direction, Playback, Region, Tempo};

/// How far inside the end of the region a jump to it lands, so that a clip played once has not
/// ended there.
const EDGE: f64 = 1e-9;

/// Where the grid's clock is: its time, and the beats it has counted by then.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tick {
    pub time: Duration,
    pub beats: f64,
}

impl From<Duration> for Tick {
    /// A tick that counted no beats, for what does not follow the tempo.
    fn from(time: Duration) -> Self {
        Self { time, beats: 0.0 }
    }
}

/// What decides how a clip moves, copied from the clip: when it is not the same as the clip's own,
/// the playhead anchors again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    pub speed: f32,
    pub direction: Direction,
    pub tempo: Tempo,
    pub region: Region,
    pub playback: Playback,
    /// A generator has no video: its time runs with the speed and direction, and nothing else.
    pub generator: bool,
}

impl Motion {
    pub fn of(clip: &Clip) -> Self {
        let generator = clip.generator.is_some();
        Self {
            speed: clip.speed,
            direction: clip.direction,
            tempo: if generator { Tempo::Free } else { clip.tempo },
            region: if generator {
                Region::default()
            } else {
                clip.region
            },
            playback: clip.playback,
            generator,
        }
    }

    /// How many seconds one pass through the region lasts at `bpm`, for a video of `length`
    /// seconds; `None` when it does not move.
    pub fn pass_seconds(&self, length: f64, bpm: f64) -> Option<f64> {
        if self.direction == Direction::Pause {
            return None;
        }
        match self.tempo {
            Tempo::Free => Some(f64::from(self.region.span()) * length / f64::from(self.speed)),
            Tempo::Beats { beats } if bpm > 0.0 => Some(f64::from(beats) * 60.0 / bpm),
            Tempo::Beats { .. } => None,
        }
    }

    /// How fast `u` moves, in lengths of the region a second, at `bpm`, for a video of `length`
    /// seconds: negative backwards, 0 paused.
    pub fn rate(&self, length: f64, bpm: f64) -> f64 {
        let sign = self.direction.sign();
        match self.tempo {
            Tempo::Free => sign * f64::from(self.speed) / (f64::from(self.region.span()) * length),
            Tempo::Beats { beats } => sign * bpm / 60.0 / f64::from(beats),
        }
    }

    /// Where in the clip `u` is, 0 to 1 of its whole length; `None` once a clip played once has
    /// ended.
    pub fn place(&self, u: f64) -> Option<f64> {
        let inside = fold(u, self.playback, self.direction)?;
        let (start, span) = (f64::from(self.region.start), f64::from(self.region.span()));
        Some(start + inside * span)
    }

    /// The `u` that is at `place`, a fraction of the whole clip brought into the region.
    fn u_at(&self, place: f64) -> f64 {
        let (start, span) = (f64::from(self.region.start), f64::from(self.region.span()));
        ((place - start) / span).clamp(0.0, 1.0 - EDGE)
    }
}

/// Where in its region a clip is when `u` lengths of it have been played, 0 to 1; `None` once a
/// clip played once has ended: forwards at the end, backwards past the start. A paused clip that
/// has ended stays so.
pub fn fold(u: f64, playback: Playback, direction: Direction) -> Option<f64> {
    match playback {
        Playback::Loop => {
            let inside = u - u.floor();
            // Backwards from the end of the region is the end, not the start.
            let at_end = inside == 0.0 && u != 0.0 && direction == Direction::Reverse;
            Some(if at_end { 1.0 } else { inside })
        }
        Playback::Once => match direction {
            Direction::Reverse => (u >= 0.0).then_some(u.min(1.0)),
            Direction::Forward => (u < 1.0).then_some(u.max(0.0)),
            Direction::Pause => (0.0..=1.0).contains(&u).then_some(u),
        },
        Playback::PingPong => {
            let round = u.rem_euclid(2.0);
            Some(if round <= 1.0 { round } else { 2.0 - round })
        }
    }
}

/// Where a launched clip is, anchored: it was at `u` when the grid's clock read `time` and had
/// counted `beats`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    time: Duration,
    beats: f64,
    u: f64,
    motion: Motion,
}

impl Playhead {
    /// A clip launched at `tick`: at the start of its region, or at the end when it plays backwards.
    pub fn launch(motion: Motion, tick: Tick) -> Self {
        let u = if motion.direction == Direction::Reverse && !motion.generator {
            1.0
        } else {
            0.0
        };
        Self {
            time: tick.time,
            beats: tick.beats,
            u,
            motion,
        }
    }

    pub fn motion(&self) -> &Motion {
        &self.motion
    }

    /// How many lengths of the region have been played at `tick`, for a video of `length` seconds
    /// (a generator's is 1).
    pub fn phase(&self, tick: Tick, length: f64) -> f64 {
        let sign = self.motion.direction.sign();
        match self.motion.tempo {
            Tempo::Free => {
                let seconds = tick.time.as_secs_f64() - self.time.as_secs_f64();
                self.u + seconds * self.motion.rate(length, 0.0)
            }
            Tempo::Beats { beats } => self.u + (tick.beats - self.beats) * sign / f64::from(beats),
        }
    }

    /// Where in the clip the playhead is at `tick`, 0 to 1 of its length; `None` once a clip played
    /// once has ended.
    pub fn at(&self, tick: Tick, length: f64) -> Option<f64> {
        self.motion.place(self.phase(tick, length))
    }

    /// How long a generator has played at `tick`, in seconds: it never goes before its start.
    pub fn seconds(&self, tick: Tick) -> f64 {
        self.phase(tick, 1.0).max(0.0)
    }

    /// Moves the playhead to `place`, a fraction of the clip's length, brought into its region.
    pub fn jump(&mut self, tick: Tick, place: f64) {
        self.time = tick.time;
        self.beats = tick.beats;
        self.u = self.motion.u_at(place);
    }

    /// Follows `motion`, which the clip now has: anchors again from where the playhead is at
    /// `tick`, so it goes on from there at the new pace. Nothing happens while it is the same.
    pub fn retime(&mut self, motion: Motion, tick: Tick, length: f64) {
        if motion == self.motion {
            return;
        }
        let now = self.phase(tick, length);
        let place = self.motion.place(now);
        let back = self.motion.playback == Playback::PingPong && now.rem_euclid(2.0) > 1.0;
        let mut u = match place {
            // The region moved or changed size: the place stays, and so does the leg of a bounce.
            Some(place) if motion.region != self.motion.region => {
                let inside = motion.u_at(place);
                if back { 2.0 - inside } else { inside }
            }
            _ => now,
        };
        // A clip that played once and ended starts again from the edge it now moves away from.
        if motion.playback == Playback::Once && motion.direction != self.motion.direction {
            u = u.clamp(0.0, 1.0);
        }
        *self = Self {
            time: tick.time,
            beats: tick.beats,
            u,
            motion,
        };
    }

    /// What the sound needs to follow the playhead from `tick` on, at `bpm`.
    pub fn course(&self, tick: Tick, length: f64, bpm: f64) -> Course {
        Course {
            time: tick.time,
            u: self.phase(tick, length),
            rate: self.motion.rate(length, bpm),
            motion: self.motion,
        }
    }
}

/// A playhead's movement from an instant on, at the pace it has then: `u` was `u` at `time`, and
/// moves `rate` lengths of the region a second. What the audio callback reads, as it cannot count
/// beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Course {
    pub time: Duration,
    pub u: f64,
    pub rate: f64,
    pub motion: Motion,
}

impl Course {
    /// Where in the clip the course is at `time` on the grid's clock, 0 to 1 of its length.
    pub fn at(&self, time: Duration) -> Option<f64> {
        self.at_seconds(time.as_secs_f64() - self.time.as_secs_f64())
    }

    /// Where in the clip the course is `seconds` after its anchor.
    pub fn at_seconds(&self, seconds: f64) -> Option<f64> {
        self.motion.place(self.u + seconds * self.rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn motion() -> Motion {
        Motion {
            speed: 1.0,
            direction: Direction::Forward,
            tempo: Tempo::Free,
            region: Region::default(),
            playback: Playback::Loop,
            generator: false,
        }
    }

    fn at(seconds: f64) -> Tick {
        Duration::from_secs_f64(seconds).into()
    }

    fn beat(seconds: f64, beats: f64) -> Tick {
        Tick {
            time: Duration::from_secs_f64(seconds),
            beats,
        }
    }

    fn near(a: Option<f64>, b: f64) {
        let a = a.expect("a place");
        assert!((a - b).abs() < 1e-6, "{a} is not {b}");
    }

    /// A clip of 10 seconds.
    const LENGTH: f64 = 10.0;

    #[test]
    fn it_should_play_forwards_backwards_and_hold_still() {
        let forward = Playhead::launch(motion(), at(1.0));
        near(forward.at(at(3.5), LENGTH), 0.25);
        let reverse = Playhead::launch(
            Motion {
                direction: Direction::Reverse,
                ..motion()
            },
            at(1.0),
        );
        near(reverse.at(at(1.0), LENGTH), 1.0);
        near(reverse.at(at(3.0), LENGTH), 0.8);
        let paused = Playhead::launch(
            Motion {
                direction: Direction::Pause,
                ..motion()
            },
            at(1.0),
        );
        near(paused.at(at(30.0), LENGTH), 0.0);
    }

    #[test]
    fn a_change_of_speed_should_go_on_from_where_the_playhead_is() {
        let mut head = Playhead::launch(motion(), at(0.0));
        near(head.at(at(2.0), LENGTH), 0.2);
        head.retime(
            Motion {
                speed: 2.0,
                ..motion()
            },
            at(2.0),
            LENGTH,
        );
        near(head.at(at(2.0), LENGTH), 0.2);
        near(head.at(at(3.0), LENGTH), 0.4);
        head.retime(
            Motion {
                speed: 2.0,
                direction: Direction::Reverse,
                ..motion()
            },
            at(3.0),
            LENGTH,
        );
        near(head.at(at(4.0), LENGTH), 0.2);
        head.retime(
            Motion {
                direction: Direction::Pause,
                ..motion()
            },
            at(4.0),
            LENGTH,
        );
        near(head.at(at(9.0), LENGTH), 0.2);
    }

    #[test]
    fn a_region_should_hold_the_loop_once_and_ping_pong() {
        let region = Region {
            start: 0.2,
            end: 0.6,
        };
        let play = |playback, direction| {
            Playhead::launch(
                Motion {
                    region,
                    playback,
                    direction,
                    ..motion()
                },
                at(0.0),
            )
        };
        // The region lasts 4 seconds: from 2 s to 6 s of the clip.
        let looped = play(Playback::Loop, Direction::Forward);
        near(looped.at(at(0.0), LENGTH), 0.2);
        near(looped.at(at(1.0), LENGTH), 0.3);
        near(looped.at(at(5.0), LENGTH), 0.3);
        let once = play(Playback::Once, Direction::Forward);
        near(once.at(at(3.9), LENGTH), 0.59);
        assert_eq!(once.at(at(4.01), LENGTH), None);
        let bounce = play(Playback::PingPong, Direction::Forward);
        near(bounce.at(at(5.0), LENGTH), 0.5);
        near(bounce.at(at(8.5), LENGTH), 0.2 + 0.4 * 0.125);
        // Backwards: from the end of the region.
        let looped = play(Playback::Loop, Direction::Reverse);
        near(looped.at(at(0.0), LENGTH), 0.6);
        near(looped.at(at(1.0), LENGTH), 0.5);
        near(looped.at(at(5.0), LENGTH), 0.5);
        let once = play(Playback::Once, Direction::Reverse);
        near(once.at(at(3.9), LENGTH), 0.21);
        assert_eq!(
            once.at(at(4.1), LENGTH),
            None,
            "ends at the start of the region"
        );
        let bounce = play(Playback::PingPong, Direction::Reverse);
        near(bounce.at(at(5.0), LENGTH), 0.3);
    }

    #[test]
    fn a_pass_of_beats_should_last_exactly_that_many_beats_and_follow_the_tempo() {
        let region = Region {
            start: 0.0,
            end: 0.5,
        };
        let beats = Motion {
            tempo: Tempo::Beats { beats: 4 },
            region,
            speed: 8.0,
            ..motion()
        };
        let head = Playhead::launch(beats, beat(0.0, 0.0));
        // At 120 bpm a beat is half a second: 4 beats are 2 seconds, whatever the speed.
        near(head.at(beat(1.0, 2.0), LENGTH), 0.25);
        near(head.at(beat(1.99, 3.98), LENGTH), 0.0 + 0.5 * 0.995);
        near(head.at(beat(2.0, 4.0), LENGTH), 0.0);
        // The tempo drops to 60 bpm after 1 beat (0.5 s): the pass goes on by the beats counted.
        let beats_after = 1.0 + (3.0 - 0.5) * 1.0;
        near(head.at(beat(3.0, beats_after), LENGTH), 0.5 * (3.5 / 4.0));
        assert_eq!(beats.pass_seconds(LENGTH, 120.0), Some(2.0));
        assert_eq!(beats.pass_seconds(LENGTH, 60.0), Some(4.0));
        assert_eq!(motion().pass_seconds(LENGTH, 60.0), Some(10.0));
        assert_eq!(
            Motion {
                speed: 2.0,
                ..motion()
            }
            .pass_seconds(LENGTH, 60.0),
            Some(5.0)
        );
    }

    #[test]
    fn a_change_of_tempo_or_region_should_not_jump() {
        let mut head = Playhead::launch(motion(), beat(0.0, 0.0));
        near(head.at(beat(4.0, 8.0), LENGTH), 0.4);
        head.retime(
            Motion {
                tempo: Tempo::Beats { beats: 8 },
                ..motion()
            },
            beat(4.0, 8.0),
            LENGTH,
        );
        near(head.at(beat(4.0, 8.0), LENGTH), 0.4);
        near(head.at(beat(5.0, 10.0), LENGTH), 0.4 + 2.0 / 8.0);
        // The region shrinks around the playhead: the place stays.
        let narrow = Motion {
            tempo: Tempo::Beats { beats: 8 },
            region: Region {
                start: 0.3,
                end: 0.7,
            },
            ..motion()
        };
        head.retime(narrow, beat(5.0, 10.0), LENGTH);
        near(head.at(beat(5.0, 10.0), LENGTH), 0.65);
        // And past the new end it is brought to the end.
        let narrower = Motion {
            region: Region {
                start: 0.1,
                end: 0.5,
            },
            ..narrow
        };
        head.retime(narrower, beat(5.0, 10.0), LENGTH);
        near(head.at(beat(5.0, 10.0), LENGTH), 0.5);
    }

    #[test]
    fn a_region_change_should_keep_the_leg_of_a_bounce() {
        let bounce = Motion {
            playback: Playback::PingPong,
            ..motion()
        };
        let mut head = Playhead::launch(bounce, at(0.0));
        // 12 s: on the way back, at 0.8.
        near(head.at(at(12.0), LENGTH), 0.8);
        let narrow = Motion {
            region: Region {
                start: 0.0,
                end: 0.9,
            },
            ..bounce
        };
        head.retime(narrow, at(12.0), LENGTH);
        near(head.at(at(12.0), LENGTH), 0.8);
        near(head.at(at(13.0), LENGTH), 0.7);
    }

    #[test]
    fn a_jump_should_land_in_the_region_and_wake_a_clip_that_ended() {
        let once = Motion {
            playback: Playback::Once,
            region: Region {
                start: 0.2,
                end: 0.6,
            },
            ..motion()
        };
        let mut head = Playhead::launch(once, at(0.0));
        assert_eq!(head.at(at(9.0), LENGTH), None);
        head.jump(at(9.0), 0.4);
        near(head.at(at(9.0), LENGTH), 0.4);
        near(head.at(at(10.0), LENGTH), 0.5);
        head.jump(at(10.0), 0.9);
        near(head.at(at(10.0), LENGTH), 0.6);
        head.jump(at(10.0), 0.0);
        near(head.at(at(10.0), LENGTH), 0.2);
    }

    #[test]
    fn a_clip_that_ended_should_play_again_the_other_way() {
        let once = Motion {
            playback: Playback::Once,
            ..motion()
        };
        let mut head = Playhead::launch(once, at(0.0));
        assert_eq!(head.at(at(11.0), LENGTH), None);
        head.retime(
            Motion {
                direction: Direction::Reverse,
                ..once
            },
            at(11.0),
            LENGTH,
        );
        near(head.at(at(11.0), LENGTH), 1.0);
        near(head.at(at(12.0), LENGTH), 0.9);
    }

    #[test]
    fn a_generator_should_run_on_its_own_time_with_speed_and_direction() {
        let generator = Motion {
            generator: true,
            speed: 2.0,
            ..motion()
        };
        let mut head = Playhead::launch(generator, at(1.0));
        assert!((head.seconds(at(4.0)) - 6.0).abs() < 1e-9);
        head.retime(
            Motion {
                direction: Direction::Reverse,
                ..generator
            },
            at(4.0),
            1.0,
        );
        assert!((head.seconds(at(5.0)) - 4.0).abs() < 1e-9);
        assert_eq!(head.seconds(at(100.0)), 0.0, "never before its start");
    }

    #[test]
    fn a_course_should_fold_as_the_playhead_does() {
        let beats = Motion {
            tempo: Tempo::Beats { beats: 2 },
            region: Region {
                start: 0.25,
                end: 0.75,
            },
            playback: Playback::PingPong,
            ..motion()
        };
        let head = Playhead::launch(beats, beat(0.0, 0.0));
        let now = beat(0.3, 0.6);
        let course = head.course(now, LENGTH, 120.0);
        for ms in 0..4000 {
            let time = Duration::from_millis(300 + ms);
            // At a steady 120 bpm the beats are twice the seconds.
            let tick = beat(time.as_secs_f64(), time.as_secs_f64() * 2.0);
            near(course.at(time), head.at(tick, LENGTH).unwrap());
        }
        let reverse = Playhead::launch(
            Motion {
                direction: Direction::Reverse,
                ..beats
            },
            beat(0.0, 0.0),
        );
        let course = reverse.course(now, LENGTH, 120.0);
        assert!(course.rate < 0.0);
        let paused = Playhead::launch(
            Motion {
                direction: Direction::Pause,
                ..beats
            },
            beat(0.0, 0.0),
        );
        assert_eq!(paused.course(now, LENGTH, 120.0).rate, 0.0);
    }
}
