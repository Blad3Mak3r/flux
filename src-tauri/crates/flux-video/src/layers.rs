use flux_core::FrameRate;
use flux_project::{Timeline, TrackKind};

use crate::mapping::{Rate, source_frame, timeline_frames};

/// What the transport asks the video to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Stopped: the outputs show black.
    Black,
    /// Playing or paused at this frame, counted from the timeline origin.
    At(u64),
}

/// One picture to draw: a frame of a video, for one track.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub track: String,
    pub clip: String,
    /// Id of an entry in `Project::videos`; empty for a generator.
    pub asset: String,
    /// The video frame to show; for a generator, the milliseconds since it was launched.
    pub frame: u64,
    /// What makes the picture when it is not a video.
    pub generator: Option<flux_project::generator::Generator>,
    /// How the clip is sized into the composition.
    pub fit: flux_project::VideoFit,
    /// How much of it shows over what is below, 0 to 1: less while the clip fades in or out.
    pub opacity: f32,
    /// How it mixes with what is below; a timeline's clips cover it.
    pub blend: flux_project::vj::Blend,
    /// How much larger than its fit it is drawn, about its centre; 1 on a timeline.
    pub scale: f32,
    /// How far its centre moves from where its fit puts it, as fractions of the composition.
    pub position: [f32; 2],
    /// The effects drawn on the picture, the first applied first: the clip's, then its layer's,
    /// without the bypassed ones; none on a timeline.
    pub effects: Vec<flux_project::vj::Effect>,
    /// How long ago the clip was launched, which animated effects run on; nothing on a timeline.
    pub elapsed: std::time::Duration,
    /// The beats the grid's clock has counted, which a generator's text steps its lines on; 0 on a
    /// timeline.
    pub beats: f64,
}

/// The rate and length of a video, which the mapping needs; `None` when it could not be read.
pub type Probe<'a> = &'a dyn Fn(&str) -> Option<(Rate, u64)>;

/// What the video tracks of `timeline` show for `cue`, the topmost first: tracks higher in the
/// timeline cover the ones below. A muted track shows nothing. Where clips of one track overlap,
/// the one that starts later is on top, as when a clip is dropped over another; while it fades,
/// the clip under it on the track shows through, which cross-fades the two.
pub fn layers(timeline: &Timeline, cue: Cue, probe: Probe<'_>) -> Vec<Layer> {
    let Cue::At(position) = cue else {
        return Vec::new();
    };
    let rate: FrameRate = timeline.timecode.frame_rate;
    timeline
        .tracks
        .iter()
        .filter(|track| !track.muted)
        .flat_map(|track| {
            let TrackKind::Video { clips } = &track.kind else {
                return Vec::new();
            };
            // Reversed, so that of two clips starting together the later in the list stays on top.
            let mut showing: Vec<(u64, Layer)> = clips
                .iter()
                .rev()
                .filter_map(|clip| {
                    let (video, frames) = probe(&clip.asset)?;
                    let frame = source_frame(clip, position, rate, video, frames)?;
                    let rest = timeline_frames(frames, video, rate).saturating_sub(clip.offset);
                    let length = clip.length.unwrap_or(rest).min(rest);
                    Some((
                        clip.start,
                        Layer {
                            track: track.id.clone(),
                            clip: clip.id.clone(),
                            asset: clip.asset.clone(),
                            frame,
                            generator: None,
                            fit: clip.fit,
                            opacity: opacity(clip, position, length),
                            blend: Default::default(),
                            scale: 1.0,
                            position: [0.0, 0.0],
                            effects: Vec::new(),
                            elapsed: Default::default(),
                            beats: 0.0,
                        },
                    ))
                })
                .collect();
            showing.sort_by_key(|(start, _)| std::cmp::Reverse(*start));
            // The latest clip, and the one under it while it fades.
            let fading = showing.first().is_some_and(|(_, top)| top.opacity < 1.0);
            showing.truncate(if fading { 2 } else { 1 });
            showing
                .into_iter()
                .map(|(_, layer)| layer)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// How much of `clip`, which lasts `length` frames of the timeline, shows at `position`: it rises
/// from 0 on its first frame over its fade in and falls to 0 on its last frame over its fade out.
pub fn opacity(clip: &flux_project::VideoClip, position: u64, length: u64) -> f32 {
    let into = position.saturating_sub(clip.start);
    let ramp = |frames: u64, fade: u64| {
        if fade == 0 {
            1.0
        } else {
            (frames as f32 / fade as f32).min(1.0)
        }
    };
    ramp(into, clip.fade_in) * ramp(length.saturating_sub(into + 1), clip.fade_out)
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
                generator: None,
                fit: clip.fit,
                opacity: 0.0,
                blend: Default::default(),
                scale: 1.0,
                position: [0.0, 0.0],
                effects: Vec::new(),
                elapsed: Default::default(),
                beats: 0.0,
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
            fade_in: 0,
            fade_out: 0,
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
    fn a_fade_should_rise_from_nothing_and_fall_back_to_nothing() {
        let mut faded = clip("c", 10, Some(20));
        (faded.fade_in, faded.fade_out) = (4, 10);
        let at = |position| opacity(&faded, position, 20);
        assert_eq!(
            [at(10), at(11), at(12), at(14), at(18)],
            [0.0, 0.25, 0.5, 1.0, 1.0]
        );
        // The last frame (29) is black, five frames before it half way.
        assert_eq!([at(29), at(24), at(19)], [0.0, 0.5, 1.0]);
    }

    #[test]
    fn a_fading_clip_should_show_the_clip_under_it_on_its_track() {
        let mut late = clip("late", 10, Some(20));
        late.fade_in = 10;
        let timeline = timeline(vec![track("v", false, vec![clip("early", 0, None), late])]);
        let shown = layers(&timeline, Cue::At(15), &probe);
        assert_eq!(ids(&shown), [("late", 5), ("early", 15)]);
        assert_eq!((shown[0].opacity, shown[1].opacity), (0.5, 1.0));
        // Faded in, it covers the other alone.
        assert_eq!(ids(&layers(&timeline, Cue::At(20), &probe)), [("late", 10)]);
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
