//! The programme: the project's audio clips mixed into a stereo pair, sample by sample on the
//! transport's clock. Pure and allocation-free once built, so the audio callback can call it.

use std::sync::Arc;

use super::transport::Clock;
use crate::audio::{AudioError, Decoded, ENGINE_RATE};
use crate::project::{AudioAsset, Project, TrackKind};

/// One clip placed on the timeline.
#[derive(Debug, Clone)]
pub struct ClipPlayer {
    pub audio: Arc<Decoded>,
    /// Sample of the timeline, counted from its origin, where the clip begins.
    pub start: u64,
    /// Linear gain.
    pub gain: f32,
}

/// Everything that sounds: the clips of the tracks that are not muted.
#[derive(Debug, Default, Clone)]
pub struct ClipSet {
    clips: Vec<ClipPlayer>,
}

/// A clip set plus what could not be included in it.
#[derive(Debug, Default)]
pub struct Programme {
    pub set: ClipSet,
    /// One line per audio file that could not be used, for the user.
    pub warnings: Vec<String>,
}

impl ClipSet {
    pub fn new(clips: Vec<ClipPlayer>) -> Self {
        Self { clips }
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Adds the clips' audio for the timeline samples `position..position + left.len()` to `left`
    /// and `right`, which must have the same length.
    pub fn mix(&self, position: u64, left: &mut [f32], right: &mut [f32]) {
        debug_assert_eq!(left.len(), right.len());
        let window_end = position + left.len() as u64;
        for clip in &self.clips {
            let clip_end = clip.start + clip.audio.frames() as u64;
            let from = position.max(clip.start);
            let to = window_end.min(clip_end);
            for at in from..to {
                let source = ((at - clip.start) * 2) as usize;
                let out = (at - position) as usize;
                left[out] += f32::from(clip.audio.samples[source]) * SCALE * clip.gain;
                right[out] += f32::from(clip.audio.samples[source + 1]) * SCALE * clip.gain;
            }
        }
    }
}

/// 16-bit samples to the -1.0 to 1.0 range.
const SCALE: f32 = 1.0 / 32_768.0;

fn gain_from_db(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Builds the clip set of a project. `load` supplies the decoded audio of an asset; an asset it
/// cannot supply is skipped with a warning, because a missing file must never stop the show.
pub fn build(
    project: &Project,
    mut load: impl FnMut(&AudioAsset) -> Result<Arc<Decoded>, AudioError>,
) -> Programme {
    let clock = Clock::new(ENGINE_RATE, project.timecode.frame_rate);
    let mut programme = Programme::default();
    let mut failed: Vec<&str> = Vec::new();

    for track in project.tracks.iter().filter(|track| !track.muted) {
        let TrackKind::Audio { clips } = &track.kind else {
            continue;
        };
        for clip in clips {
            let Some(asset) = project.assets.iter().find(|asset| asset.id == clip.asset) else {
                continue;
            };
            if failed.contains(&asset.id.as_str()) {
                continue;
            }
            match load(asset) {
                Ok(audio) => programme.set.clips.push(ClipPlayer {
                    audio,
                    start: clock.frames_to_samples(clip.start),
                    gain: gain_from_db(clip.gain_db),
                }),
                Err(err) => {
                    failed.push(&asset.id);
                    programme
                        .warnings
                        .push(format!("{} is silent: {err}", asset.name));
                }
            }
        }
    }
    programme
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

    fn set(clips: Vec<ClipPlayer>) -> ClipSet {
        ClipSet { clips }
    }

    fn player(audio: Arc<Decoded>, start: u64, gain: f32) -> ClipPlayer {
        ClipPlayer { audio, start, gain }
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
        project.tracks = tracks;
        project.assets = assets;
        project
    }

    fn asset(id: &str) -> AudioAsset {
        AudioAsset {
            id: id.into(),
            name: id.into(),
            file: format!("audio/{id}.wav"),
        }
    }

    fn audio_track(id: &str, muted: bool, clips: Vec<AudioClip>) -> Track {
        Track {
            id: id.into(),
            name: id.into(),
            muted,
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

        let programme = build(&project, |_| Ok(audio(1, 1, 10)));

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

        let programme = build(&project, |_| Ok(audio(1, 1, 10)));

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

        let programme = build(&project, |_| Err(AudioError::NoAudioTrack));

        assert!(programme.set.is_empty());
        assert_eq!(programme.warnings.len(), 1);
        assert!(programme.warnings[0].starts_with("gone is silent"));
    }
}
