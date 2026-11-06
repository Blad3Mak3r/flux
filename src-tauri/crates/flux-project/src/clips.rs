//! What the user can do to an audio clip besides moving it: trim its edges, fade it in and out, and
//! split it in two. The window only says which edit and at which frame of the timeline; the rules
//! (what stays in place, what is clamped, what is refused) live here.

use serde::Deserialize;

use super::error::{ProjectError, Result};
use super::model::AudioClip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipOp {
    /// Moves the start edge to the frame; the audio stays where it was on the timeline. An edge
    /// dragged too far stops at the other edge, or at the start of the asset.
    TrimStart,
    /// Moves the end edge to the frame.
    TrimEnd,
    /// The fade in ends at the frame.
    FadeIn,
    /// The fade out begins at the frame.
    FadeOut,
    /// Cuts the clip in two at the frame. A fade the cut falls inside of cannot be shared out, so
    /// each half keeps what fits of it.
    Split,
}

fn refused(clip: &AudioClip, why: &str) -> ProjectError {
    ProjectError::Invalid(format!("clip {}: {why}", clip.id))
}

/// Applies `op` at `frame` (a position of the timeline, counted like `start`) to `clip`, whose
/// asset lasts `asset_frames`. Gives the clips that replace it: one, or two for a split, whose new
/// half is called `new_id`.
pub fn apply(
    clip: &AudioClip,
    op: ClipOp,
    frame: u64,
    asset_frames: u64,
    new_id: impl FnOnce() -> String,
) -> Result<Vec<AudioClip>> {
    let rest = asset_frames.saturating_sub(clip.offset);
    let length = clip.played_frames(asset_frames);
    let end = clip.start + length;
    let mut edited = clip.clone();
    match op {
        ClipOp::TrimStart => {
            if length == 0 {
                return Err(refused(clip, "there is no audio left to trim"));
            }
            // The audio cannot begin before the asset does, and at least a frame must remain.
            let start = frame
                .max(clip.start.saturating_sub(clip.offset))
                .min(end - 1);
            edited.offset = clip.offset + start - clip.start;
            edited.start = start;
            // An offset needs a length, so that the clip reads as it sounds without the asset.
            edited.length = (clip.length.is_some() || edited.offset > 0).then_some(end - start);
        }
        ClipOp::TrimEnd => {
            if length == 0 {
                return Err(refused(clip, "there is no audio left to trim"));
            }
            // At least a frame must remain, and no more than the asset has.
            let length = frame.saturating_sub(clip.start).max(1).min(rest);
            edited.length = (length < rest || clip.offset > 0).then_some(length);
        }
        ClipOp::FadeIn => edited.fade_in = frame.saturating_sub(clip.start),
        ClipOp::FadeOut => edited.fade_out = end.saturating_sub(frame),
        ClipOp::Split => {
            if frame <= clip.start || frame >= end {
                return Err(refused(clip, "the cut must fall inside the clip"));
            }
            edited.length = Some(frame - clip.start);
            edited.fade_out = 0;
            let mut second = AudioClip::new(new_id(), clip.asset.clone(), frame);
            second.gain_db = clip.gain_db;
            second.name = clip.name.clone();
            second.offset = clip.offset + frame - clip.start;
            second.length = Some(end - frame);
            second.fade_out = clip.fade_out;
            fit_fades(&mut edited, asset_frames);
            fit_fades(&mut second, asset_frames);
            return Ok(vec![edited, second]);
        }
    }
    fit_fades(&mut edited, asset_frames);
    Ok(vec![edited])
}

