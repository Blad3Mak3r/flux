//! The sound of the VJ grid: the clips launched by hand play the sound of their video, each at
//! the level of its layer's fader. The grid has a clock of its own, so this is a pure function of
//! what is playing and where its playhead is, like the picture. The app builds a [`GridMix`] and
//! hands it to the output, whose audio callback mixes it in, whether the transport plays or not.
//!
//! A voice follows the clip's playhead as a [`Course`]: where the playhead was on the grid's clock
//! and how fast it moves, which the callback reads at a fractional frame with linear
//! interpolation. The sound is varispeed: its pitch follows the speed, it plays backwards in
//! reverse and is silent while paused. The callback cannot count beats, so a clip that lasts a
//! number of beats is turned into its speed when the mix is built, and the mix is built again when
//! the tempo changes. When a new mix moves a voice to somewhere else (a scrub, a cue, a clip that
//! ends or starts moving), the old voice fades out and the new one fades in over one chunk, about
//! 5 ms, so the jump does not click. A loop wrapping around is not smoothed: it is as continuous
//! as the video's own loop is.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flux_audio::{Decoded, ENGINE_RATE};
use flux_project::playhead::Course;

use crate::mixer::MIX_CHUNK;

const SCALE: f32 = 1.0 / 32_768.0;

/// How far the callback's own count of the grid clock may drift from the wall clock before it
/// starts over from it.
const RESYNC: Duration = Duration::from_millis(30);

/// A fade of a voice: it follows the transition of its layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    /// When the fade started or last changed speed, and how far it was then, 0 to 1.
    pub since: Duration,
    pub done: f32,
    /// How long the whole fade takes.
    pub over: Duration,
    /// Fading in (0 to 1) or out (1 to 0).
    pub rising: bool,
}

impl Ramp {
    /// The gain of the fade at `now` on the grid's clock.
    pub fn at(&self, now: Duration) -> f32 {
        let progress = if self.over.is_zero() {
            1.0
        } else {
            let moved = now.saturating_sub(self.since).as_secs_f32() / self.over.as_secs_f32();
            (self.done + moved).min(1.0)
        };
        if self.rising {
            progress
        } else {
            1.0 - progress
        }
    }
}

/// The sound of one clip playing on the grid.
#[derive(Debug, Clone)]
pub struct GridVoice {
    /// Tells the voice apart across mixes (its layer, clip and launch), so a change of level can be
    /// smoothed from where it was.
    pub key: u64,
    pub audio: Arc<Decoded>,
    /// Where the clip is in its video, and how it moves, on the grid's clock.
    pub course: Course,
    /// How long the clip's video lasts: sound and picture fold together.
    pub length: Duration,
    /// Linear gain: the layer's volume, master and crossfader side.
    pub gain: f32,
    pub ramp: Option<Ramp>,
}

/// How far apart two courses are, in frames, when the voice is taken to have jumped.
const JUMP: f64 = ENGINE_RATE as f64 * 0.01;

impl GridVoice {
    /// Which frame of the sound plays `seconds` after the course's anchor, fractional; `None` when
    /// it is silent: the clip is paused or played once and has ended, or the sound is shorter than
    /// the video.
    pub fn frame_at(&self, seconds: f64) -> Option<f64> {
        if self.course.rate == 0.0 {
            return None;
        }
        let place = self.course.at_seconds(seconds)?;
        let frame = place * self.length.as_secs_f64() * f64::from(ENGINE_RATE);
        (frame >= 0.0 && frame < self.audio.frames() as f64).then_some(frame)
    }

    /// Whether `other` is not where this voice would be at `now`, nor moving the same.
    fn jumps_to(&self, other: &Self, now: Duration) -> bool {
        let at = |voice: &Self| voice.frame_at(now.as_secs_f64() - voice.course.time.as_secs_f64());
        match (at(self), at(other)) {
            (Some(a), Some(b)) => (a - b).abs() > JUMP,
            (None, None) => false,
            _ => true,
        }
    }

