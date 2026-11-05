use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::error::{ProjectError, Result};

/// Version written to `project.json`. Readers reject anything newer.
///
/// Version 2 turned `output.ltc` and `output.mtc` into lists; `migrate` reads version 1 files.
pub const FORMAT_VERSION: u32 = 2;

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
    /// Where the project's audio clips are played.
    pub audio: Option<AudioOutput>,
    /// Every LTC signal to send; each one names its own device and channel.
    #[serde(default, deserialize_with = "list_or_one")]
    pub ltc: Vec<LtcOutput>,
    /// Every MIDI port that receives MTC.
    #[serde(default, deserialize_with = "list_or_one")]
    pub mtc: Vec<MtcOutput>,
}

/// Channel numbers are 1-based, as an audio interface labels them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioOutput {
    pub device_name: String,
    /// Interface channels that receive the programme: `[1, 2]` for a stereo pair.
    pub channels: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LtcOutput {
    pub device_name: String,
    /// 1-based interface channel.
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
            if track.gain_db.is_some_and(|db| !db.is_finite()) {
                return Err(ProjectError::Invalid(format!(
                    "track {} has a gain that is not a number",
                    track.id
                )));
            }
            if let Some(color) = &track.color {
                check_color(color)?;
            }
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
        self.output.validate()
    }
}

impl OutputConfig {
    /// Channels are 1-based and a programme channel may be listed only once.
    fn validate(&self) -> Result<()> {
        for ltc in &self.ltc {
            check_channel(ltc.channel)?;
        }
        if let Some(audio) = &self.audio {
            let mut seen = HashSet::new();
            for &channel in &audio.channels {
                check_channel(channel)?;
                if !seen.insert(channel) {
                    return Err(ProjectError::Invalid(format!(
                        "duplicate audio output channel: {channel}"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Reads a list that older files wrote as a single object or as `null`. Files carrying version 2
/// with that shape exist (written while the format was changing), so this does not look at the
/// version.
fn list_or_one<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape<T> {
        Many(Vec<T>),
        One(T),
        Nothing(()),
    }
    Ok(match Shape::deserialize(deserializer)? {
        Shape::Many(items) => items,
        Shape::One(item) => vec![item],
        Shape::Nothing(()) => Vec::new(),
    })
}

/// Brings a `project.json` of an older version up to `FORMAT_VERSION`, in place.
///
/// Version 1 held at most one LTC and one MTC output as an object or `null`; version 2 holds lists.
pub(super) fn migrate(value: &mut serde_json::Value) {
    use serde_json::Value;

    let version = value
        .get("formatVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if version >= 2 {
        return;
    }
    if let Some(output) = value.get_mut("output").and_then(Value::as_object_mut) {
        for key in ["ltc", "mtc"] {
            let list = match output.remove(key) {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items,
                Some(single) => vec![single],
            };
            output.insert(key.into(), Value::Array(list));
        }
    }
    value["formatVersion"] = Value::from(FORMAT_VERSION);
}

/// `#rrggbb`, so it can go into a stylesheet as it is.
fn check_color(color: &str) -> Result<()> {
    match color.strip_prefix('#') {
        Some(digits) if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(())
        }
        _ => Err(ProjectError::Invalid(format!(
            "track colour must look like #rrggbb: {color}"
        ))),
    }
}

fn check_channel(channel: u16) -> Result<()> {
    if channel == 0 {
        return Err(ProjectError::Invalid(
            "output channels are 1-based, 0 is not valid".into(),
        ));
    }
    Ok(())
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
                    gain_db: Some(-6.0),
                    color: Some("#5cd0ea".into()),
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
                    gain_db: None,
                    color: None,
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
                audio: Some(AudioOutput {
                    device_name: "Main Interface".into(),
                    channels: vec![1, 2],
                }),
                ltc: vec![LtcOutput {
                    device_name: "Main Interface".into(),
                    channel: 3,
                }],
                mtc: vec![MtcOutput {
                    port_name: "IAC Driver Bus 1".into(),
                }],
            },
        }
    }

    const FIXTURE: &str = r##"{
        "formatVersion": 2,
        "name": "Opening night",
        "timecode": { "frameRate": "fps2997Drop", "start": 107892 },
        "assets": [{ "id": "a1", "name": "Click", "file": "audio/click.wav" }],
        "tracks": [
            {
                "id": "t1", "name": "Backing", "muted": false, "gainDb": -6.0, "color": "#5cd0ea",
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
            "audio": { "deviceName": "Main Interface", "channels": [1, 2] },
            "ltc": [{ "deviceName": "Main Interface", "channel": 3 }],
            "mtc": [{ "portName": "IAC Driver Bus 1" }]
        }
    }"##;

    /// The same show as `FIXTURE`, written by a build that held one LTC and one MTC output.
    fn v1_fixture() -> serde_json::Value {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["formatVersion"] = 1.into();
        json["output"]["ltc"] = serde_json::json!({ "deviceName": "Main Interface", "channel": 3 });
        json["output"]["mtc"] = serde_json::json!({ "portName": "IAC Driver Bus 1" });
        json
    }

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
    fn migrate_should_turn_the_v1_outputs_into_lists() {
        let mut json = v1_fixture();
        migrate(&mut json);
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, sample_project());
    }

    #[test]
    fn migrate_should_turn_a_missing_or_null_output_into_an_empty_list() {
        let mut json = v1_fixture();
        json["output"]["ltc"] = serde_json::Value::Null;
        json["output"].as_object_mut().unwrap().remove("mtc");
        migrate(&mut json);
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert!(parsed.output.ltc.is_empty());
        assert!(parsed.output.mtc.is_empty());
        assert_eq!(parsed.format_version, FORMAT_VERSION);
    }

    #[test]
    fn a_v2_file_with_the_old_output_shape_should_still_open() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["output"]["ltc"] = serde_json::Value::Null;
        json["output"]["mtc"] = serde_json::json!({ "portName": "loopMIDI Port" });
        migrate(&mut json);
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert!(parsed.output.ltc.is_empty());
        assert_eq!(
            parsed.output.mtc,
            vec![MtcOutput {
                port_name: "loopMIDI Port".into()
            }]
        );
    }

    #[test]
    fn outputs_should_parse_from_null_one_object_or_a_list_without_migrating() {
        for (ltc, expected) in [
            (serde_json::Value::Null, 0),
            (serde_json::json!({ "deviceName": "A", "channel": 1 }), 1),
            (
                serde_json::json!([{ "deviceName": "A", "channel": 1 }, { "deviceName": "B", "channel": 2 }]),
                2,
            ),
        ] {
            let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
            json["output"]["ltc"] = ltc;
            let parsed: Project = serde_json::from_value(json).unwrap();
            assert_eq!(parsed.output.ltc.len(), expected);
        }
    }

    #[test]
    fn migrate_should_leave_a_current_file_alone() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let before = json.clone();
        migrate(&mut json);
        assert_eq!(json, before);
    }

