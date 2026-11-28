//! Clips: audio with its envelope and fades, and video.

use serde::{Deserialize, Serialize};

use crate::serde_util::is_zero;

/// How the gain of a fade runs between silence and full level.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FadeCurve {
    /// Amplitude rises in a straight line.
    #[default]
    Linear,
    /// A quarter of a sine: keeps the power even when it crosses another clip that fades the other way.
    EqualPower,
    /// Slow at both ends and quick in the middle.
    SCurve,
    /// The square of the line: quiet for longer, which sounds even for a fade out.
    Exponential,
}

/// How the gain moves from one envelope point to the next.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvCurve {
    /// A straight line between the two points, in dB.
    #[default]
    Linear,
    /// Eases out of the first point and into the second.
    Smooth,
    /// Moves quickly at first and settles into the second point.
    Fast,
    /// Stays near the first point and moves late.
    Slow,
    /// Keeps the first point's gain, then steps to the second just before it (a few ms of ramp, never a click).
    Hold,
}

/// One breakpoint of a clip's volume envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvPoint {
    /// Where the point is, in 1/1000 of a frame from the start of the clip's audio file. Counted
    /// from the file and not from the clip, so moving or trimming the clip leaves it where it is.
    pub at: u64,
    /// Gain at the point; [`ENVELOPE_MIN_DB`] is silence.
    pub gain_db: f32,
    /// How the gain gets from this point to the next; absent is a straight line.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub curve: EnvCurve,
}

/// Envelope points per frame: positions are finer than a frame.
pub const ENVELOPE_TICKS_PER_FRAME: u64 = 1000;

/// The most points a clip's envelope can have.
pub const MAX_ENVELOPE_POINTS: usize = 256;

/// The lowest and highest gain an envelope point can have, in dB. The lowest is silence.
pub const ENVELOPE_MIN_DB: f32 = -60.0;
pub const ENVELOPE_MAX_DB: f32 = 6.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: String,
    /// Id of an entry in `Project::assets`.
    pub asset: String,
    pub start: u64,
    pub gain_db: f32,
    /// The name the clip goes by in the timeline and in Live; absent means the asset's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Frames of the asset skipped at the start of the clip (trim in); absent is 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: u64,
    /// Frames of the asset the clip plays (trim out); absent means up to the end of the asset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// Frames the clip takes to rise from silence; absent is 0 (a hard start).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in: u64,
    /// Frames the clip takes to fall to silence at its end; absent is 0 (a hard cut).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out: u64,
    /// Shape of the fade in; absent is a straight line.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in_curve: FadeCurve,
    /// Shape of the fade out; absent is a straight line.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out_curve: FadeCurve,
    /// The volume drawn over the clip, from the first point to the last; absent is none. It
    /// multiplies `gain_db` and the fades. Before the first point and after the last the gain is
    /// that of the nearest point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub envelope: Vec<EnvPoint>,
}

impl AudioClip {
    /// A clip that plays the whole asset from `start`, at 0 dB.
    pub fn new(id: impl Into<String>, asset: impl Into<String>, start: u64) -> Self {
        Self {
            id: id.into(),
            asset: asset.into(),
            start,
            gain_db: 0.0,
            name: None,
            offset: 0,
            length: None,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: FadeCurve::Linear,
            fade_out_curve: FadeCurve::Linear,
            envelope: Vec::new(),
        }
    }

    /// How many frames of an asset of `asset_frames` the clip plays: its length, but never past
    /// the end of the asset.
    pub fn played_frames(&self, asset_frames: u64) -> u64 {
        let rest = asset_frames.saturating_sub(self.offset);
        self.length.map_or(rest, |length| length.min(rest))
    }
}

/// A stretch of a video file placed on the timeline. Positions are frames at the timeline's rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoClip {
    pub id: String,
    /// Id of an entry in `Project::videos`.
    pub asset: String,
    pub start: u64,
    /// The name the clip goes by in the timeline and in Live; absent means the asset's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Frames skipped at the start of the video (trim in), at the timeline's rate like `start`;
    /// absent is 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: u64,
    /// Frames of the video the clip shows (trim out); absent means up to the end of the video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// How the video is sized into the composition; absent is [`VideoFit::Fit`].
    #[serde(default, skip_serializing_if = "VideoFit::is_fit")]
    pub fit: VideoFit,
    /// Frames the picture takes to rise from black, or from what is below it; absent is 0 (a cut).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_in: u64,
    /// Frames the picture takes to fall to black, or to what is below it, at its end; absent is 0.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade_out: u64,
}

/// How a clip's video is sized into the composition, as a media server sizes a clip into its
/// layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoFit {
    /// As large as it fits with its shape kept, centred: black bars where the shapes differ.
    #[default]
    Fit,
    /// As small as it covers the composition with its shape kept, centred: the edges are cut.
    Fill,
    /// Exactly the composition, its shape changed to it.
    Stretch,
    /// At its own size in pixels, centred.
    None,
}

