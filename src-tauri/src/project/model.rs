use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ProjectError, Result};
pub use super::meta::Meta;

/// Version written to `project.json`. Readers reject anything newer and migrate older ones
/// (see `migrate.rs`).
///
/// Version 2 turned event tracks into notes and `output.ltc` / `output.mtc` into lists; version 3
/// added `meta`; version 4 moved `timecode` and `tracks` into `timelines`. Files of version 2
/// written while the format was changing can hold either shape, so the migration looks at the
/// shape, not at the number.
pub const FORMAT_VERSION: u32 = 4;

/// Folder inside the archive that holds the audio files (flat, no subfolders).
pub const AUDIO_DIR: &str = "audio/";

/// Contents of `project.json`. The audio and the output devices belong to the project; each
/// timeline has its own timecode and tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub meta: Meta,
    pub assets: Vec<AudioAsset>,
    pub output: OutputConfig,
    /// Never empty. Only one plays at a time.
    pub timelines: Vec<Timeline>,
}

/// One line of time of a project, e.g. a song. All positions in it are frame counts at its own
/// frame rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
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
    fn validate(&self) -> Result<()> {
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
    /// The tempo found in the audio, or set by the user; absent when it was never detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
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
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl AudioClip {
    /// A clip that plays the whole asset from `start`, at 0 dB.
    pub fn new(id: impl Into<String>, asset: impl Into<String>, start: u64) -> Self {
        Self {
            id: id.into(),
            asset: asset.into(),
            start,
            gain_db: 0.0,
            offset: 0,
            length: None,
            fade_in: 0,
            fade_out: 0,
        }
    }

    /// How many frames of an asset of `asset_frames` the clip plays: its length, but never past
    /// the end of the asset.
    pub fn played_frames(&self, asset_frames: u64) -> u64 {
        let rest = asset_frames.saturating_sub(self.offset);
        self.length.map_or(rest, |length| length.min(rest))
    }
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
    /// Where the Edit view auditions the clips, when it should not use `audio`. Edit then sends
    /// no timecode. Absent means Edit plays like Live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<AudioOutput>,
    /// Where the metronome clicks, on a device and level of its own. Absent means no click plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metronome: Option<MetronomeOutput>,
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

/// The audio interface and channels the metronome plays on, apart from the programme's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetronomeOutput {
    pub device_name: String,
    /// 1-based interface channels: a pair plays the click on both, a single one on that channel.
    pub channels: Vec<u16>,
    /// Level of the click in dB; absent is 0 dB.
    #[serde(default)]
    pub gain_db: f32,
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
    /// A project with one timeline, named like the project.
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        let name = name.into();
        Self {
            format_version: FORMAT_VERSION,
            meta: Meta::new(),
            assets: Vec::new(),
            output: OutputConfig::default(),
            timelines: vec![Timeline::new(name.clone(), frame_rate)],
            name,
        }
    }

    pub fn timeline(&self, id: &str) -> Option<&Timeline> {
        self.timelines.iter().find(|timeline| timeline.id == id)
    }

    /// Ids of the assets some clip of any timeline plays.
    pub fn used_assets(&self) -> HashSet<&str> {
        self.timelines
            .iter()
            .flat_map(|timeline| &timeline.tracks)
            .filter_map(|track| match &track.kind {
                TrackKind::Audio { clips } => Some(clips),
                TrackKind::Notes { .. } => None,
            })
            .flatten()
            .map(|clip| clip.asset.as_str())
            .collect()
    }

    /// Checks the invariants the archive format relies on: at least one timeline, unique ids (a
    /// track, clip or note id is unique in the whole project), clips pointing at known assets, and
    /// asset files living directly under `audio/`.
    pub fn validate(&self) -> Result<()> {
        self.meta.validate()?;
        let mut asset_ids = HashSet::new();
        let mut asset_files = HashSet::new();
        for asset in &self.assets {
            check_asset_path(&asset.file)?;
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

        if self.timelines.is_empty() {
            return Err(ProjectError::Invalid("a project needs a timeline".into()));
        }
        let mut timeline_ids = HashSet::new();
        let mut track_ids = HashSet::new();
        let mut item_ids = HashSet::new();
        for timeline in &self.timelines {
            ensure_unique(&mut timeline_ids, "timeline id", &timeline.id)?;
            timeline.tempo.validate()?;
            for track in &timeline.tracks {
                validate_track(track, &asset_ids, &mut track_ids, &mut item_ids)?;
            }
        }
        self.output.validate()
    }
}

fn validate_track<'a>(
    track: &'a Track,
    asset_ids: &HashSet<&str>,
    track_ids: &mut HashSet<&'a str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    ensure_unique(track_ids, "track id", &track.id)?;
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
                ensure_unique(item_ids, "clip/note id", &clip.id)?;
                if clip.length == Some(0) {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} plays no time; leave the length out instead",
                        clip.id
                    )));
                }
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
                ensure_unique(item_ids, "clip/note id", &note.id)?;
                if note.length == Some(0) {
                    return Err(ProjectError::Invalid(format!(
                        "note {} lasts no time; leave the length out instead",
                        note.id
                    )));
                }
            }
        }
    }
    Ok(())
}

