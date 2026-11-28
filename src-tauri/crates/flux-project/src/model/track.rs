//! Tracks, and the notes of a notes track.

use serde::{Deserialize, Serialize};

use super::*;
use crate::events::{MidiEvent, MidiPortRef, OscEvent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    #[serde(
        default,
        skip_serializing_if = "crate::routing::AudioAssignment::is_inherit"
    )]
    pub audio_output: crate::routing::AudioAssignment,
    pub id: String,
    pub name: String,
    pub muted: bool,
    /// The track's fader in dB; absent is 0 dB. Only audio tracks have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
    /// The colour the track is drawn in, `#rrggbb`; absent is the default for its kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub kind: TrackKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TrackKind {
    Operator {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        voice: Option<String>,
        orders: Vec<crate::VoiceOrder>,
    },
    Audio {
        clips: Vec<AudioClip>,
    },
    /// Text for the operator, shown on the timeline and in the Live view.
    Notes {
        notes: Vec<Note>,
    },
    /// MIDI messages sent at points of the timeline to the track's port.
    Midi {
        /// Absent until the user picks one; the track then fires nothing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<MidiPortRef>,
        events: Vec<MidiEvent>,
    },
    /// OSC messages sent at points of the timeline to one of `output.osc`.
    Osc {
        /// Id of an entry in `OutputConfig::osc`; absent until the user picks one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        destination: Option<String>,
        events: Vec<OscEvent>,
    },
    /// Video clips shown in time with the timeline.
    Video {
        clips: Vec<VideoClip>,
    },
}

/// A note at a point of the timeline. It is shown from `at` for `length` frames, or until the next
/// note of its track when it has none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub at: u64,
    /// How many frames the note lasts; absent means until the next note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// Free text, possibly several lines.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProjectError;

    use crate::model::tests::*;

    #[test]
    fn note_without_a_length_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        let TrackKind::Notes { notes } = &parsed.timelines[0].tracks[1].kind else {
            panic!("the second track holds notes");
        };
        assert_eq!(notes[0].length, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(
            json["timelines"][0]["tracks"][1]["kind"]["notes"][0]
                .get("length")
                .is_none()
        );
    }

    #[test]
    fn validate_should_reject_a_note_that_lasts_no_time() {
        let mut project = sample_project();
        if let TrackKind::Notes { notes } = &mut project.timelines[0].tracks[1].kind {
            notes[0].length = Some(0);
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_note_id_that_a_clip_already_uses() {
        let mut project = sample_project();
        if let TrackKind::Notes { notes } = &mut project.timelines[0].tracks[1].kind {
            notes[0].id = "c1".into();
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn track_without_a_gain_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(parsed.timelines[0].tracks[1].gain_db, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(json["timelines"][0]["tracks"][1].get("gainDb").is_none());
    }

    #[test]
    fn validate_should_reject_a_track_gain_that_is_not_finite() {
        let mut project = sample_project();
        project.timelines[0].tracks[0].gain_db = Some(f32::NAN);
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn track_without_a_colour_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(parsed.timelines[0].tracks[1].color, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(json["timelines"][0]["tracks"][1].get("color").is_none());
        assert_eq!(json["timelines"][0]["tracks"][0]["color"], "#5cd0ea");
    }

    #[test]
    fn validate_should_reject_a_track_colour_that_is_not_a_hex_triplet() {
        for bad in ["red", "#fff", "5cd0ea", "#5cd0eg", "#5cd0ea; x", ""] {
            let mut project = sample_project();
            project.timelines[0].tracks[0].color = Some(bad.into());
            assert!(
                matches!(project.validate(), Err(ProjectError::Invalid(_))),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn a_video_track_should_round_trip_through_json() {
        let project = video_project();
        project.validate().unwrap();
        let json = serde_json::to_value(&project).unwrap();
        assert_eq!(json["videos"][0]["file"], "video/intro.mp4");
        let kind = &json["timelines"][0]["tracks"][4]["kind"];
        assert_eq!(kind["type"], "video");
        assert_eq!(kind["clips"][0]["offset"], 5);
        assert_eq!(serde_json::from_value::<Project>(json).unwrap(), project);
    }
}
