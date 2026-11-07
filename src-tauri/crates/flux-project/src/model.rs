use std::collections::HashSet;

use flux_core::FrameRate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ProjectError, Result};
use super::events::{MidiEvent, MidiPortRef, OscDestination, OscEvent};
pub use super::meta::Meta;

/// Version written to `project.json`. Readers reject anything newer and migrate older ones
/// (see `migrate.rs`).
///
/// Version 2 turned event tracks into notes and `output.ltc` / `output.mtc` into lists; version 3
/// added `meta`; version 4 moved `timecode` and `tracks` into `timelines`; version 5 added the `midi`
/// and `osc` track kinds and `output.osc` (nothing was reshaped, but an older build cannot read them). Files of version 2
/// written while the format was changing can hold either shape, so the migration looks at the
/// shape, not at the number.
/// Version 6 separates portable signals from local hardware routing.
/// Version 7 adds project-owned autosave preferences.
pub const FORMAT_VERSION: u32 = 7;

/// Folder inside the archive that holds the audio files (flat, no subfolders).
pub const AUDIO_DIR: &str = "audio/";

/// Contents of `project.json`. Audio and logical signals belong to the project; each
/// timeline has its own timecode and tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub meta: Meta,
    #[serde(default)]
    pub autosave: Autosave,
    pub assets: Vec<AudioAsset>,
    pub output: crate::ShowOutputs,
    /// Never empty. Only one plays at a time.
    pub timelines: Vec<Timeline>,
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
}

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

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
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
    /// The audio API every audio device below is opened through (`"ALSA"`, `"PipeWire"`, `"JACK"`,
    /// `"WASAPI"`, `"CoreAudio"`, ...). Absent is the system's default one, which is also what plays
    /// when this machine does not have the named driver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
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
    /// The OSC receivers the project's OSC tracks send to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub osc: Vec<OscDestination>,
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
    /// Peak level of the signal, linear from 0 to 1; absent is [`DEFAULT_LTC_LEVEL`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    /// Latency compensation in ms, positive = emitted earlier; absent is 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_ms: Option<i32>,
}

/// The largest latency offset of an output, in ms either way.
pub const MAX_OFFSET_MS: i32 = 1000;

/// The most characters a clip's own name can have.
pub const MAX_CLIP_NAME: usize = 64;

/// The LTC level of an output that does not set one: half scale, -6 dBFS.
pub const DEFAULT_LTC_LEVEL: f32 = 0.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MtcOutput {
    pub port_name: String,
    /// Which of the ports named `port_name` this is, counting from 0 in the order the system lists
    /// them. Tells apart two interfaces of the same model.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub port_index: usize,
    /// Latency compensation in ms, positive = emitted earlier; absent is 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_ms: Option<i32>,
}

impl Project {
    /// A project with one timeline, named like the project.
    pub fn new(name: impl Into<String>, frame_rate: FrameRate) -> Self {
        let name = name.into();
        Self {
            format_version: FORMAT_VERSION,
            meta: Meta::new(),
            autosave: Autosave::default(),
            assets: Vec::new(),
            output: crate::ShowOutputs::default(),
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
                _ => None,
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
        self.autosave.validate()?;
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
            for track in &timeline.tracks {
                validate_track(
                    track,
                    &asset_ids,
                    &destination_ids,
                    &mut track_ids,
                    &mut item_ids,
                )?;
            }
        }
        self.output.validate()
    }
}

fn validate_track<'a>(
    track: &'a Track,
    asset_ids: &HashSet<&str>,
    destination_ids: &HashSet<&str>,
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
                if let Some(name) = &clip.name
                    && (name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME)
                {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} needs a name of 1 to {MAX_CLIP_NAME} characters; leave it out instead",
                        clip.id
                    )));
                }
                check_envelope(clip)?;
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
        TrackKind::Midi { events, .. } => {
            for event in events {
                ensure_unique(item_ids, "clip/note id", &event.id)?;
                event.validate()?;
            }
        }
        TrackKind::Osc {
            destination,
            events,
        } => {
            if let Some(id) = destination
                && !destination_ids.contains(id.as_str())
            {
                return Err(ProjectError::Invalid(format!(
                    "track {} sends to unknown OSC destination {id}",
                    track.id
                )));
            }
            for event in events {
                ensure_unique(item_ids, "clip/note id", &event.id)?;
                event.validate()?;
            }
        }
    }
    Ok(())
}

/// An envelope has at most [`MAX_ENVELOPE_POINTS`] points, in order, each within the range of gains.
fn check_envelope(clip: &AudioClip) -> Result<()> {
    let invalid =
        |why: &str| ProjectError::Invalid(format!("the envelope of clip {} {why}", clip.id));
    if clip.envelope.len() > MAX_ENVELOPE_POINTS {
        return Err(invalid(&format!(
            "has more than {MAX_ENVELOPE_POINTS} points"
        )));
    }
    if clip
        .envelope
        .windows(2)
        .any(|pair| pair[0].at >= pair[1].at)
    {
        return Err(invalid(
            "must have its points in order, none at the same place",
        ));
    }
    if clip
        .envelope
        .iter()
        .any(|point| !(ENVELOPE_MIN_DB..=ENVELOPE_MAX_DB).contains(&point.gain_db))
    {
        return Err(invalid(&format!(
            "has a gain outside {ENVELOPE_MIN_DB} to {ENVELOPE_MAX_DB} dB"
        )));
    }
    Ok(())
}