impl OutputConfig {
    /// Channels are 1-based and a programme channel may be listed only once.
    fn validate(&self) -> Result<()> {
        for ltc in &self.ltc {
            check_channel(ltc.channel)?;
        }
        if let Some(metronome) = &self.metronome {
            if !metronome.gain_db.is_finite() {
                return Err(ProjectError::Invalid(
                    "the metronome level is not a number".into(),
                ));
            }
            validate_channels(&metronome.channels)?;
        }
        for audio in [&self.audio, &self.preview].into_iter().flatten() {
            validate_channels(&audio.channels)?;
        }
        Ok(())
    }
}

/// Channels are 1-based and may be listed only once.
fn validate_channels(channels: &[u16]) -> Result<()> {
    let mut seen = HashSet::new();
    for &channel in channels {
        check_channel(channel)?;
        if !seen.insert(channel) {
            return Err(ProjectError::Invalid(format!(
                "duplicate audio output channel: {channel}"
            )));
        }
    }
    Ok(())
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
            meta: Meta {
                id: "6f0c1d52-7a0e-4a52-9c3b-2d6a5f1b8e10".into(),
                created_at: "2026-11-01T09:30:00Z".into(),
                modified_at: "2026-11-05T14:29:03Z".into(),
                created_with: "0.1.0".into(),
                saved_with: "0.1.0".into(),
                author: "Ana".into(),
                description: String::new(),
            },
            assets: vec![AudioAsset {
                id: "a1".into(),
                name: "Click".into(),
                file: "audio/click.wav".into(),
                bpm: Some(128.5),
            }],
            timelines: vec![Timeline {
                id: "l1".into(),
                name: "Opening night".into(),
                timecode: TimecodeSettings {
                    frame_rate: FrameRate::Fps2997Drop,
                    start: 107_892,
                },
                tempo: Tempo {
                    bpm: 128.0,
                    beats_per_bar: 3,
                    click: true,
                },
                tracks: vec![
                    Track {
                        id: "t1".into(),
                        name: "Backing".into(),
                        muted: false,
                        gain_db: Some(-6.0),
                        color: Some("#5cd0ea".into()),
                        kind: TrackKind::Audio {
                            clips: vec![AudioClip {
                                gain_db: -3.5,
                                ..AudioClip::new("c1", "a1", 120)
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
            }],
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
                preview: None,
                metronome: Some(MetronomeOutput {
                    device_name: "Band Monitor".into(),
                    channels: vec![3, 4],
                    gain_db: -6.0,
                }),
            },
        }
    }

    /// The shape of the current format: what `sample_project` must serialise to and parse from.
    const FIXTURE: &str = r##"{
        "formatVersion": 4,
        "name": "Opening night",
        "meta": {
            "id": "6f0c1d52-7a0e-4a52-9c3b-2d6a5f1b8e10",
            "createdAt": "2026-11-01T09:30:00Z",
            "modifiedAt": "2026-11-05T14:29:03Z",
            "createdWith": "0.1.0",
            "savedWith": "0.1.0",
            "author": "Ana"
        },
        "assets": [{ "id": "a1", "name": "Click", "file": "audio/click.wav", "bpm": 128.5 }],
        "timelines": [{
            "id": "l1", "name": "Opening night",
            "timecode": { "frameRate": "fps2997Drop", "start": 107892 },
            "tempo": { "bpm": 128.0, "beatsPerBar": 3, "click": true },
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
        ] }],
        "output": {
            "audio": { "deviceName": "Main Interface", "channels": [1, 2] },
            "metronome": { "deviceName": "Band Monitor", "channels": [3, 4], "gainDb": -6.0 },
            "ltc": [{ "deviceName": "Main Interface", "channel": 3 }],
            "mtc": [{ "portName": "IAC Driver Bus 1" }]
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
    fn validate_should_reject_a_clip_that_plays_no_time() {
        let mut project = sample_project();
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        clips[0].length = Some(0);
        assert!(project.validate().is_err());
    }

    #[test]
    fn outputs_should_parse_from_null_one_object_or_a_list() {
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
    fn output_lists_should_default_to_empty_when_absent() {
        let mut json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        json["output"] = serde_json::json!({});
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert!(parsed.output.ltc.is_empty() && parsed.output.mtc.is_empty());
    }

    #[test]
    fn preview_should_default_to_none_and_stay_out_of_the_file_when_unset() {
        let json: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert!(parsed.output.preview.is_none());
        let written = serde_json::to_value(&parsed).unwrap();
        assert!(written["output"].get("preview").is_none());
    }

    #[test]
    fn validate_should_reject_bad_preview_channels() {
        for channels in [vec![0], vec![1, 1]] {
            let mut project = sample_project();
            project.output.preview = Some(AudioOutput {
                device_name: "Headphones".into(),
                channels,
            });
            assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
        }
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
        json["output"].as_object_mut().unwrap().remove("audio");
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.output.audio, None);
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

    /// The sample project with a second timeline that holds one note.
    fn two_timelines() -> Project {
        let mut project = sample_project();
        let mut second = Timeline::new("Second", FrameRate::Fps25);
        second.tracks.push(Track {
            id: "t3".into(),
            name: "Notes".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Notes {
                notes: vec![Note {
                    id: "n3".into(),
                    at: 0,
                    length: None,
                    text: String::new(),
                }],
            },
        });
        project.timelines.push(second);
        project
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
    fn metronome_and_asset_bpm_should_stay_out_of_the_file_when_unset() {
        let mut project = sample_project();
        project.output.metronome = None;
        project.assets[0].bpm = None;
        let written = serde_json::to_value(&project).unwrap();
        assert!(written["output"].get("metronome").is_none());
        assert!(written["assets"][0].get("bpm").is_none());
        let parsed: Project = serde_json::from_value(written).unwrap();
        assert_eq!(parsed, project);
    }

    #[test]
    fn validate_should_reject_a_tempo_out_of_range() {
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

    #[test]
    fn validate_should_reject_an_asset_tempo_out_of_range() {
        for bpm in [0.0, 5.0, 1_000.0, f64::INFINITY] {
            let mut project = sample_project();
            project.assets[0].bpm = Some(bpm);
            assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
        }
    }

    #[test]
    fn validate_should_reject_bad_metronome_outputs() {
        for (channels, gain) in [(vec![0], 0.0), (vec![1, 1], 0.0), (vec![1], f32::NAN)] {
            let mut project = sample_project();
            project.output.metronome = Some(MetronomeOutput {
                device_name: "Band Monitor".into(),
                channels,
                gain_db: gain,
            });
            assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
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