    #[test]
    fn output_lists_should_default_to_empty_when_absent() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["output"] = serde_json::json!({});
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert!(parsed.output.ltc.is_empty() && parsed.output.mtc.is_empty());
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
    fn project_without_audio_output_should_still_parse() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["output"].as_object_mut().unwrap().remove("audio");
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.output.audio, None);
    }

    #[test]
    fn track_without_a_gain_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(parsed.tracks[1].gain_db, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(json["tracks"][1].get("gainDb").is_none());
    }

    #[test]
    fn validate_should_reject_a_track_gain_that_is_not_finite() {
        let mut project = sample_project();
        project.tracks[0].gain_db = Some(f32::NAN);
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn track_without_a_colour_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(parsed.tracks[1].color, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(json["tracks"][1].get("color").is_none());
        assert_eq!(json["tracks"][0]["color"], "#5cd0ea");
    }

    #[test]
    fn validate_should_reject_a_track_colour_that_is_not_a_hex_triplet() {
        for bad in ["red", "#fff", "5cd0ea", "#5cd0eg", "#5cd0ea; x", ""] {
            let mut project = sample_project();
            project.tracks[0].color = Some(bad.into());
            assert!(
                matches!(project.validate(), Err(ProjectError::Invalid(_))),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn validate_should_reject_channel_zero() {
        let mut project = sample_project();
        project.output.ltc[0].channel = 0;
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));

        let mut project = sample_project();
        project.output.audio.as_mut().unwrap().channels = vec![0, 1];
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_repeated_audio_channel() {
        let mut project = sample_project();
        project.output.audio.as_mut().unwrap().channels = vec![1, 1];
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