fn check_offset(offset_ms: Option<i32>) -> Result<()> {
    if offset_ms.is_some_and(|ms| ms.abs() > MAX_OFFSET_MS) {
        return Err(ProjectError::Invalid(format!(
            "the offset must be within {MAX_OFFSET_MS} ms either way"
        )));
    }
    Ok(())
}

impl OutputConfig {
    /// Channels are 1-based and a programme channel may be listed only once.
    pub fn validate(&self) -> Result<()> {
        for ltc in &self.ltc {
            check_channel(ltc.channel)?;
            if !ltc.level.is_none_or(|level| (0.0..=1.0).contains(&level)) {
                return Err(ProjectError::Invalid(
                    "the LTC level must be between 0 and 1".into(),
                ));
            }
            check_offset(ltc.offset_ms)?;
        }
        for mtc in &self.mtc {
            check_offset(mtc.offset_ms)?;
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
    use crate::{MidiMessage, MscCommand, OscArg};

    pub fn sample_project() -> Project {
        Project {
            autosave: Autosave::default(),
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
                    Track {
                        id: "t3".into(),
                        name: "Console".into(),
                        muted: false,
                        gain_db: None,
                        color: None,
                        kind: TrackKind::Midi {
                            port: Some(MidiPortRef {
                                port_name: "USB MIDI".into(),
                                port_index: 0,
                            }),
                            events: vec![
                                MidiEvent {
                                    id: "m1".into(),
                                    at: 600,
                                    message: MidiMessage::NoteOn {
                                        channel: 1,
                                        note: 60,
                                        velocity: 100,
                                        length: Some(25),
                                    },
                                },
                                MidiEvent {
                                    id: "m2".into(),
                                    at: 700,
                                    message: MidiMessage::Msc {
                                        device_id: 127,
                                        format: 1,
                                        command: MscCommand::Go,
                                        cue: Some("12.5".into()),
                                        list: None,
                                    },
                                },
                            ],
                        },
                    },
                    Track {
                        id: "t4".into(),
                        name: "QLab".into(),
                        muted: false,
                        gain_db: None,
                        color: None,
                        kind: TrackKind::Osc {
                            destination: Some("d1".into()),
                            events: vec![OscEvent {
                                id: "o1".into(),
                                at: 800,
                                address: "/cue/1/start".into(),
                                args: vec![OscArg::Int(1), OscArg::Float(0.5), OscArg::Bool(true)],
                            }],
                        },
                    },
                ],
            }],
            output: OutputConfig {
                driver: Some("PipeWire".into()),
                audio: Some(AudioOutput {
                    device_name: "Main Interface".into(),
                    channels: vec![1, 2],
                }),
                ltc: vec![LtcOutput {
                    device_name: "Main Interface".into(),
                    channel: 3,
                    level: Some(0.25),
                    offset_ms: Some(-12),
                }],
                mtc: vec![MtcOutput {
                    port_name: "IAC Driver Bus 1".into(),
                    port_index: 1,
                    offset_ms: Some(30),
                }],
                osc: vec![OscDestination {
                    id: "d1".into(),
                    name: "QLab".into(),
                    host: "192.168.1.20".into(),
                    port: 53000,
                }],
                preview: None,
                metronome: Some(MetronomeOutput {
                    device_name: "Band Monitor".into(),
                    channels: vec![3, 4],
                    gain_db: -6.0,
                }),
            }
            .into(),
        }
    }

    /// The shape of the current format: what `sample_project` must serialise to and parse from.
    const FIXTURE: &str = r##"{
        "formatVersion": 7,
        "autosave": { "enabled": true, "intervalSeconds": 30 },
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
            },
            {
                "id": "t3", "name": "Console", "muted": false,
                "kind": { "type": "midi", "port": { "portName": "USB MIDI" }, "events": [
                    { "id": "m1", "at": 600, "message": { "type": "noteOn", "channel": 1, "note": 60, "velocity": 100, "length": 25 } },
                    { "id": "m2", "at": 700, "message": { "type": "msc", "deviceId": 127, "format": 1, "command": "go", "cue": "12.5" } }
                ] }
            },
            {
                "id": "t4", "name": "QLab", "muted": false,
                "kind": { "type": "osc", "destination": "d1", "events": [
                    { "id": "o1", "at": 800, "address": "/cue/1/start", "args": [
                        { "type": "int", "value": 1 }, { "type": "float", "value": 0.5 }, { "type": "bool", "value": true }
                    ] }
                ] }
            }
        ] }],
        "output": {
            "metronome": { "gainDb": -6.0 },
            "ltc": [{ "slot": 1, "level": 0.25 }],
            "mtc": [{ "slot": 1 }],
            "osc": [{"id": "d1", "name": "QLab", "host": "192.168.1.20", "port": 53000}],
            "legacyRoutes": {
                "driver": "PipeWire",
                "audio": { "deviceName": "Main Interface", "channels": [1, 2] },
                "metronome": { "deviceName": "Band Monitor", "channels": [3, 4], "gainDb": -6.0 },
                "ltc": [{ "deviceName": "Main Interface", "channel": 3, "level": 0.25, "offsetMs": -12 }],
                "mtc": [{ "portName": "IAC Driver Bus 1", "portIndex": 1, "offsetMs": 30 }],
                "osc": [{ "id": "d1", "name": "QLab", "host": "192.168.1.20", "port": 53000 }]
            }
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

    fn envelope_clip(points: &[(u64, f32)]) -> AudioClip {
        AudioClip {
            envelope: points
                .iter()
                .map(|&(at, gain_db)| EnvPoint {
                    at,
                    gain_db,
                    curve: EnvCurve::Linear,
                })
                .collect(),
            ..AudioClip::new("c1", "a1", 0)
        }
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

    fn project_with(clip: AudioClip) -> Project {
        let mut project = sample_project();
        let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        clips[0] = AudioClip {
            id: clips[0].id.clone(),
            asset: clips[0].asset.clone(),
            ..clip
        };
        project
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
            json["output"]["legacyRoutes"]["ltc"] = ltc;
            let parsed: OutputConfig =
                serde_json::from_value(json["output"]["legacyRoutes"].clone()).unwrap();
            assert_eq!(parsed.ltc.len(), expected);
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
        assert!(
            parsed
                .output
                .legacy_routes
                .as_ref()
                .unwrap()
                .preview
                .is_none()
        );
        let written = serde_json::to_value(&parsed).unwrap();
        assert!(written["output"].get("preview").is_none());
    }

    #[test]
    fn validate_should_reject_bad_preview_channels() {
        for channels in [vec![0], vec![1, 1]] {
            let mut output = sample_project().output.legacy_routes.unwrap();
            output.preview = Some(AudioOutput {
                device_name: "Headphones".into(),
                channels,
            });
            assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
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
        json["output"]["legacyRoutes"]
            .as_object_mut()
            .unwrap()
            .remove("audio");
        let parsed: Project = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.output.legacy_routes.as_ref().unwrap().audio, None);
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
            id: "t9".into(),
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
            let mut output = sample_project().output.legacy_routes.unwrap();
            output.metronome = Some(MetronomeOutput {
                device_name: "Band Monitor".into(),
                channels,
                gain_db: gain,
            });
            assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
        }
    }

    #[test]
    fn validate_should_reject_channel_zero() {
        let mut output = sample_project().output.legacy_routes.unwrap();
        output.ltc[0].channel = 0;
        assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));

        let mut output = sample_project().output.legacy_routes.unwrap();
        output.audio.as_mut().unwrap().channels = vec![0, 1];
        assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn validate_should_reject_an_ltc_level_outside_0_to_1() {
        for level in [-0.1, 1.1, f32::NAN] {
            let mut project = sample_project();
            project.output.ltc[0].level = Some(level);
            assert!(matches!(project.validate(), Err(ProjectError::Invalid(_))));
        }
    }

    #[test]
    fn validate_should_reject_an_offset_beyond_the_limit() {
        for ms in [MAX_OFFSET_MS + 1, -MAX_OFFSET_MS - 1] {
            let mut output = sample_project().output.legacy_routes.unwrap();
            output.ltc[0].offset_ms = Some(ms);
            assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
            let mut output = sample_project().output.legacy_routes.unwrap();
            output.mtc[0].offset_ms = Some(ms);
            assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
        }
    }

    #[test]
    fn offset_should_be_left_out_when_unset() {
        let mut output = sample_project().output.legacy_routes.unwrap();
        output.ltc[0].offset_ms = None;
        output.mtc[0].offset_ms = None;
        let json = serde_json::to_value(&output).unwrap();
        assert!(json["ltc"][0].get("offsetMs").is_none());
        assert!(json["mtc"][0].get("offsetMs").is_none());
    }

    #[test]
    fn ltc_level_and_mtc_port_index_should_be_left_out_when_unset() {
        let mut project = sample_project();
        project.output.ltc[0].level = None;
        project.output.legacy_routes.as_mut().unwrap().mtc[0].port_index = 0;
        let json = serde_json::to_value(&project).unwrap();
        assert!(json["output"]["ltc"][0].get("level").is_none());
        assert!(json["output"]["mtc"][0].get("portIndex").is_none());
    }

    #[test]
    fn validate_should_reject_a_repeated_audio_channel() {
        let mut output = sample_project().output.legacy_routes.unwrap();
        output.audio.as_mut().unwrap().channels = vec![1, 1];
        assert!(matches!(output.validate(), Err(ProjectError::Invalid(_))));
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
