//! The output configuration: audio, metronome, LTC and MTC.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::*;
use crate::error::{ProjectError, Result};
use crate::events::OscDestination;
use crate::serde_util::is_zero;

/// Devices are machine-specific, so only a name hint is stored, never an OS identifier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_routes: Vec<crate::routing::ResolvedAudioBus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub track_routes: Vec<crate::routing::TrackRoute>,
    /// The audio API every audio device below is opened through (`"ALSA"`, `"PipeWire"`, `"JACK"`,
    /// `"WASAPI"`, `"CoreAudio"`, ...). Absent is the system's default one, which is also what plays
    /// when this machine does not have the named driver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// Where the project's audio clips are played.
    pub audio: Option<AudioOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<AudioOutput>,
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

impl OutputConfig {
    /// Channels are 1-based and a programme channel may be listed only once.
    pub fn validate(&self) -> Result<()> {
        if let Some(route) = &self.operator {
            if route.channels.is_empty() {
                return Err(ProjectError::Invalid("select an operator channel".into()));
            }
            validate_channels(&route.channels)?;
        }
        for bus in &self.audio_routes {
            if let Some(route) = &bus.route {
                if !(1..=2).contains(&route.channels.len()) {
                    return Err(ProjectError::Invalid(
                        "choose mono or stereo channels".into(),
                    ));
                }
                validate_channels(&route.channels)?;
            }
        }
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
pub(super) fn validate_channels(channels: &[u16]) -> Result<()> {
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
pub(super) fn list_or_one<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
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

pub(super) fn check_channel(channel: u16) -> Result<()> {
    if channel == 0 {
        return Err(ProjectError::Invalid(
            "output channels are 1-based, 0 is not valid".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProjectError;

    use crate::model::tests::*;

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
}