/// Keeps the fades inside the clip: the second one gives way to the first.
fn fit_fades(clip: &mut AudioClip, asset_frames: u64) {
    let length = clip.played_frames(asset_frames);
    clip.fade_in = clip.fade_in.min(length);
    clip.fade_out = clip.fade_out.min(length - clip.fade_in);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSET: u64 = 1000;

    fn clip() -> AudioClip {
        AudioClip::new("c1", "a1", 100)
    }

    fn edit(clip: &AudioClip, op: ClipOp, frame: u64) -> AudioClip {
        let mut result = apply(clip, op, frame, ASSET, || "c2".into()).unwrap();
        assert_eq!(result.len(), 1);
        result.remove(0)
    }

    #[test]
    fn trim_start_should_keep_the_audio_in_place_and_fix_the_length() {
        let trimmed = edit(&clip(), ClipOp::TrimStart, 150);
        assert_eq!(
            (trimmed.start, trimmed.offset, trimmed.length),
            (150, 50, Some(950))
        );
    }

    #[test]
    fn trim_start_should_stop_at_the_start_of_the_asset() {
        let trimmed = edit(&edit(&clip(), ClipOp::TrimStart, 150), ClipOp::TrimStart, 0);
        assert_eq!(
            (trimmed.start, trimmed.offset, trimmed.length),
            (100, 0, Some(1000))
        );
    }

    #[test]
    fn trim_start_should_leave_a_frame_when_dragged_past_the_end() {
        let trimmed = edit(&clip(), ClipOp::TrimStart, 5000);
        assert_eq!(trimmed.length, Some(1));
    }

    #[test]
    fn trim_end_should_set_the_length_and_drop_it_at_the_end_of_the_asset() {
        let trimmed = edit(&clip(), ClipOp::TrimEnd, 400);
        assert_eq!(trimmed.length, Some(300));
        assert_eq!(edit(&trimmed, ClipOp::TrimEnd, 5000).length, None);
    }

    #[test]
    fn trim_end_should_leave_a_frame_when_dragged_before_the_start() {
        assert_eq!(edit(&clip(), ClipOp::TrimEnd, 10).length, Some(1));
    }

    #[test]
    fn fades_should_never_overlap() {
        let faded = edit(&clip(), ClipOp::TrimEnd, 200);
        let faded = edit(&faded, ClipOp::FadeIn, 160);
        let faded = edit(&faded, ClipOp::FadeOut, 120);
        assert_eq!((faded.fade_in, faded.fade_out), (60, 40));
    }

    #[test]
    fn a_fade_handle_at_the_edge_should_remove_the_fade() {
        let faded = edit(&clip(), ClipOp::FadeIn, 150);
        assert_eq!(edit(&faded, ClipOp::FadeIn, 100).fade_in, 0);
    }

    #[test]
    fn trimming_should_shrink_the_fades_with_the_clip() {
        let faded = edit(&edit(&clip(), ClipOp::FadeIn, 300), ClipOp::FadeOut, 900);
        let trimmed = edit(&faded, ClipOp::TrimEnd, 150);
        assert_eq!((trimmed.fade_in, trimmed.fade_out), (50, 0));
    }

    #[test]
    fn split_should_cut_the_clip_in_two_that_continue_each_other() {
        let faded = edit(&edit(&clip(), ClipOp::FadeIn, 120), ClipOp::FadeOut, 1000);
        let halves = apply(&faded, ClipOp::Split, 400, ASSET, || "c2".into()).unwrap();
        let [first, second] = &halves[..] else {
            panic!("expected two clips")
        };
        assert_eq!(
            (first.start, first.offset, first.length),
            (100, 0, Some(300))
        );
        assert_eq!(
            (second.start, second.offset, second.length),
            (400, 300, Some(700))
        );
        assert_eq!((first.fade_in, first.fade_out), (20, 0));
        assert_eq!((second.fade_in, second.fade_out), (0, 100));
        assert_eq!(second.id, "c2");
        // Both halves measure the asset from the same frame.
        assert_eq!(first.start - first.offset, second.start - second.offset);
    }

    #[test]
    fn split_should_give_both_halves_the_name_of_the_clip() {
        let named = AudioClip {
            name: Some("Intro".into()),
            ..clip()
        };
        let halves = apply(&named, ClipOp::Split, 400, ASSET, || "c2".into()).unwrap();
        assert!(
            halves
                .iter()
                .all(|half| half.name.as_deref() == Some("Intro"))
        );
    }

    #[test]
    fn split_should_refuse_a_cut_outside_the_clip() {
        for frame in [100, 1100, 50, 2000] {
            assert!(apply(&clip(), ClipOp::Split, frame, ASSET, || "x".into()).is_err());
        }
    }
}
