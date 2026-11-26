//! The sound of the VJ grid: the clips launched by hand play the sound of their video, each at
//! the level of its layer's fader. The grid has a clock of its own, so this is a pure function of
//! what is playing and when it was launched, like the picture. The app builds a [`GridMix`] and
//! hands it to the output, whose audio callback mixes it in, whether the transport plays or not.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flux_audio::{Decoded, ENGINE_RATE};
use flux_project::vj::Playback;

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
    /// When the clip was launched, on the grid's clock.
    pub at: Duration,
    pub playback: Playback,
    /// How long the clip's video lasts: sound and picture wrap together.
    pub length: Duration,
    /// Linear gain: the layer's volume, master and crossfader side.
    pub gain: f32,
    pub ramp: Option<Ramp>,
}

fn frames_of(time: Duration) -> u64 {
    ((time.as_nanos() * u128::from(ENGINE_RATE) + 500_000_000) / 1_000_000_000) as u64
}

impl GridVoice {
    /// Which frame of the sound plays `elapsed` frames after the launch, or `None` when it is
    /// silent: a clip played once has ended, or the sound is shorter than the video.
    pub fn frame_at(&self, elapsed: u64) -> Option<usize> {
        let length = frames_of(self.length);
        if length == 0 {
            return None;
        }
        let at = match self.playback {
            Playback::Once => (elapsed < length).then_some(elapsed)?,
            Playback::Loop => elapsed % length,
            Playback::PingPong => {
                let period = 2 * length;
                let at = elapsed % period;
                if at < length { at } else { period - 1 - at }
            }
        };
        let at = usize::try_from(at).ok()?;
        (at < self.audio.frames()).then_some(at)
    }

    /// Adds the sound for the frames from the grid time `now` on to `left` and `right`, with the
    /// voice's gain moving linearly from `previous` to its own across them.
    fn add(&self, now: Duration, previous: f32, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        let end = now + Duration::from_secs_f64(frames as f64 / f64::from(ENGINE_RATE));
        let fade = |at| self.ramp.map_or(1.0, |ramp| ramp.at(at));
        let (from, to) = (previous * fade(now), self.gain * fade(end));
        if from <= 0.0 && to <= 0.0 {
            return;
        }
        let launched = frames_of(self.at);
        let first = frames_of(now);
        for out in 0..frames {
            let Some(elapsed) = (first + out as u64).checked_sub(launched) else {
                continue;
            };
            let Some(at) = self.frame_at(elapsed) else {
                continue;
            };
            let gain = SCALE * (from + (to - from) * (out as f32 / frames as f32));
            left[out] += f32::from(self.audio.samples[at * 2]) * gain;
            right[out] += f32::from(self.audio.samples[at * 2 + 1]) * gain;
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
            voice.add(now, previous.unwrap_or(voice.gain), left, right);
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
}

/// How many voices have their level smoothed.
const MAX_SMOOTHED: usize = 64;

impl Default for GridPlayer {
    fn default() -> Self {
        Self {
            seen: 0,
            mix: Arc::new(GridMix::silent()),
            clock: None,
            applied: [(0, 0.0); MAX_SMOOTHED],
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
            for (index, voice) in slot.voices.iter().enumerate().take(MAX_SMOOTHED) {
                let carried = self.applied[..self.mix.voices.len().min(MAX_SMOOTHED)]
                    .iter()
                    .find(|(key, _)| *key == voice.key);
                applied[index] = (voice.key, carried.map_or(voice.gain, |(_, gain)| *gain));
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
        if crate::analysis::grid_tap_on() && frames <= MIX_CHUNK && right.len() == frames {
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

    fn voice(frames: usize, playback: Playback, length_frames: u64) -> GridVoice {
        // A ramp: frame i is i * 100.
        let samples = (0..frames)
            .flat_map(|i| [((i % 300) as i16) * 100; 2])
            .collect();
        GridVoice {
            key: 0,
            audio: Arc::new(Decoded {
                samples,
                source_rate: ENGINE_RATE,
            }),
            at: Duration::ZERO,
            playback,
            length: Duration::from_secs_f64(length_frames as f64 / f64::from(ENGINE_RATE)),
            gain: 1.0,
            ramp: None,
        }
    }

    #[test]
    fn loop_wraps_at_the_video_length() {
        let v = voice(10, Playback::Loop, 4);
        assert_eq!(v.frame_at(3), Some(3));
        assert_eq!(v.frame_at(4), Some(0));
        assert_eq!(v.frame_at(9), Some(1));
    }

    #[test]
    fn once_ends() {
        let v = voice(10, Playback::Once, 4);
        assert_eq!(v.frame_at(3), Some(3));
        assert_eq!(v.frame_at(4), None);
    }

    #[test]
    fn ping_pong_plays_back() {
        let v = voice(10, Playback::PingPong, 4);
        let got: Vec<_> = (0..9).map(|e| v.frame_at(e)).collect();
        assert_eq!(got, [0, 1, 2, 3, 3, 2, 1, 0, 0].map(Some).to_vec(),);
    }

    #[test]
    fn sound_shorter_than_the_video_is_silent_after_it() {
        let v = voice(3, Playback::Loop, 6);
        assert_eq!(v.frame_at(2), Some(2));
        assert_eq!(v.frame_at(3), None);
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
        let now = Duration::from_secs_f64(2.0 / f64::from(ENGINE_RATE));
        v.add(now, v.gain, &mut l, &mut r);
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
        v.add(Duration::from_secs(2), v.gain, &mut l, &mut r);
        assert_eq!(l, [0.0; 4]);
        v.add(Duration::ZERO, v.gain, &mut l, &mut r);
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
}