    /// Adds the sound for the frames from the grid time `now` on to `left` and `right`, with the
    /// voice's gain moving linearly from `previous` to `target` across them.
    fn add(
        &self,
        now: Duration,
        (previous, target): (f32, f32),
        left: &mut [f32],
        right: &mut [f32],
    ) {
        let frames = left.len().min(right.len());
        let end = now + Duration::from_secs_f64(frames as f64 / f64::from(ENGINE_RATE));
        let fade = |at| self.ramp.map_or(1.0, |ramp| ramp.at(at));
        let (from, to) = (previous * fade(now), target * fade(end));
        if (from <= 0.0 && to <= 0.0) || self.course.rate == 0.0 {
            return;
        }
        let base = now.as_secs_f64() - self.course.time.as_secs_f64();
        let samples = &self.audio.samples;
        for out in 0..frames {
            let seconds = base + out as f64 / f64::from(ENGINE_RATE);
            let Some(frame) = self.frame_at(seconds) else {
                continue;
            };
            // Linear interpolation between this frame and the next, which the last one lacks.
            let (first, mix) = (frame as usize, (frame - frame.floor()) as f32);
            let next = (first + 1).min(self.audio.frames() - 1);
            let sample = |frame: usize, side: usize| f32::from(samples[frame * 2 + side]);
            let gain = SCALE * (from + (to - from) * (out as f32 / frames as f32));
            left[out] += (sample(first, 0) + (sample(next, 0) - sample(first, 0)) * mix) * gain;
            right[out] += (sample(first, 1) + (sample(next, 1) - sample(first, 1)) * mix) * gain;
        }
    }
}

/// Everything the grid sounds, and the instant its clock started.
#[derive(Debug, Clone)]
pub struct GridMix {
    pub origin: Instant,
    pub voices: Vec<GridVoice>,
}

impl GridMix {
    pub fn silent() -> Self {
        Self {
            origin: Instant::now(),
            voices: Vec::new(),
        }
    }

    /// Adds the voices' sound for the frames from the grid time `now` on to `left` and `right`,
    /// which have the same length of at most `MIX_CHUNK`.
    /// `applied` holds the gain each voice ended the last chunk on, by index; it is updated.
    fn mix(&self, now: Duration, applied: &mut [(u64, f32)], left: &mut [f32], right: &mut [f32]) {
        for (index, voice) in self.voices.iter().enumerate() {
            let previous = applied.get_mut(index).map(|slot| {
                let before = slot.1;
                slot.1 = voice.gain;
                before
            });
            let from = previous.unwrap_or(voice.gain);
            voice.add(now, (from, voice.gain), left, right);
        }
    }
}

/// Where the control side leaves the grid's sound for the audio callback.
#[derive(Debug)]
pub struct GridAudio {
    generation: AtomicU64,
    slot: Mutex<Arc<GridMix>>,
    /// The mixes the callback may still hold: it must never be the one to free one.
    retired: Mutex<Vec<Arc<GridMix>>>,
}

impl Default for GridAudio {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            slot: Mutex::new(Arc::new(GridMix::silent())),
            retired: Mutex::default(),
        }
    }
}

impl GridAudio {
    pub fn set(&self, mix: GridMix) {
        let old = std::mem::replace(
            &mut *self.slot.lock().unwrap_or_else(PoisonError::into_inner),
            Arc::new(mix),
        );
        let mut retired = self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        retired.retain(|kept| Arc::strong_count(kept) > 1);
        retired.push(old);
        self.generation.fetch_add(1, Ordering::Release);
    }
}

/// The callback's side: the mix it plays and its count of the grid's clock.
#[derive(Debug)]
pub struct GridPlayer {
    seen: u64,
    mix: Arc<GridMix>,
    clock: Option<Duration>,
    /// The key and last gain of each voice of `mix`, by index; voices past the end are not smoothed.
    applied: [(u64, f32); MAX_SMOOTHED],
    /// Voices that were taken somewhere else by the last mix, fading out where they were.
    tails: [Option<Tail>; MAX_TAILS],
}

