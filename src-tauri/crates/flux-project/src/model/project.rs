//! The project and its autosave.

use std::collections::HashSet;

use flux_core::FrameRate;
use serde::{Deserialize, Serialize};

use super::*;
use crate::error::{ProjectError, Result};

/// Contents of `project.json`. Audio and logical signals belong to the project; each
/// timeline has its own timecode and tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setlists: Vec<crate::show::Setlist>,
    pub format_version: u32,
    pub name: String,
    pub meta: Meta,
    #[serde(default)]
    pub autosave: Autosave,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    pub assets: Vec<AudioAsset>,
    /// The video files the video tracks play. Their ids share the namespace of `assets`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub videos: Vec<VideoAsset>,
    pub output: crate::ShowOutputs,
    /// Never empty. Only one plays at a time.
    pub timelines: Vec<Timeline>,
    /// The kind of project, chosen when it is created and never changed; absent is a timeline project.
    #[serde(default, skip_serializing_if = "crate::vj::ProjectMode::is_timeline")]
    pub mode: crate::vj::ProjectMode,
    /// The VJ grid of a VJ project; a timeline project has none.
    #[serde(default, skip_serializing_if = "crate::vj::Vj::is_empty")]
    pub vj: crate::vj::Vj,
}

/// Recovery preferences travel with the project, never with the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Autosave {
    pub enabled: bool,
    pub interval_seconds: u64,
}

impl Default for Autosave {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 30,
        }
    }
}

impl Autosave {
    pub fn validate(self) -> Result<()> {
        if ![15, 30, 60].contains(&self.interval_seconds) {
            return Err(ProjectError::Invalid(
                "autosave interval must be 15, 30 or 60 seconds".into(),
            ));
        }
        Ok(())
    }
}

