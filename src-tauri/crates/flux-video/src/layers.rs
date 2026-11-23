use flux_core::FrameRate;
use flux_project::{Timeline, TrackKind};

use crate::mapping::{Rate, source_frame};

/// What the transport asks the video to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Stopped: the outputs show black.
    Black,
    /// Playing or paused at this frame, counted from the timeline origin.
    At(u64),
}

/// One picture to draw: a frame of a video, for one track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub track: String,
    pub clip: String,
    /// Id of an entry in `Project::videos`.
    pub asset: String,
    pub frame: u64,
    /// How the clip is sized into the composition.
    pub fit: flux_project::VideoFit,
}

/// The rate and length of a video, which the mapping needs; `None` when it could not be read.
pub type Probe<'a> = &'a dyn Fn(&str) -> Option<(Rate, u64)>;

/// What the video tracks of `timeline` show for `cue`, the topmost first: tracks higher in the
/// timeline cover the ones below. A muted track shows nothing. Where clips of one track overlap,
/// the one that starts later is on top, as when a clip is dropped over another.
pub fn layers(timeline: &Timeline, cue: Cue, probe: Probe<'_>) -> Vec<Layer> {
    let Cue::At(position) = cue else {
        return Vec::new();
    };
    let rate: FrameRate = timeline.timecode.frame_rate;
    timeline
        .tracks
        .iter()
        .filter(|track| !track.muted)
        .filter_map(|track| {
            let TrackKind::Video { clips } = &track.kind else {
                return None;
            };
            clips
                .iter()
                .filter_map(|clip| {
                    let (video, frames) = probe(&clip.asset)?;
                    let frame = source_frame(clip, position, rate, video, frames)?;
                    Some((clip, frame))
                })
                .max_by_key(|(clip, _)| clip.start)
                .map(|(clip, frame)| Layer {
                    track: track.id.clone(),
                    clip: clip.id.clone(),
                    asset: clip.asset.clone(),
                    frame,
                    fit: clip.fit,
                })
        })
        .collect()
}

/// The clips of `timeline` that start within `ahead` frames after `position`, with the first frame
/// each will show: opening their files early means that frame is ready when they start.
pub fn upcoming(timeline: &Timeline, position: u64, ahead: u64, probe: Probe<'_>) -> Vec<Layer> {
    let rate = timeline.timecode.frame_rate;
    timeline
        .tracks
        .iter()
        .filter(|track| !track.muted)
        .flat_map(|track| match &track.kind {
            TrackKind::Video { clips } => clips.iter().map(|clip| (track, clip)).collect(),
            _ => Vec::new(),
        })
        .filter(|(_, clip)| clip.start > position && clip.start - position <= ahead)
        .filter_map(|(track, clip)| {
            let (video, frames) = probe(&clip.asset)?;
            Some(Layer {
                track: track.id.clone(),
                clip: clip.id.clone(),
                asset: clip.asset.clone(),
                frame: source_frame(clip, clip.start, rate, video, frames)?,
                fit: clip.fit,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use flux_project::{Track, VideoClip};

    use super::*;

    fn clip(id: &str, start: u64, length: Option<u64>) -> VideoClip {
        VideoClip {
            id: id.into(),
            asset: "v".into(),
            start,
            name: None,
            offset: 0,
            length,
            fit: Default::default(),
        }
    }

    fn track(id: &str, muted: bool, clips: Vec<VideoClip>) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Video { clips },
        }
    }

    fn timeline(tracks: Vec<Track>) -> Timeline {
        let mut timeline = Timeline::new("Show", FrameRate::Fps25);
        timeline.tracks = tracks;
        timeline
    }

    /// Every video is a 25 fps file of 100 frames.
    fn probe(_: &str) -> Option<(Rate, u64)> {
        Some((Rate::new(25, 1), 100))
    }

    fn ids(layers: &[Layer]) -> Vec<(&str, u64)> {
        layers.iter().map(|l| (l.clip.as_str(), l.frame)).collect()
    }

    #[test]
    fn stopped_should_show_black() {
        let timeline = timeline(vec![track("t", false, vec![clip("a", 0, None)])]);
        assert!(layers(&timeline, Cue::Black, &probe).is_empty());
    }

    #[test]
    fn higher_tracks_should_cover_lower_ones_and_muted_tracks_show_nothing() {
        let timeline = timeline(vec![
            track("top", false, vec![clip("a", 10, None)]),
            track("muted", true, vec![clip("b", 0, None)]),
            track("bottom", false, vec![clip("c", 0, None)]),
        ]);
        assert_eq!(ids(&layers(&timeline, Cue::At(5), &probe)), [("c", 5)]);
        assert_eq!(
            ids(&layers(&timeline, Cue::At(15), &probe)),
            [("a", 5), ("c", 15)]
        );
    }

    #[test]
    fn the_later_clip_should_win_where_clips_of_a_track_overlap() {
        let timeline = timeline(vec![track(
            "t",
            false,
            vec![clip("late", 20, None), clip("early", 0, None)],
        )]);
        assert_eq!(
            ids(&layers(&timeline, Cue::At(19), &probe)),
            [("early", 19)]
        );
        assert_eq!(ids(&layers(&timeline, Cue::At(20), &probe)), [("late", 0)]);
    }

    #[test]
    fn a_clip_should_disappear_after_its_length_or_the_end_of_its_video() {
        let timeline = timeline(vec![
            track("t1", false, vec![clip("short", 0, Some(10))]),
            track("t2", false, vec![clip("whole", 0, None)]),
        ]);
        assert_eq!(
            ids(&layers(&timeline, Cue::At(10), &probe)),
            [("whole", 10)]
        );
        assert!(layers(&timeline, Cue::At(100), &probe).is_empty());
    }

    #[test]
    fn a_video_that_cannot_be_probed_should_show_nothing() {
        let timeline = timeline(vec![track("t", false, vec![clip("a", 0, None)])]);
        assert!(layers(&timeline, Cue::At(0), &|_| None).is_empty());
    }

    #[test]
    fn upcoming_should_list_clips_starting_within_the_window_with_their_first_frame() {
        let mut later = clip("later", 50, None);
        later.offset = 7;
        let timeline = timeline(vec![track(
            "t",
            false,
            vec![clip("now", 0, None), clip("soon", 30, None), later],
        )]);
        assert_eq!(ids(&upcoming(&timeline, 10, 30, &probe)), [("soon", 0)]);
        assert_eq!(
            ids(&upcoming(&timeline, 20, 30, &probe)),
            [("soon", 0), ("later", 7)]
        );
    }
}