/// A voice of an earlier mix, fading out over the next chunk.
#[derive(Debug)]
struct Tail {
    mix: Arc<GridMix>,
    index: usize,
    /// Its gain when it was left.
    gain: f32,
}

/// How many voices can fade out at once after a jump.
const MAX_TAILS: usize = 16;

/// How many voices have their level smoothed.
const MAX_SMOOTHED: usize = 64;

impl Default for GridPlayer {
    fn default() -> Self {
        Self {
            seen: 0,
            mix: Arc::new(GridMix::silent()),
            clock: None,
            applied: [(0, 0.0); MAX_SMOOTHED],
            tails: std::array::from_fn(|_| None),
        }
    }
}

impl GridPlayer {
    /// Takes the newest mix, if the control side left one and is not writing it right now, and
    /// keeps the clock in step with the wall's. Call once per buffer.
    pub fn begin(&mut self, shared: &GridAudio) {
        let generation = shared.generation.load(Ordering::Acquire);
        if generation != self.seen
            && let Ok(slot) = shared.slot.try_lock()
        {
            let mut applied = [(0, 0.0); MAX_SMOOTHED];
            let before = &self.applied[..self.mix.voices.len().min(MAX_SMOOTHED)];
            for (index, voice) in slot.voices.iter().enumerate().take(MAX_SMOOTHED) {
                let carried = before.iter().position(|(key, _)| *key == voice.key);
                let mut gain = voice.gain;
                if let Some(old) = carried {
                    gain = before[old].1;
                    let jumped = self
                        .clock
                        .is_some_and(|now| self.mix.voices[old].jumps_to(voice, now));
                    // It fades out where it was and in where it is, instead of cutting.
                    if jumped && let Some(free) = self.tails.iter_mut().find(|tail| tail.is_none())
                    {
                        *free = Some(Tail {
                            mix: Arc::clone(&self.mix),
                            index: old,
                            gain,
                        });
                        gain = 0.0;
                    }
                }
                applied[index] = (voice.key, gain);
            }
            self.applied = applied;
            self.mix = Arc::clone(&slot);
            self.seen = generation;
        }
        if self.mix.voices.is_empty() {
            self.clock = None;
            return;
        }
        let wall = self.mix.origin.elapsed();
        match self.clock {
            Some(clock) if clock.abs_diff(wall) < RESYNC => {}
            _ => self.clock = Some(wall),
        }
    }

    /// Adds one chunk of the grid's sound to `left` and `right` and moves the clock on.
    pub fn chunk(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Some(now) = self.clock else {
            return;
        };
        let frames = left.len();
        for slot in &mut self.tails {
            if let Some(tail) = slot.take() {
                let voice = &tail.mix.voices[tail.index];
                voice.add(now, (tail.gain, 0.0), left, right);
            }
        }
        if frames <= MIX_CHUNK && right.len() == frames {
            // The grid's own sound, apart from what it is added to, is what the analysis hears.
            let mut own_left = [0.0f32; MIX_CHUNK];
            let mut own_right = [0.0f32; MIX_CHUNK];
            self.mix.mix(
                now,
                &mut self.applied,
                &mut own_left[..frames],
                &mut own_right[..frames],
            );
            crate::analysis::tap_grid(&own_left[..frames], &own_right[..frames], ENGINE_RATE);
            for at in 0..frames {
                left[at] += own_left[at];
                right[at] += own_right[at];
            }
        } else {
            self.mix.mix(now, &mut self.applied, left, right);
        }
        self.clock =
            Some(now + Duration::from_secs_f64(left.len() as f64 / f64::from(ENGINE_RATE)));
    }

