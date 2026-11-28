//! A timeline: its timecode, its tempo and its tracks.

use flux_core::FrameRate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::*;
use crate::error::{ProjectError, Result};

/// One line of time of a project, e.g. a song. All positions in it are frame counts at its own
/// frame rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<crate::show::TimelineControl>,
    pub id: String,
    pub name: String,
    pub timecode: TimecodeSettings,
    /// The beat grid of the timeline and its metronome. Absent in files written before it existed,
    /// read as the default tempo with the click off.
    #[serde(default)]
    pub tempo: Tempo,
    pub tracks: Vec<Track>,
}

/// Slowest and fastest tempo a timeline can hold.
pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 400.0;
/// Most beats a bar can hold.
pub const MAX_BEATS_PER_BAR: u8 = 16;

/// The beat grid of a timeline: beats are counted from its origin. `click` says whether the
/// metronome plays while this timeline runs; where it plays is `output.metronome`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tempo {
    pub bpm: f64,
    pub beats_per_bar: u8,
    #[serde(default)]
    pub click: bool,
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            click: false,
        }
    }
}

impl Tempo {
    pub(super) fn validate(&self) -> Result<()> {
        if !(MIN_BPM..=MAX_BPM).contains(&self.bpm) {
            return Err(ProjectError::Invalid(format!(
                "tempo must be between {MIN_BPM} and {MAX_BPM} BPM: {}",
                self.bpm
            )));
        }
        if !(1..=MAX_BEATS_PER_BAR).contains(&self.beats_per_bar) {
            return Err(ProjectError::Invalid(format!(
                "a bar holds 1 to {MAX_BEATS_PER_BAR} beats: {}",
                self.beats_per_bar
            )));
        }
        Ok(())
    }
}

impl Timeline {
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            controls: Vec::new(),
            name: name.into(),
            timecode: TimecodeSettings {
                frame_rate,
                start: 0,
            },
            tempo: Tempo::default(),
            tracks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimecodeSettings {
    pub frame_rate: FrameRate,
    /// Frame count of the timeline origin (e.g. 01:00:00:00).
    pub start: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProjectError;

    use crate::model::tests::*;

    #[test]
    fn a_file_without_tempo_should_read_the_default_with_the_click_off() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["timelines"][0]
            .as_object_mut()
            .unwrap()
            .remove("tempo");
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.timelines[0].tempo, Tempo::default());
        assert!(!parsed.timelines[0].tempo.click);
    }

    #[test]
    pub(super) fn validate_should_reject_a_tempo_out_of_range() {
        for (bpm, beats) in [
            (0.0, 4),
            (19.9, 4),
            (400.1, 4),
            (f64::NAN, 4),
            (120.0, 0),
            (120.0, 17),
        ] {
            let mut project = sample_project();
            project.timelines[0].tempo.bpm = bpm;
            project.timelines[0].tempo.beats_per_bar = beats;
            assert!(
                matches!(project.validate(), Err(ProjectError::Invalid(_))),
                "{bpm} BPM, {beats} beats should be rejected"
            );
        }
    }
}
