//! What a spoken order is: its voice, its recording and its view.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

pub const VOICE_MODEL: &str = "eleven_multilingual_v2";
pub const MAX_DIALOGUE: usize = 5_000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingOrigin {
    #[default]
    Generated,
    Imported,
    Recorded,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceMode {
    #[default]
    Generated,
    Manual,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Alignment {
    #[default]
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedVoice {
    #[serde(default)]
    pub origin: RecordingOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesis_text: Option<String>,
    pub asset: String,
    pub dialogue: String,
    pub voice: String,
    pub model: String,
    /// Decoded frames at ENGINE_RATE, not the provider's compressed duration.
    pub samples: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceOrder {
    #[serde(default)]
    pub mode: VoiceMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesis_text: Option<String>,
    #[serde(default)]
    pub trim_start: u64,
    #[serde(default)]
    pub trim_end: u64,
    pub id: String,
    pub at: u64,
    pub dialogue: String,
    #[serde(default)]
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<GeneratedVoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceStatus {
    Ungenerated,
    Synced,
    Stale,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceView {
    pub id: String,
    pub voice: Option<String>,
    pub source: &'static str,
    pub status: VoiceStatus,
    pub before_origin: bool,
    pub start: Option<f64>,
    pub length: Option<f64>,
    /// Exact source offset for drawing a trimmed recording, without millisecond rounding.
    pub audio_offset_seconds: f64,
    pub trim_start_ms: String,
    pub trim_end_ms: String,
}

pub fn validate_voice(voice: Option<&str>) -> Result<()> {
    if voice.is_some_and(|v| {
        v.is_empty()
            || v.len() > 128
            || !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }) {
        return Err(ProjectError::Invalid("invalid voice id".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator::support::*;
    use crate::operator::*;
    use crate::{FrameRate, Session, TrackKind};

    #[test]
    fn version_eight_recordings_default_to_generated_untrimmed_without_override() {
        let mut json = serde_json::to_value(order()).unwrap();
        for field in ["synthesisText", "trimStart", "trimEnd"] {
            json.as_object_mut().unwrap().remove(field);
        }
        let g = json["generated"].as_object_mut().unwrap();
        g.remove("origin");
        g.remove("synthesisText");
        let old: VoiceOrder = serde_json::from_value(json).unwrap();
        assert_eq!(old, order());
    }

    #[test]
    fn archive_and_recovery_keep_generated_audio_and_pending_orders() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("voice.mp3");
        std::fs::write(&audio, b"test audio bytes").unwrap();
        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        let active = project.timelines[0].id.clone();
        let asset = session.import_embedded(&audio).unwrap();
        let mut ready = order();
        ready.generated.as_mut().unwrap().asset = asset.id.clone();
        let pending = VoiceOrder {
            mode: Default::default(),
            synthesis_text: None,
            trim_start: 0,
            trim_end: 0,
            id: "pending".into(),
            generated: None,
            ..ready.clone()
        };
        project.assets.push(asset.clone());
        apply(
            &mut project,
            &active,
            OperatorOp::AddTrack {
                name: "Operator".into(),
            },
        )
        .unwrap();
        if let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind {
            *orders = vec![ready, pending];
        }
        session.update(project.clone());
        assert!(project.used_assets().contains(asset.id.as_str()));
        let recovery = session.recovery().unwrap();
        recovery.validate().unwrap();
        let archive = dir.path().join("show.fxtc");
        session.save(&project, &archive).unwrap();
        std::fs::remove_file(audio).unwrap();
        let mut reopened = Session::default();
        let opened = reopened.open(&archive).unwrap();
        assert_eq!(opened.timelines, project.timelines);
        assert_eq!(reopened.asset_bytes(&asset).unwrap(), b"test audio bytes");
    }

    #[test]
    fn old_generations_remain_readable_after_save_prune_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let voice = dir.path().join("voice.mp3");
        std::fs::write(&voice, b"old generation").unwrap();
        let mut session = Session::default();
        let mut original = session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_embedded(&voice).unwrap();
        original.assets.push(asset.clone());
        session.update(original.clone());
        let path = dir.path().join("show.fxtc");
        session.save(&original, &path).unwrap();
        std::fs::remove_file(voice).unwrap();
        session.open(&path).unwrap();
        session.retain_voice_asset(&asset, dir.path()).unwrap();
        let mut pruned = original.clone();
        pruned.assets.clear();
        session.save(&pruned, &path).unwrap();
        session.sync(original).unwrap();
        assert_eq!(session.asset_bytes(&asset).unwrap(), b"old generation");
        session.recovery().unwrap().validate().unwrap();
    }

    #[test]
    fn previous_format_recoveries_are_migrated_without_rewriting_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        let recovery = session.recovery().unwrap();
        let mut value = serde_json::to_value(&recovery).unwrap();
        value["document"]["project"]["formatVersion"] = 7.into();
        value["document"]["baseline"]["formatVersion"] = 7.into();
        let path = dir.path().join(format!("{}.json", recovery.id));
        let bytes = serde_json::to_vec(&value).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let reopened = crate::recovery::read(dir.path(), &recovery.id).unwrap();
        assert_eq!(
            reopened.document.project.format_version,
            crate::FORMAT_VERSION
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
