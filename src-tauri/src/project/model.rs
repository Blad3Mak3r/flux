use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::error::{ProjectError, Result};

/// Version written to `project.json`. Readers reject anything newer.
pub const FORMAT_VERSION: u32 = 1;

/// Folder inside the archive that holds the audio files (flat, no subfolders).
pub const AUDIO_DIR: &str = "audio/";

/// Contents of `project.json`. All positions are frame counts at the project frame rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub timecode: TimecodeSettings,
    pub assets: Vec<AudioAsset>,
    pub tracks: Vec<Track>,
    pub output: OutputConfig,
}

/// The four rates MTC can express; LTC supports them too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrameRate {
    Fps24,
    Fps25,
    Fps2997Drop,
    Fps30,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimecodeSettings {
    pub frame_rate: FrameRate,
    /// Frame count of the timeline origin (e.g. 01:00:00:00).
    pub start: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioAsset {
    pub id: String,
    pub name: String,
    /// Archive path, `audio/<file name>`.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    pub muted: bool,
    pub kind: TrackKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TrackKind {
    Audio { clips: Vec<AudioClip> },
    Events { triggers: Vec<Trigger> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: String,
    /// Id of an entry in `Project::assets`.
    pub asset: String,
    pub start: u64,
    pub gain_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trigger {
    pub id: String,
    pub at: u64,
    pub label: String,
    pub action: TriggerAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TriggerAction {
    Marker,
    Midi { bytes: Vec<u8> },
}

/// Devices are machine-specific, so only a name hint is stored, never an OS identifier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    pub ltc: Option<LtcOutput>,
    pub mtc: Option<MtcOutput>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LtcOutput {
    pub device_name: String,
    pub channel: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MtcOutput {
    pub port_name: String,
}

impl Project {
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            name: name.into(),
            timecode: TimecodeSettings {
                frame_rate,
                start: 0,
            },
            assets: Vec::new(),
            tracks: Vec::new(),
            output: OutputConfig::default(),
        }
    }

    /// Checks the invariants the archive format relies on: unique ids, clips pointing at known
    /// assets, and asset files living directly under `audio/`.
    pub fn validate(&self) -> Result<()> {
        let mut asset_ids = HashSet::new();
        let mut asset_files = HashSet::new();
        for asset in &self.assets {
            check_asset_path(&asset.file)?;
            ensure_unique(&mut asset_ids, "asset id", &asset.id)?;
            ensure_unique(&mut asset_files, "asset file", &asset.file)?;
        }

        let mut track_ids = HashSet::new();
        let mut item_ids = HashSet::new();
        for track in &self.tracks {
            ensure_unique(&mut track_ids, "track id", &track.id)?;
            match &track.kind {
                TrackKind::Audio { clips } => {
                    for clip in clips {
                        ensure_unique(&mut item_ids, "clip/trigger id", &clip.id)?;
                        if !asset_ids.contains(clip.asset.as_str()) {
                            return Err(ProjectError::Invalid(format!(
                                "clip {} references unknown asset {}",
                                clip.id, clip.asset
                            )));
                        }
                    }
                }
                TrackKind::Events { triggers } => {
                    for trigger in triggers {
                        ensure_unique(&mut item_ids, "clip/trigger id", &trigger.id)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// An asset file must be `audio/<name>` with a plain file name: no separators, no `.`/`..`.
pub(super) fn check_asset_path(file: &str) -> Result<()> {
    let valid = file
        .strip_prefix(AUDIO_DIR)
        .is_some_and(|name| !matches!(name, "" | "." | "..") && !name.contains(['/', '\\', '\0']));
    if valid {
        Ok(())
    } else {
        Err(ProjectError::InvalidAssetPath(file.to_owned()))
    }
}

fn ensure_unique<'a>(seen: &mut HashSet<&'a str>, what: &str, value: &'a str) -> Result<()> {
    if seen.insert(value) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate {what}: {value}")))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn sample_project() -> Project {
        Project {
            format_version: FORMAT_VERSION,
            name: "Opening night".into(),
            timecode: TimecodeSettings {
                frame_rate: FrameRate::Fps2997Drop,
                start: 107_892,
            },
            assets: vec![AudioAsset {
                id: "a1".into(),
                name: "Click".into(),
                file: "audio/click.wav".into(),
            }],
            tracks: vec![
                Track {
                    id: "t1".into(),
                    name: "Backing".into(),
                    muted: false,
                    kind: TrackKind::Audio {
                        clips: vec![AudioClip {
                            id: "c1".into(),
                            asset: "a1".into(),
                            start: 120,
                            gain_db: -3.5,
                        }],
                    },
                },
                Track {
                    id: "t2".into(),
                    name: "Cues".into(),
                    muted: true,
                    kind: TrackKind::Events {
                        triggers: vec![
                            Trigger {
                                id: "e1".into(),
                                at: 300,
                                label: "Lights up".into(),
                                action: TriggerAction::Marker,
                            },
                            Trigger {
                                id: "e2".into(),
                                at: 450,
                                label: "Program change".into(),
                                action: TriggerAction::Midi {
                                    bytes: vec![0xC0, 0x05],
                                },
                            },
                        ],
                    },
                },
            ],
            output: OutputConfig {
                ltc: Some(LtcOutput {
                    device_name: "Main Interface".into(),
                    channel: 3,
                }),
                mtc: Some(MtcOutput {
                    port_name: "IAC Driver Bus 1".into(),
                }),
            },
        }
    }

    const V1_FIXTURE: &str = r#"{
        "formatVersion": 1,
        "name": "Opening night",
        "timecode": { "frameRate": "fps2997Drop", "start": 107892 },
        "assets": [{ "id": "a1", "name": "Click", "file": "audio/click.wav" }],
        "tracks": [
            {
                "id": "t1", "name": "Backing", "muted": false,
                "kind": { "type": "audio", "clips": [
                    { "id": "c1", "asset": "a1", "start": 120, "gainDb": -3.5 }
                ] }
            },
            {
                "id": "t2", "name": "Cues", "muted": true,
                "kind": { "type": "events", "triggers": [
                    { "id": "e1", "at": 300, "label": "Lights up", "action": { "type": "marker" } },
                    { "id": "e2", "at": 450, "label": "Program change",
                      "action": { "type": "midi", "bytes": [192, 5] } }
                ] }
            }
        ],
        "output": {
            "ltc": { "deviceName": "Main Interface", "channel": 3 },
            "mtc": { "portName": "IAC Driver Bus 1" }
        }
    }"#;

    #[test]
    fn v1_fixture_should_parse_into_the_sample_project() {
        let parsed: Project = serde_json::from_str(V1_FIXTURE).unwrap();
        assert_eq!(parsed, sample_project());
    }

    #[test]
    fn sample_project_should_serialize_to_the_v1_fixture_shape() {
        let expected: serde_json::Value = serde_json::from_str(V1_FIXTURE).unwrap();
        let actual = serde_json::to_value(sample_project()).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn validate_should_accept_the_sample_project() {
        assert!(sample_project().validate().is_ok());
    }

    #[test]
    fn validate_should_reject_a_clip_with_an_unknown_asset() {
        let mut project = sample_project();
        if let TrackKind::Audio { clips } = &mut project.tracks[0].kind {
            clips[0].asset = "missing".into();
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_duplicate_ids() {
        let mut project = sample_project();
        project.tracks[1].id = "t1".into();
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_asset_paths_outside_the_audio_folder() {
        for bad in [
            "audio/../evil.wav",
            "audio/sub/x.wav",
            "audio/",
            "click.wav",
            "/audio/click.wav",
            "audio\\x.wav",
        ] {
            let mut project = sample_project();
            project.assets[0].file = bad.into();
            assert!(
                matches!(project.validate(), Err(ProjectError::InvalidAssetPath(_))),
                "{bad} should be rejected"
            );
        }
    }
}
