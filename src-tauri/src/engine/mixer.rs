//! The programme: the project's audio clips mixed into a stereo pair, sample by sample on the
//! transport's clock. Pure and allocation-free once built, so the audio callback can call it.
//!
//! Clips are mixed per track: each track's clips are summed, scaled by the track's fader and added
//! to the programme, and the peak of what the track contributed is kept for the level meters.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;

use super::transport::Clock;
use crate::audio::{AudioError, Decoded, ENGINE_RATE};
use crate::project::{AudioAsset, Project, Timeline, TrackKind};

/// Frames mixed at a time, on the stack: the callback must not allocate.
pub const MIX_CHUNK: usize = 256;

/// A track fader at or below this is silence (the fader's "-inf" end).
pub const SILENT_DB: f32 = -60.0;

/// One clip placed on the timeline.
#[derive(Debug, Clone)]
pub struct ClipPlayer {
    pub audio: Arc<Decoded>,
    /// Sample of the timeline, counted from its origin, where the clip begins.
    pub start: u64,
    /// Linear gain.
    pub gain: f32,
    /// Index into the clip set's tracks.
    pub track: usize,
}

/// An `f32` the audio callback and the other threads share without locks. Only non-negative
/// values go through [`AtomicF32::fetch_max`]: their bit patterns order like the numbers.
#[derive(Debug, Default)]
struct AtomicF32(AtomicU32);