impl VideoFit {
    fn is_fit(&self) -> bool {
        *self == Self::Fit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProjectError;

    use crate::model::tests::*;

    use crate::model::*;

    #[test]
    fn clip_without_a_trim_or_fades_should_serialize_without_them() {
        let json = serde_json::to_value(AudioClip::new("c1", "a1", 0)).unwrap();
        for field in ["offset", "length", "fadeIn", "fadeOut"] {
            assert!(json.get(field).is_none(), "{field} should be left out");
        }
    }

    #[test]
    fn trimmed_and_faded_clip_should_round_trip() {
        let clip = AudioClip {
            offset: 30,
            length: Some(200),
            fade_in: 5,
            fade_out: 10,
            ..AudioClip::new("c1", "a1", 120)
        };
        let json = serde_json::to_value(&clip).unwrap();
        assert_eq!(json["offset"], 30);
        assert_eq!(json["fadeIn"], 5);
        assert_eq!(serde_json::from_value::<AudioClip>(json).unwrap(), clip);
    }

    #[test]
    fn clip_without_a_name_should_serialize_without_one() {
        let json = serde_json::to_value(AudioClip::new("c1", "a1", 0)).unwrap();
        assert!(json.get("name").is_none());
    }

    #[test]
    fn clip_without_fade_curves_should_serialize_without_them() {
        let clip = AudioClip {
            fade_in: 5,
            ..AudioClip::new("c1", "a1", 0)
        };
        let json = serde_json::to_value(&clip).unwrap();
        for field in ["fadeInCurve", "fadeOutCurve"] {
            assert!(json.get(field).is_none(), "{field} should be left out");
        }
    }

    #[test]
    fn fade_curves_should_round_trip_under_their_camel_case_names() {
        let clip = AudioClip {
            fade_in: 5,
            fade_out: 8,
            fade_in_curve: FadeCurve::SCurve,
            fade_out_curve: FadeCurve::EqualPower,
            ..AudioClip::new("c1", "a1", 0)
        };
        let json = serde_json::to_value(&clip).unwrap();
        assert_eq!(json["fadeInCurve"], "sCurve");
        assert_eq!(json["fadeOutCurve"], "equalPower");
        assert_eq!(serde_json::from_value::<AudioClip>(json).unwrap(), clip);
    }

    #[test]
    fn clip_without_an_envelope_should_serialize_without_one() {
        let json = serde_json::to_value(AudioClip::new("c1", "a1", 0)).unwrap();
        assert!(json.get("envelope").is_none());
    }

    #[test]
    fn envelope_should_round_trip_and_leave_a_straight_line_unnamed() {
        let mut clip = envelope_clip(&[(0, -60.0), (2000, 0.0)]);
        clip.envelope[1].curve = EnvCurve::Smooth;
        let json = serde_json::to_value(&clip).unwrap();
        assert!(json["envelope"][0].get("curve").is_none());
        assert_eq!(json["envelope"][1]["curve"], "smooth");
        assert_eq!(json["envelope"][1]["gainDb"], 0.0);
        assert_eq!(serde_json::from_value::<AudioClip>(json).unwrap(), clip);
    }

    #[test]
    fn validate_should_accept_an_envelope_in_order_and_in_range() {
        let clip = envelope_clip(&[(0, ENVELOPE_MIN_DB), (500, 0.0), (900, ENVELOPE_MAX_DB)]);
        assert!(project_with(clip).validate().is_ok());
    }

    #[test]
    fn validate_should_reject_an_envelope_out_of_order_or_out_of_range() {
        for points in [
            vec![(500, 0.0), (500, -3.0)],
            vec![(900, 0.0), (500, -3.0)],
            vec![(0, ENVELOPE_MIN_DB - 1.0)],
            vec![(0, ENVELOPE_MAX_DB + 1.0)],
            vec![(0, f32::NAN)],
        ] {
            let shown = format!("{points:?}");
            assert!(
                project_with(envelope_clip(&points)).validate().is_err(),
                "{shown} should be refused"
            );
        }
    }

    #[test]
    fn validate_should_reject_an_envelope_with_too_many_points() {
        let points: Vec<(u64, f32)> = (0..=MAX_ENVELOPE_POINTS as u64)
            .map(|at| (at, 0.0))
            .collect();
        assert!(project_with(envelope_clip(&points)).validate().is_err());
        assert!(
            project_with(envelope_clip(&points[..MAX_ENVELOPE_POINTS]))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn named_clip_should_round_trip() {
        let clip = AudioClip {
            name: Some("Intro".into()),
            ..AudioClip::new("c1", "a1", 0)
        };
        let json = serde_json::to_value(&clip).unwrap();
        assert_eq!(json["name"], "Intro");
        assert_eq!(serde_json::from_value::<AudioClip>(json).unwrap(), clip);
    }

    #[test]
    fn validate_should_reject_a_blank_or_too_long_clip_name() {
        for name in ["", "   ", &"x".repeat(MAX_CLIP_NAME + 1)] {
            let mut project = sample_project();
            let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
                panic!("the first track is audio");
            };
            clips[0].name = Some(name.into());
            assert!(project.validate().is_err(), "{name:?} should be refused");
        }
        let mut project = sample_project();
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        clips[0].name = Some("x".repeat(MAX_CLIP_NAME));
        assert!(project.validate().is_ok());
    }

    #[test]
    fn validate_should_reject_a_clip_that_plays_no_time() {
        let mut project = sample_project();
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        clips[0].length = Some(0);
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_reject_a_video_clip_of_an_unknown_or_audio_asset() {
        for asset in ["nope", "a1"] {
            let mut project = video_project();
            video_clip(&mut project).asset = asset.into();
            assert!(
                matches!(project.validate(), Err(ProjectError::Invalid(_))),
                "{asset}"
            );
        }
    }

    #[test]
    fn validate_should_reject_an_audio_clip_that_plays_a_video() {
        let mut project = video_project();
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        clips[0].asset = "v1".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_video_clip_of_no_length_or_a_blank_name() {
        let mut project = video_project();
        video_clip(&mut project).length = Some(0);
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
        let mut project = video_project();
        video_clip(&mut project).name = Some("  ".into());
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }
}
