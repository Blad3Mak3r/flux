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
    /// Where the tempo or the time signature changes after the origin, in order of position. Absent
    /// is none: the whole timeline follows `tempo`, which is the first point of the tempo map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tempo_changes: Vec<TempoChange>,
    /// Named points of the timeline, in order of position.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Named stretches of the timeline, in order of start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<Region>,
    pub tracks: Vec<Track>,
}

/// Slowest and fastest tempo a timeline can hold.
pub const MIN_BPM: f64 = 20.0;
pub const MAX_BPM: f64 = 400.0;
/// Most beats a bar can hold.
pub const MAX_BEATS_PER_BAR: u8 = 16;
/// The lengths of note a beat can be: 1 is a whole note, 4 a quarter, 8 an eighth.
pub const BEAT_UNITS: [u8; 5] = [1, 2, 4, 8, 16];
/// Most tempo changes, markers and regions a timeline holds, each.
pub const MAX_MARKS: usize = 2_000;
/// The most characters the name of a marker or a region has.
pub const MAX_MARK_NAME: usize = 64;

/// The beat grid of a timeline: beats are counted from its origin. `click` says whether the
/// metronome plays while this timeline runs; where it plays is `output.metronome`. `bpm` counts
/// quarter notes a minute; `beat_unit` is the length of note one beat is (4 is a quarter, 8 an
/// eighth), so 6 beats of an eighth is a 6/8 bar. This is the first point of the tempo map.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tempo {
    pub bpm: f64,
    pub beats_per_bar: u8,
    #[serde(default = "quarter", skip_serializing_if = "is_quarter")]
    pub beat_unit: u8,
    #[serde(default)]
    pub click: bool,
}

fn quarter() -> u8 {
    4
}

fn is_quarter(unit: &u8) -> bool {
    *unit == 4
}

impl Default for Tempo {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            beat_unit: 4,
            click: false,
        }
    }
}

/// A point of the tempo map after the origin: from `at` on, the timeline runs at `bpm` with bars
/// of `beats_per_bar` beats of `beat_unit`. A change starts a new bar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoChange {
    pub id: String,
    /// Frames from the timeline origin.
    pub at: u64,
    pub bpm: f64,
    pub beats_per_bar: u8,
    #[serde(default = "quarter", skip_serializing_if = "is_quarter")]
    pub beat_unit: u8,
}

/// A named point of a timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub id: String,
    /// Frames from the timeline origin.
    pub at: u64,
    pub name: String,
    /// `#rrggbb`; absent is the default colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A named stretch of a timeline: a song, a section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub id: String,
    /// Frames from the timeline origin; `end` is after `start`.
    pub start: u64,
    pub end: u64,
    pub name: String,
    /// `#rrggbb`; absent is the default colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl Tempo {
    pub(crate) fn validate(&self) -> Result<()> {
        if !(MIN_BPM..=MAX_BPM).contains(&self.bpm) {
            return Err(ProjectError::Invalid(format!(
                "tempo must be between {MIN_BPM} and {MAX_BPM} BPM: {}",
                self.bpm
            )));
        }
        validate_signature(self.beats_per_bar, self.beat_unit)
    }
}

fn validate_signature(beats_per_bar: u8, beat_unit: u8) -> Result<()> {
    if !(1..=MAX_BEATS_PER_BAR).contains(&beats_per_bar) {
        return Err(ProjectError::Invalid(format!(
            "a bar holds 1 to {MAX_BEATS_PER_BAR} beats: {beats_per_bar}"
        )));
    }
    if !BEAT_UNITS.contains(&beat_unit) {
        return Err(ProjectError::Invalid(format!(
            "a beat is a 1, 2, 4, 8 or 16th note: {beat_unit}"
        )));
    }
    Ok(())
}

impl Timeline {
    /// Refuses a tempo map, markers or regions that are out of order, out of range or not unique.
    /// `ids` collects the ids across the project, which share one namespace with the items.
    pub(super) fn validate_marks<'a>(
        &'a self,
        ids: &mut std::collections::HashSet<&'a str>,
    ) -> Result<()> {
        if [
            self.tempo_changes.len(),
            self.markers.len(),
            self.regions.len(),
        ]
        .iter()
        .any(|&n| n > MAX_MARKS)
        {
            return Err(ProjectError::Invalid(format!(
                "a timeline holds at most {MAX_MARKS} tempo changes, markers or regions each"
            )));
        }
        let mut last = 0;
        for change in &self.tempo_changes {
            Tempo {
                bpm: change.bpm,
                beats_per_bar: change.beats_per_bar,
                beat_unit: change.beat_unit,
                click: false,
            }
            .validate()?;
            if change.at <= last {
                return Err(ProjectError::Invalid(
                    "tempo changes go after the origin, in order, one to a position".into(),
                ));
            }
            last = change.at;
            super::validate::ensure_unique(ids, "tempo change id", &change.id)?;
        }
        for marker in &self.markers {
            check_mark(&marker.name, marker.color.as_deref())?;
            super::validate::ensure_unique(ids, "marker id", &marker.id)?;
        }
        for region in &self.regions {
            check_mark(&region.name, region.color.as_deref())?;
            if region.end <= region.start {
                return Err(ProjectError::Invalid(format!(
                    "region {} must end after it starts",
                    region.id
                )));
            }
            super::validate::ensure_unique(ids, "region id", &region.id)?;
        }
        Ok(())
    }
}

fn check_mark(name: &str, color: Option<&str>) -> Result<()> {
    if name.chars().count() > MAX_MARK_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name has at most {MAX_MARK_NAME} characters"
        )));
    }
    color.map_or(Ok(()), super::validate::check_color)
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
            tempo_changes: Vec::new(),
            markers: Vec::new(),
            regions: Vec::new(),
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