impl AtomicF32 {
    fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn store(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    fn fetch_max(&self, value: f32) {
        self.0.fetch_max(value.to_bits(), Ordering::Relaxed);
    }

    fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

#[derive(Debug)]
struct TrackMeter {
    id: String,
    /// Where the fader is, linear. Written by the control side.
    target: AtomicF32,
    /// The gain the last buffer ended on, so a change is ramped over the next buffer, not stepped.
    applied: AtomicF32,
    /// Largest sample the track contributed since it was last read, after its gain.
    peak: AtomicF32,
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
    tracks: Vec<TrackMeter>,
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

/// Everything that sounds: the clips of the tracks that are not muted.
#[derive(Debug, Default, Clone)]
pub struct ClipSet {
    clips: Vec<ClipPlayer>,
    levels: Arc<TrackLevels>,
    /// Samples at which the show ends; `None` while it has no end (see [`ClipSet::with_end`]).
    end: Option<u64>,
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
            self.mix_chunk(position + (index * MIX_CHUNK) as u64, left, right);
        }
    }

    /// One chunk of at most [`MIX_CHUNK`] frames.
    fn mix_chunk(&self, position: u64, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len();
        let window_end = position + frames as u64;
        for (index, meter) in self.levels.tracks.iter().enumerate() {
            let mut track_left = [0.0f32; MIX_CHUNK];
            let mut track_right = [0.0f32; MIX_CHUNK];
            for clip in self.clips.iter().filter(|clip| clip.track == index) {
                let clip_end = clip.start + clip.audio.frames() as u64;
                let from = position.max(clip.start);
                let to = window_end.min(clip_end);
                for at in from..to {
                    let source = ((at - clip.start) * 2) as usize;
                    let out = (at - position) as usize;
                    track_left[out] += f32::from(clip.audio.samples[source]) * SCALE * clip.gain;
                    track_right[out] +=
                        f32::from(clip.audio.samples[source + 1]) * SCALE * clip.gain;
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

/// Where the engine's current [`TrackLevels`] is, for threads outside the audio callback. The set
/// is swapped when the clips change, so readers ask this each time instead of keeping the old one.
#[derive(Debug, Clone, Default)]
pub struct SharedLevels(Arc<Mutex<Arc<TrackLevels>>>);

impl SharedLevels {
    pub fn new(levels: Arc<TrackLevels>) -> Self {
        Self(Arc::new(Mutex::new(levels)))
    }

    pub fn replace(&self, levels: Arc<TrackLevels>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = levels;
    }

    pub fn current(&self) -> Arc<TrackLevels> {
        Arc::clone(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// 16-bit samples to the -1.0 to 1.0 range.
const SCALE: f32 = 1.0 / 32_768.0;

fn gain_from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A track fader in dB to a linear gain; [`SILENT_DB`] and below is silence.
fn track_gain_from_db(db: f32) -> f32 {
    if db <= SILENT_DB {
        0.0
    } else {
        gain_from_db(db)
    }
}

/// Builds the clip set of a timeline of `project`. `load` supplies the decoded audio of an asset; an asset it
/// cannot supply is skipped with a warning, because a missing file must never stop the show.
pub fn build(
    project: &Project,
    timeline: &Timeline,
    mut load: impl FnMut(&AudioAsset) -> Result<Arc<Decoded>, AudioError>,
) -> Programme {
    let clock = Clock::new(ENGINE_RATE, timeline.timecode.frame_rate);
    let mut clips = Vec::new();
    let mut tracks = Vec::new();
    let mut warnings = Vec::new();
    let mut failed: Vec<&str> = Vec::new();

    for track in timeline.tracks.iter().filter(|track| !track.muted) {
        let TrackKind::Audio { clips: track_clips } = &track.kind else {
            continue;
        };
        let index = tracks.len();
        tracks.push((
            track.id.clone(),
            track.gain_db.map_or(1.0, track_gain_from_db),
        ));
        for clip in track_clips {
            let Some(asset) = project.assets.iter().find(|asset| asset.id == clip.asset) else {
                continue;
            };
            if failed.contains(&asset.id.as_str()) {
                continue;
            }
            match load(asset) {
                Ok(audio) => clips.push(ClipPlayer {
                    audio,
                    start: clock.frames_to_samples(clip.start),
                    gain: gain_from_db(clip.gain_db),
                    track: index,
                }),
                Err(err) => {
                    failed.push(&asset.id);
                    warnings.push(format!("{} is silent: {err}", asset.name));
                }
            }
        }
    }
    Programme {
        set: ClipSet::new(clips, tracks),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AudioClip, FrameRate, Track};

    fn audio(left: i16, right: i16, frames: usize) -> Arc<Decoded> {
        Arc::new(Decoded {
            samples: (0..frames).flat_map(|_| [left, right]).collect(),
            source_rate: ENGINE_RATE,
        })
    }

    /// A clip set with one track at 0 dB that holds every clip.
    fn set(clips: Vec<ClipPlayer>) -> ClipSet {
        ClipSet::new(clips, vec![("t1".into(), 1.0)])
    }

    fn player(audio: Arc<Decoded>, start: u64, gain: f32) -> ClipPlayer {
        ClipPlayer {
            audio,
            start,
            gain,
            track: 0,
        }
    }

    fn mix(set: &ClipSet, position: u64, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
        set.mix(position, &mut left, &mut right);
        (left, right)
    }

    #[test]
    fn mix_should_be_silent_with_no_clips() {
        let (left, right) = mix(&ClipSet::default(), 0, 8);
        assert!(left.iter().chain(&right).all(|&s| s == 0.0));
    }

    #[test]
    fn mix_should_play_a_clip_from_its_start_sample_to_its_end() {
        let clips = set(vec![player(audio(16_384, -16_384, 4), 3, 1.0)]);

        let (left, right) = mix(&clips, 0, 10);

        assert_eq!(left, [0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0]);
        assert_eq!(
            right,
            [0.0, 0.0, 0.0, -0.5, -0.5, -0.5, -0.5, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn mix_should_continue_a_clip_across_buffers() {
        let clips = set(vec![player(audio(8_192, 8_192, 6), 0, 1.0)]);

        let (first, _) = mix(&clips, 0, 4);
        let (second, _) = mix(&clips, 4, 4);

        assert_eq!(first, [0.25; 4]);
        assert_eq!(second, [0.25, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn mix_should_start_in_the_middle_of_a_clip_after_a_seek() {
        let mut samples = vec![0i16; 20];
        samples[10] = 16_384; // frame 5, left
        let clip = Arc::new(Decoded {
            samples,
            source_rate: ENGINE_RATE,
        });
        let clips = set(vec![player(clip, 100, 1.0)]);

        let (left, _) = mix(&clips, 103, 4);

        assert_eq!(left, [0.0, 0.0, 0.5, 0.0]);
    }

    #[test]
    fn mix_should_add_overlapping_clips_and_apply_gain() {
        let clips = set(vec![
            player(audio(16_384, 0, 2), 0, 1.0),
            player(audio(16_384, 0, 2), 0, 0.5),
        ]);

        let (left, _) = mix(&clips, 0, 2);

        assert_eq!(left, [0.75, 0.75]);
    }

    fn project_with(tracks: Vec<Track>, assets: Vec<AudioAsset>) -> Project {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].tracks = tracks;
        project.assets = assets;
        project
    }

    fn asset(id: &str) -> AudioAsset {
        AudioAsset {
            id: id.into(),
            name: id.into(),
            file: format!("audio/{id}.wav"),
            bpm: None,
        }
    }

    fn audio_track(id: &str, muted: bool, clips: Vec<AudioClip>) -> Track {
        Track {
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Audio { clips },
        }
    }

    fn clip(id: &str, asset: &str, start: u64, gain_db: f32) -> AudioClip {
        AudioClip {
            id: id.into(),
            asset: asset.into(),
            start,
            gain_db,
        }
    }

    #[test]
    fn build_should_place_clips_by_frame_and_convert_gain_from_db() {
        let project = project_with(
            vec![audio_track("t1", false, vec![clip("c1", "a1", 25, -6.0)])],
            vec![asset("a1")],
        );

        let programme = build(&project, &project.timelines[0], |_| Ok(audio(1, 1, 10)));

        assert!(programme.warnings.is_empty());
        assert_eq!(programme.set.len(), 1);
        let placed = &programme.set.clips[0];
        // 25 frames at 25 fps is one second.
        assert_eq!(placed.start, 48_000);
        assert!(
            (placed.gain - 0.501_187).abs() < 1e-5,
            "gain {}",
            placed.gain
        );
    }

    #[test]
    fn build_should_leave_out_muted_tracks() {
        let project = project_with(
            vec![
                audio_track("t1", true, vec![clip("c1", "a1", 0, 0.0)]),
                audio_track("t2", false, vec![clip("c2", "a1", 0, 0.0)]),
            ],
            vec![asset("a1")],
        );

        let programme = build(&project, &project.timelines[0], |_| Ok(audio(1, 1, 10)));

        assert_eq!(programme.set.len(), 1);
    }

    #[test]
    fn build_should_skip_an_asset_it_cannot_load_and_say_so_once() {
        let project = project_with(
            vec![audio_track(
                "t1",
                false,
                vec![clip("c1", "gone", 0, 0.0), clip("c2", "gone", 100, 0.0)],
            )],
            vec![asset("gone")],
        );

        let programme = build(&project, &project.timelines[0], |_| {
            Err(AudioError::NoAudioTrack)
        });

        assert!(programme.set.is_empty());
        assert_eq!(programme.warnings.len(), 1);
        assert!(programme.warnings[0].starts_with("gone is silent"));
    }

    fn on_track(mut player: ClipPlayer, track: usize) -> ClipPlayer {
        player.track = track;
        player
    }

    fn two_tracks(first_gain: f32, second_gain: f32) -> ClipSet {
        ClipSet::new(
            vec![
                on_track(player(audio(16_384, 0, 8), 0, 1.0), 0),
                on_track(player(audio(8_192, 0, 8), 0, 1.0), 1),
            ],
            vec![("t1".into(), first_gain), ("t2".into(), second_gain)],
        )
    }

    #[test]
    fn mix_should_scale_each_track_by_its_own_gain() {
        let clips = two_tracks(0.5, 1.0);

        let (left, _) = mix(&clips, 0, 4);

        // 0.5 * 0.5 from the first track, 0.25 from the second.
        assert_eq!(left, [0.5; 4]);
    }

    #[test]
    fn mix_should_multiply_the_track_gain_by_the_clip_gain() {
        let clips = ClipSet::new(
            vec![player(audio(16_384, 0, 4), 0, 0.5)],
            vec![("t1".into(), 0.5)],
        );

        let (left, _) = mix(&clips, 0, 4);

        assert_eq!(left, [0.125; 4]);
    }

    #[test]
    fn mix_should_be_silent_on_a_track_at_zero_gain() {
        let clips = two_tracks(0.0, 1.0);

        let (left, _) = mix(&clips, 0, 4);

        assert_eq!(left, [0.25; 4]);
    }

    #[test]
    fn mix_should_ramp_a_gain_change_instead_of_stepping() {
        let clips = ClipSet::new(
            vec![player(audio(32_767, 0, 512), 0, 1.0)],
            vec![("t1".into(), 1.0)],
        );
        clips.levels().set_gain_db("t1", SILENT_DB);

        let (left, _) = mix(&clips, 0, 256);
        let (next, _) = mix(&clips, 256, 256);

        let slope = 1.0 / 256.0 + 1e-4;
        assert!(left[0] < 1.0, "the first sample already moved");
        assert!(left.windows(2).all(|pair| pair[0] - pair[1] <= slope));
        assert!(left[255].abs() < 1e-4, "the ramp reached silence");
        assert!(next.iter().all(|&s| s == 0.0), "and stayed there");
    }

    fn peaks_of(set: &ClipSet) -> Vec<(String, f32)> {
        set.levels()
            .take_peaks()
            .into_iter()
            .map(|track| (track.id, track.peak))
            .collect()
    }

    #[test]
    fn peaks_should_be_measured_after_the_gain_and_per_track() {
        let clips = two_tracks(0.5, 1.0);
        mix(&clips, 0, 4);
        assert_eq!(
            peaks_of(&clips),
            [("t1".to_owned(), 0.25), ("t2".to_owned(), 0.25)]
        );

        let clips = two_tracks(1.0, 0.5);
        mix(&clips, 0, 4);
        assert_eq!(
            peaks_of(&clips),
            [("t1".to_owned(), 0.5), ("t2".to_owned(), 0.125)]
        );
    }

    #[test]
    fn peak_should_be_the_larger_channel_and_the_largest_sample() {
        let mut samples = vec![0i16; 8];
        samples[2] = 8_192; // frame 1, left
        samples[5] = -24_576; // frame 2, right
        let clip = Arc::new(Decoded {
            samples,
            source_rate: ENGINE_RATE,
        });
        let clips = set(vec![player(clip, 0, 1.0)]);

        mix(&clips, 0, 4);

        assert_eq!(peaks_of(&clips)[0].1, 0.75);
    }

    #[test]
    fn peak_should_accumulate_across_buffers_and_reset_when_read() {
        let clips = set(vec![player(audio(16_384, 0, 8), 4, 1.0)]);

        mix(&clips, 0, 4);
        assert_eq!(clips.levels().take_peaks()[0].peak, 0.0);

        mix(&clips, 4, 4);
        mix(&clips, 8, 4);
        let levels = clips.levels();
        assert_eq!(levels.take_peaks()[0].peak, 0.5);
        assert_eq!(levels.take_peaks()[0].peak, 0.0);
    }

    #[test]
    fn set_gain_db_should_ignore_unknown_tracks_and_non_finite_values() {
        let levels = TrackLevels::new([("t1".to_owned(), 1.0)]);

        assert!(!levels.set_gain_db("nope", -6.0));
        assert!(levels.set_gain_db("t1", f32::NAN));
        assert_eq!(levels.tracks[0].target.load(), 1.0);
        assert!(levels.set_gain_db("t1", -6.0));
        assert!((levels.tracks[0].target.load() - 0.501_187).abs() < 1e-5);
    }

    #[test]
    fn track_gain_from_db_should_be_silent_at_the_bottom_of_the_fader() {
        assert_eq!(track_gain_from_db(SILENT_DB), 0.0);
        assert_eq!(track_gain_from_db(-90.0), 0.0);
        assert_eq!(track_gain_from_db(0.0), 1.0);
        assert!((track_gain_from_db(6.0) - 1.995_262).abs() < 1e-5);
    }

    #[test]
    fn build_should_seed_each_track_with_its_gain_and_skip_muted_ones() {
        let mut quiet = audio_track("t1", false, vec![clip("c1", "a1", 0, 0.0)]);
        quiet.gain_db = Some(-6.0);
        let mut muted = audio_track("t2", true, vec![clip("c2", "a1", 0, 0.0)]);
        muted.gain_db = Some(0.0);
        let project = project_with(vec![quiet, muted], vec![asset("a1")]);

        let programme = build(&project, &project.timelines[0], |_| Ok(audio(16_384, 0, 4)));

        let (left, _) = mix(&programme.set, 0, 4);
        assert!((left[0] - 0.5 * 0.501_187).abs() < 1e-5, "{}", left[0]);
        let peaks = programme.set.levels().take_peaks();
        assert_eq!(peaks.len(), 1, "the muted track has no meter");
        assert_eq!(peaks[0].id, "t1");
    }
}