impl Project {
    /// A project with one timeline, named like the project.
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        let name = name.into();
        Self {
            format_version: FORMAT_VERSION,
            setlists: Vec::new(),
            meta: Meta::new(),
            autosave: Autosave::default(),
            voice: None,
            assets: Vec::new(),
            videos: Vec::new(),
            output: crate::ShowOutputs::default(),
            timelines: vec![Timeline::new(name.clone(), frame_rate)],
            mode: Default::default(),
            vj: Default::default(),
            name,
        }
    }

    pub fn timeline(&self, id: &str) -> Option<&Timeline> {
        self.timelines.iter().find(|timeline| timeline.id == id)
    }

    /// Ids of the audio and video assets some clip of any timeline plays.
    pub fn used_assets(&self) -> HashSet<&str> {
        let mut used = HashSet::new();
        for track in self.timelines.iter().flat_map(|t| &t.tracks) {
            match &track.kind {
                TrackKind::Audio { clips } => used.extend(clips.iter().map(|c| c.asset.as_str())),
                TrackKind::Video { clips } => used.extend(clips.iter().map(|c| c.asset.as_str())),
                TrackKind::Operator { orders, .. } => used.extend(
                    orders
                        .iter()
                        .filter_map(|o| o.generated.as_ref().map(|g| g.asset.as_str())),
                ),
                _ => {}
            }
        }
        // The image a test card draws as its logo.
        let logos = self
            .output
            .screens
            .iter()
            .flat_map(|screen| &screen.slices)
            .map(|slice| slice.test_card.logo.as_str());
        used.extend(logos.filter(|logo| !logo.is_empty()));
        let grid = self.vj.layers.iter().flat_map(|layer| &layer.clips);
        used.extend(
            grid.map(|clip| clip.asset.as_str())
                .filter(|asset| !asset.is_empty()),
        );
        used
    }

    /// Checks the invariants the archive format relies on: at least one timeline, unique ids (a
    /// track, clip or note id is unique in the whole project), clips pointing at known assets, and
    /// asset files living directly under `audio/`.
    pub fn validate(&self) -> Result<()> {
        crate::show::validate(self)?;
        crate::operator::validate_voice(self.voice.as_deref())?;
        crate::routing::validate(self)?;
        self.meta.validate()?;
        self.autosave.validate()?;
        let mut asset_ids = HashSet::new();
        let mut asset_files = HashSet::new();
        for asset in &self.assets {
            check_asset_path(&asset.file)?;
            check_link(asset.link.as_ref())?;
            ensure_unique(&mut asset_ids, "asset id", &asset.id)?;
            ensure_unique(&mut asset_files, "asset file", &asset.file)?;
            if asset
                .bpm
                .is_some_and(|bpm| !(MIN_BPM..=MAX_BPM).contains(&bpm))
            {
                return Err(ProjectError::Invalid(format!(
                    "asset {} has a tempo outside {MIN_BPM} to {MAX_BPM} BPM",
                    asset.id
                )));
            }
        }
        let mut video_ids = HashSet::new();
        for video in &self.videos {
            check_video_path(&video.file)?;
            check_link(video.link.as_ref())?;
            ensure_unique(&mut asset_ids, "asset id", &video.id)?;
            ensure_unique(&mut asset_files, "asset file", &video.file)?;
            video_ids.insert(video.id.as_str());
        }
        // Audio clips must not play a video, so they check against the audio ids only.
        asset_ids.retain(|id| !video_ids.contains(id));

        if self.timelines.is_empty() {
            return Err(ProjectError::Invalid("a project needs a timeline".into()));
        }
        let mut timeline_ids = HashSet::new();
        let mut destination_ids = HashSet::new();
        for destination in &self.output.osc {
            destination.validate()?;
            ensure_unique(&mut destination_ids, "OSC destination id", &destination.id)?;
        }
        let mut track_ids = HashSet::new();
        let mut item_ids = HashSet::new();
        for timeline in &self.timelines {
            ensure_unique(&mut timeline_ids, "timeline id", &timeline.id)?;
            timeline.tempo.validate()?;
            timeline.validate_marks(&mut item_ids)?;
            for track in &timeline.tracks {
                validate_track(
                    track,
                    &asset_ids,
                    &video_ids,
                    &destination_ids,
                    &mut track_ids,
                    &mut item_ids,
                )?;
            }
        }
        crate::vj::validate(self, &video_ids, &mut item_ids)?;
        self.output.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProjectError;

    use crate::model::tests::*;

    use flux_core::FrameRate;
    use std::collections::HashSet;

    #[test]
    fn fixture_should_parse_into_the_sample_project() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(parsed, sample_project());
    }

    #[test]
    fn sample_project_should_serialize_to_the_fixture_shape() {
        let expected: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let actual = serde_json::to_value(sample_project()).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn validate_should_reject_a_project_id_that_is_not_a_uuid() {
        let mut project = sample_project();
        project.meta.id = "not-a-uuid".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_accept_the_sample_project() {
        assert!(sample_project().validate().is_ok());
    }

    #[test]
    fn validate_should_reject_a_clip_with_an_unknown_asset() {
        let mut project = sample_project();
        if let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind {
            clips[0].asset = "missing".into();
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_duplicate_ids() {
        let mut project = sample_project();
        project.timelines[0].tracks[1].id = "t1".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn project_without_audio_output_should_still_parse() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["output"]["legacyRoutes"]
            .as_object_mut()
            .unwrap()
            .remove("audio");
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.output.legacy_routes.as_ref().unwrap().audio, None);
    }

    #[test]
    fn validate_should_accept_several_timelines() {
        assert!(two_timelines().validate().is_ok());
    }

    #[test]
    fn validate_should_reject_a_project_without_timelines() {
        let mut project = sample_project();
        project.timelines.clear();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_repeated_timeline_id() {
        let mut project = two_timelines();
        project.timelines[1].id = "l1".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_note_id_used_in_another_timeline() {
        let mut project = two_timelines();
        if let TrackKind::Notes { notes } = &mut project.timelines[1].tracks[0].kind {
            notes[0].id = "c1".into();
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn used_assets_should_count_the_clips_of_every_timeline() {
        let mut project = two_timelines();
        let clip = AudioClip::new("c2", "a1", 0);
        project.timelines[1].tracks.push(Track {
            audio_output: Default::default(),
            id: "t4".into(),
            name: "Backing".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Audio { clips: vec![clip] },
        });
        project.timelines[0].tracks.remove(0);

        assert_eq!(project.used_assets(), HashSet::from(["a1"]));
    }

    #[test]
    fn new_should_give_the_first_timeline_the_name_of_the_project() {
        let project = Project::new("Show", FrameRate::Fps30);
        assert_eq!(project.timelines[0].name, "Show");
    }

    #[test]
    fn a_project_without_videos_should_not_write_the_field() {
        let json = serde_json::to_value(sample_project()).unwrap();
        assert!(json.get("videos").is_none());
    }

    #[test]
    fn used_assets_should_count_video_clips() {
        assert!(video_project().used_assets().contains("v1"));
    }
}
