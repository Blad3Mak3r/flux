use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::error::{ProjectError, Result};

/// Version written to `project.json`. Readers reject anything newer and migrate older ones
/// (see `migrate.rs`).
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
    Audio {
        clips: Vec<AudioClip>,
    },
    /// Text for the operator, shown on the timeline and in the Live view.
    Notes {
        notes: Vec<Note>,
    },
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

/// Devices are machine-specific, so only a name hint is stored, never an OS identifier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    /// Where the project's audio clips are played.
    pub audio: Option<AudioOutput>,
    pub ltc: Option<LtcOutput>,
    pub mtc: Option<MtcOutput>,
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
                        ensure_unique(&mut item_ids, "clip/note id", &clip.id)?;
                        if !asset_ids.contains(clip.asset.as_str()) {
                            return Err(ProjectError::Invalid(format!(
                                "clip {} references unknown asset {}",
                                clip.id, clip.asset
                            )));
                        }
                    }
                }
                TrackKind::Notes { notes } => {
                    for note in notes {
                        ensure_unique(&mut item_ids, "clip/note id", &note.id)?;
                        if note.length == Some(0) {
                            return Err(ProjectError::Invalid(format!(
                                "note {} lasts no time; leave the length out instead",
                                note.id
                            )));
                        }
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
        if let Some(ltc) = &self.ltc {
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
                    name: "Notes".into(),
                    muted: true,
                    gain_db: None,
                    color: None,
                    kind: TrackKind::Notes {
                        notes: vec![
                            Note {
                                id: "n1".into(),
                                at: 300,
                                length: None,
                                text: "Lights up".into(),
                            },
                            Note {
                                id: "n2".into(),
                                at: 450,
                                length: Some(75),
                                text: "Singer walks on\nstage left".into(),
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

    /// The shape of the current format: what `sample_project` must serialise to and parse from.
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
                "id": "t2", "name": "Notes", "muted": true,
                "kind": { "type": "notes", "notes": [
                    { "id": "n1", "at": 300, "text": "Lights up" },
                    { "id": "n2", "at": 450, "length": 75, "text": "Singer walks on\nstage left" }
                ] }
            }
        ],
        "output": {
            "audio": { "deviceName": "Main Interface", "channels": [1, 2] },
            "ltc": { "deviceName": "Main Interface", "channel": 3 },
            "mtc": { "portName": "IAC Driver Bus 1" }
        }
    }"##;

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
    fn note_without_a_length_should_parse_and_serialize_without_one() {
        let parsed: Project = serde_json::from_str(FIXTURE).unwrap();
        let TrackKind::Notes { notes } = &parsed.tracks[1].kind else {
            panic!("the second track holds notes");
        };
        assert_eq!(notes[0].length, None);
        let json = serde_json::to_value(&parsed).unwrap();
        assert!(
            json["tracks"][1]["kind"]["notes"][0]
                .get("length")
                .is_none()
        );
    }

    #[test]
    fn validate_should_reject_a_note_that_lasts_no_time() {
        let mut project = sample_project();
        if let TrackKind::Notes { notes } = &mut project.tracks[1].kind {
            notes[0].length = Some(0);
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_a_note_id_that_a_clip_already_uses() {
        let mut project = sample_project();
        if let TrackKind::Notes { notes } = &mut project.tracks[1].kind {
            notes[0].id = "c1".into();
        }
        assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
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
        project.output.ltc.as_mut().unwrap().channel = 0;
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