    pub fn is_silent(&self) -> bool {
        self.clock.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::playhead::{Motion, Playhead, Tick};
    use flux_project::vj::{Direction, Playback, Region, Tempo};

    fn motion(playback: Playback) -> Motion {
        Motion {
            speed: 1.0,
            direction: Direction::Forward,
            tempo: Tempo::Free,
            region: Region::default(),
            playback,
            generator: false,
        }
    }

    fn seconds(frames: u64) -> f64 {
        frames as f64 / f64::from(ENGINE_RATE)
    }

    /// A voice of a video of `length_frames` frames of sound, launched at the origin; the sound is
    /// a ramp: frame i is i * 100.
    fn voice_of(frames: usize, motion: Motion, length_frames: u64) -> GridVoice {
        let samples = (0..frames)
            .flat_map(|i| [((i % 300) as i16) * 100; 2])
            .collect();
        let length = seconds(length_frames);
        let head = Playhead::launch(motion, Tick::default());
        GridVoice {
            key: 0,
            audio: Arc::new(Decoded {
                samples,
                source_rate: ENGINE_RATE,
            }),
            course: head.course(Tick::default(), length, 120.0),
            length: Duration::from_secs_f64(length),
            gain: 1.0,
            ramp: None,
        }
    }

    fn voice(frames: usize, playback: Playback, length_frames: u64) -> GridVoice {
        voice_of(frames, motion(playback), length_frames)
    }

    /// The frame the voice plays `elapsed` frames after the launch, rounded.
    fn frame(voice: &GridVoice, elapsed: u64) -> Option<u64> {
        voice.frame_at(seconds(elapsed)).map(|f| f.round() as u64)
    }

    #[test]
    fn loop_wraps_at_the_video_length() {
        let v = voice(10, Playback::Loop, 4);
        assert_eq!(frame(&v, 3), Some(3));
        assert_eq!(frame(&v, 4), Some(0));
        assert_eq!(frame(&v, 9), Some(1));
    }

    #[test]
    fn once_ends() {
        let v = voice(10, Playback::Once, 4);
        assert_eq!(frame(&v, 3), Some(3));
        assert_eq!(frame(&v, 4), None);
    }

    #[test]
    fn ping_pong_plays_back() {
        let v = voice(10, Playback::PingPong, 4);
        let got: Vec<_> = (0..9).map(|e| frame(&v, e)).collect();
        assert_eq!(got, [0, 1, 2, 3, 4, 3, 2, 1, 0].map(Some).to_vec());
    }

    #[test]
    fn sound_shorter_than_the_video_is_silent_after_it() {
        let v = voice(3, Playback::Loop, 6);
        assert_eq!(frame(&v, 2), Some(2));
        assert_eq!(frame(&v, 3), None);
    }

    #[test]
    fn speed_should_move_the_sound_at_that_rate_and_reverse_play_it_backwards() {
        let fast = voice_of(
            100,
            Motion {
                speed: 2.0,
                ..motion(Playback::Loop)
            },
            40,
        );
        assert_eq!(frame(&fast, 5), Some(10));
        let slow = voice_of(
            100,
            Motion {
                speed: 0.5,
                ..motion(Playback::Loop)
            },
            40,
        );
        assert_eq!(frame(&slow, 10), Some(5));
        let reverse = voice_of(
            100,
            Motion {
                direction: Direction::Reverse,
                ..motion(Playback::Loop)
            },
            40,
        );
        assert_eq!(frame(&reverse, 0), Some(40));
        assert_eq!(frame(&reverse, 10), Some(30));
        let got = reverse.frame_at(seconds(10)).unwrap();
        assert!((got - 30.0).abs() < 1e-3);
    }

    #[test]
    fn the_sound_should_be_interpolated_between_frames_and_silent_while_paused() {
        let slow = voice_of(
            100,
            Motion {
                speed: 0.5,
                ..motion(Playback::Loop)
            },
            100,
        );
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        // From 10 frames in, at half speed: 5, 5.5, 6, 6.5 of the sound.
        slow.add(
            Duration::from_secs_f64(seconds(10)),
            (1.0, 1.0),
            &mut l,
            &mut r,
        );
        for (i, l) in l.iter().enumerate() {
            let expected = (5.0 + i as f32 * 0.5) * 100.0 * SCALE;
            assert!((l - expected).abs() < 1e-6, "{l} is not {expected}");
        }
        let paused = voice_of(
            100,
            Motion {
                direction: Direction::Pause,
                ..motion(Playback::Loop)
            },
            100,
        );
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        paused.add(Duration::ZERO, (1.0, 1.0), &mut l, &mut r);
        assert_eq!(l, [0.0; 4]);
        assert_eq!(paused.frame_at(1.0), None);
    }

    #[test]
    fn a_region_should_fold_the_sound_as_it_folds_the_picture() {
        for playback in [Playback::Loop, Playback::Once, Playback::PingPong] {
            for direction in [Direction::Forward, Direction::Reverse] {
                let m = Motion {
                    region: Region {
                        start: 0.25,
                        end: 0.75,
                    },
                    speed: 1.5,
                    direction,
                    ..motion(playback)
                };
                let (frames, length) = (4_800u64, 4_800u64);
                let v = voice_of(frames as usize, m, length);
                let head = Playhead::launch(m, Tick::default());
                for step in 0..400 {
                    let elapsed = step * 40 + 7;
                    let tick: Tick = Duration::from_secs_f64(seconds(elapsed)).into();
                    let picture = head.at(tick, seconds(length));
                    let sound = v.frame_at(seconds(elapsed));
                    match (picture, sound) {
                        (Some(x), Some(frame)) => {
                            assert!(
                                (x * frames as f64 - frame).abs() < 1e-3,
                                "{playback:?} {direction:?} at {elapsed}: {} {frame}",
                                x * frames as f64
                            );
                        }
                        (None, None) => {}
                        both => panic!("{playback:?} {direction:?} at {elapsed}: {both:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn a_pass_of_beats_should_play_at_the_speed_the_tempo_makes_and_go_on_when_it_changes() {
        let m = Motion {
            tempo: Tempo::Beats { beats: 2 },
            ..motion(Playback::Loop)
        };
        let length = seconds(48_000);
        let head = Playhead::launch(m, Tick::default());
        // 2 beats at 120 bpm last 1 s: half the video in half a second.
        let course = head.course(Tick::default(), length, 120.0);
        assert!((course.rate - 1.0).abs() < 1e-9);
        // The tempo halves after 0.5 s (1 beat counted): the course is built again there.
        let tick = Tick {
            time: Duration::from_millis(500),
            beats: 1.0,
        };
        let slower = head.course(tick, length, 60.0);
        assert!((slower.rate - 0.5).abs() < 1e-9);
        assert_eq!(slower.at(tick.time), course.at(tick.time));
    }

    #[test]
    fn ramp_follows_the_fade() {
        let up = Ramp {
            since: Duration::from_secs(1),
            done: 0.0,
            over: Duration::from_secs(2),
            rising: true,
        };
        assert_eq!(up.at(Duration::ZERO), 0.0);
        assert_eq!(up.at(Duration::from_secs(2)), 0.5);
        assert_eq!(up.at(Duration::from_secs(9)), 1.0);
        let down = Ramp {
            rising: false,
            ..up
        };
        assert_eq!(down.at(Duration::from_secs(2)), 0.5);
        assert_eq!(down.at(Duration::from_secs(9)), 0.0);
    }

    #[test]
    fn mixes_at_the_gain_of_the_voice() {
        let mut v = voice(10, Playback::Loop, 10);
        v.gain = 0.5;
        let (mut l, mut r) = ([1.0f32; 4], [0.0f32; 4]);
        let now = Duration::from_secs_f64(seconds(2));
        v.add(now, (v.gain, v.gain), &mut l, &mut r);
        // Frames 2..6 of the sound, at half.
        for (i, (l, r)) in l.iter().zip(&r).enumerate() {
            let sound = (2 + i) as f32 * 100.0 * SCALE * 0.5;
            assert!((l - (1.0 + sound)).abs() < 1e-6);
            assert!((r - sound).abs() < 1e-6);
        }
    }

    #[test]
    fn a_fading_voice_gets_quieter_and_a_silent_one_adds_nothing() {
        let mut v = voice(10, Playback::Loop, 10);
        v.ramp = Some(Ramp {
            since: Duration::ZERO,
            done: 0.0,
            over: Duration::from_secs(1),
            rising: false,
        });
        let (mut l, mut r) = ([0.0f32; 4], [0.0f32; 4]);
        v.add(Duration::from_secs(2), (v.gain, v.gain), &mut l, &mut r);
        assert_eq!(l, [0.0; 4]);
        v.add(Duration::ZERO, (v.gain, v.gain), &mut l, &mut r);
        assert_eq!(l[0], 0.0, "frame 0 is silent anyway");
        assert!(l[3] > 0.0);
    }

    #[test]
    fn the_player_takes_what_is_set_and_moves_its_clock() {
        let shared = GridAudio::default();
        let mut player = GridPlayer::default();
        player.begin(&shared);
        assert!(player.is_silent());
        shared.set(GridMix {
            origin: Instant::now(),
            voices: vec![voice(1000, Playback::Loop, 1000)],
        });
        player.begin(&shared);
        assert!(!player.is_silent());
        let before = player.clock.unwrap();
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        player.chunk(&mut l, &mut r);
        assert!(player.clock.unwrap() > before);
    }

    #[test]
    fn a_change_of_level_ramps_from_where_the_voice_was() {
        let shared = GridAudio::default();
        let mut player = GridPlayer::default();
        let mut quiet = voice(1000, Playback::Loop, 1000);
        quiet.key = 7;
        quiet.gain = 0.0;
        let mix = |voice: GridVoice| GridMix {
            origin: Instant::now(),
            voices: vec![voice],
        };
        shared.set(mix(quiet.clone()));
        player.begin(&shared);
        assert_eq!(player.applied[0], (7, 0.0));
        let mut loud = quiet;
        loud.gain = 1.0;
        shared.set(mix(loud));
        player.begin(&shared);
        assert_eq!(player.applied[0].1, 0.0, "carried over, not stepped");
        let (mut l, mut r) = ([0.0f32; 64], [0.0f32; 64]);
        player.chunk(&mut l, &mut r);
        assert_eq!(l[0], 0.0);
        assert!(l[63] > l[10] && l[10] > 0.0, "rises across the buffer");
        assert_eq!(player.applied[0].1, 1.0);
    }

    #[test]
    fn a_jump_should_fade_the_old_place_out_and_the_new_one_in_but_a_steady_voice_should_not() {
        let shared = GridAudio::default();
        let mut player = GridPlayer::default();
        let mix = |voice: GridVoice| GridMix {
            origin: Instant::now(),
            voices: vec![voice],
        };
        let mut steady = voice(48_000, Playback::Loop, 48_000);
        steady.key = 3;
        shared.set(mix(steady.clone()));
        player.begin(&shared);
        // The same course pushed again is no jump.
        shared.set(mix(steady.clone()));
        player.begin(&shared);
        assert!(player.tails.iter().all(Option::is_none));
        assert_eq!(player.applied[0].1, 1.0);
        // A cue: the course is anchored somewhere else.
        let mut jumped = steady;
        let mut head = Playhead::launch(motion(Playback::Loop), Tick::default());
        head.jump(Tick::default(), 0.5);
        jumped.course = head.course(Tick::default(), 1.0, 120.0);
        shared.set(mix(jumped));
        player.begin(&shared);
        assert!(
            player.tails.iter().any(Option::is_some),
            "old place fades out"
        );
        assert_eq!(player.applied[0].1, 0.0, "new place fades in");
        let (mut l, mut r) = ([0.0f32; 256], [0.0f32; 256]);
        player.chunk(&mut l, &mut r);
        assert!(player.tails.iter().all(Option::is_none), "one chunk");
        assert_eq!(player.applied[0].1, 1.0);
    }
}
