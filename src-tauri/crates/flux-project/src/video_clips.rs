//! What the user can do to a video clip: move it, trim its edges, split it in two and remove it.
//! The window says which edit and at which frame of the timeline; the rules live here. Like an audio
//! clip, its `offset` and `length` count frames at the timeline's rate.

use serde::Deserialize;

use super::error::{ProjectError, Result};
use super::model::{VideoClip, VideoFit};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoClipOp {
    /// The clip starts at the frame.
    Move,
    /// Moves the start edge to the frame; the picture stays where it was on the timeline. An edge
    /// dragged too far stops at the other edge, or at the start of the video.
    TrimStart,
    /// Moves the end edge to the frame, no further than the end of the video.
    TrimEnd,
    /// Cuts the clip in two at the frame.
    Split,
    /// Takes the clip off its track.
    Remove,
    /// Sizes the video into the composition this way; the frame is not used.
    Fit(VideoFit),
}

fn refused(clip: &VideoClip, why: &str) -> ProjectError {
    ProjectError::Invalid(format!("clip {}: {why}", clip.id))
}

/// Applies `op` at `frame` (counted like `start`) to `clip`, whose video lasts `video_frames`
/// frames at the timeline's rate. Gives the clips that replace it: none for a removal, one, or two
/// for a split, whose new half is called `new_id`.
pub fn apply(
    clip: &VideoClip,
    op: VideoClipOp,
    frame: u64,
    video_frames: u64,
    new_id: impl FnOnce() -> String,
) -> Result<Vec<VideoClip>> {
    let rest = video_frames.saturating_sub(clip.offset);
    let length = clip.length.unwrap_or(rest).min(rest);
    let end = clip.start + length;
    let mut edited = clip.clone();
    match op {
        VideoClipOp::Move => edited.start = frame,
        VideoClipOp::Remove => return Ok(Vec::new()),
        VideoClipOp::Fit(fit) => edited.fit = fit,
        VideoClipOp::TrimStart => {
            if length == 0 {
                return Err(refused(clip, "there is no video left to trim"));
            }
            let start = frame
                .max(clip.start.saturating_sub(clip.offset))
                .min(end - 1);
            edited.offset = clip.offset + start - clip.start;
            edited.start = start;
            edited.length = (clip.length.is_some() || edited.offset > 0).then_some(end - start);
        }
        VideoClipOp::TrimEnd => {
            if length == 0 {
                return Err(refused(clip, "there is no video left to trim"));
            }
            let length = frame.saturating_sub(clip.start).max(1).min(rest);
            edited.length = (length < rest || clip.offset > 0).then_some(length);
        }
        VideoClipOp::Split => {
            if frame <= clip.start || frame >= end {
                return Err(refused(clip, "the cut must fall inside the clip"));
            }
            edited.length = Some(frame - clip.start);
            let second = VideoClip {
                id: new_id(),
                asset: clip.asset.clone(),
                start: frame,
                name: clip.name.clone(),
                offset: clip.offset + frame - clip.start,
                length: Some(end - frame),
                fit: clip.fit,
            };
            return Ok(vec![edited, second]);
        }
    }
    Ok(vec![edited])
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: u64 = 1000;

    fn clip() -> VideoClip {
        VideoClip {
            id: "c1".into(),
            asset: "v1".into(),
            start: 100,
            name: None,
            offset: 0,
            length: None,
            fit: VideoFit::Fit,
        }
    }

    #[test]
    fn fit_should_change_only_how_the_video_is_sized() {
        let fitted = edit(&clip(), VideoClipOp::Fit(VideoFit::Fill), 0);
        assert_eq!(fitted.fit, VideoFit::Fill);
        assert_eq!((fitted.start, fitted.offset, fitted.length), (100, 0, None));
    }

    #[test]
    fn a_split_should_keep_the_fit_on_both_halves() {
        let mut filled = clip();
        filled.fit = VideoFit::None;
        let halves = apply(&filled, VideoClipOp::Split, 300, VIDEO, || "c2".into()).unwrap();
        assert!(halves.iter().all(|half| half.fit == VideoFit::None));
    }

    #[test]
    fn the_fit_should_be_written_only_when_it_is_not_fit() {
        let json = serde_json::to_value(clip()).unwrap();
        assert!(json.get("fit").is_none());
        let mut stretched = clip();
        stretched.fit = VideoFit::Stretch;
        let json = serde_json::to_value(&stretched).unwrap();
        assert_eq!(json["fit"], "stretch");
        let op: VideoClipOp = serde_json::from_str(r#"{"fit":"none"}"#).unwrap();
        assert_eq!(op, VideoClipOp::Fit(VideoFit::None));
        let op: VideoClipOp = serde_json::from_str(r#""move""#).unwrap();
        assert_eq!(op, VideoClipOp::Move);
    }

    fn edit(clip: &VideoClip, op: VideoClipOp, frame: u64) -> VideoClip {
        let mut result = apply(clip, op, frame, VIDEO, || "c2".into()).unwrap();
        assert_eq!(result.len(), 1);
        result.remove(0)
    }

    #[test]
    fn move_should_change_only_the_start() {
        let moved = edit(&clip(), VideoClipOp::Move, 40);
        assert_eq!((moved.start, moved.offset, moved.length), (40, 0, None));
    }

    #[test]
    fn trim_start_should_keep_the_picture_in_place() {
        let trimmed = edit(&clip(), VideoClipOp::TrimStart, 150);
        assert_eq!(
            (trimmed.start, trimmed.offset, trimmed.length),
            (150, 50, Some(950))
        );
        // Not before the video starts, and never past the end.
        let back = edit(&trimmed, VideoClipOp::TrimStart, 0);
        assert_eq!((back.start, back.offset), (100, 0));
        let far = edit(&clip(), VideoClipOp::TrimStart, 5000);
        assert_eq!((far.start, far.length), (1099, Some(1)));
    }

    #[test]
    fn trim_end_should_stop_at_the_end_of_the_video() {
        assert_eq!(edit(&clip(), VideoClipOp::TrimEnd, 600).length, Some(500));
        assert_eq!(edit(&clip(), VideoClipOp::TrimEnd, 9000).length, None);
        assert_eq!(edit(&clip(), VideoClipOp::TrimEnd, 0).length, Some(1));
    }

    #[test]
    fn split_should_give_two_halves_that_show_the_same_frames() {
        let halves = apply(&clip(), VideoClipOp::Split, 400, VIDEO, || "c2".into()).unwrap();
        let first = (halves[0].start, halves[0].offset, halves[0].length);
        let second = (halves[1].start, halves[1].offset, halves[1].length);
        assert_eq!(first, (100, 0, Some(300)));
        assert_eq!(second, (400, 300, Some(700)));
        assert_eq!(halves[1].id, "c2");
        assert!(apply(&clip(), VideoClipOp::Split, 100, VIDEO, || "c2".into()).is_err());
        assert!(apply(&clip(), VideoClipOp::Split, 1100, VIDEO, || "c2".into()).is_err());
    }

    #[test]
    fn remove_should_leave_nothing() {
        assert!(
            apply(&clip(), VideoClipOp::Remove, 0, VIDEO, || "c2".into())
                .unwrap()
                .is_empty()
        );
    }
}
