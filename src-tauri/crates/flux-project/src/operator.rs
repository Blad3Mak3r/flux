//! Spoken orders: portable generation history, inherited voices and sample-accurate placement.
use crate::{Project, ProjectError, Result, Track, TrackKind};
use flux_core::{Clock, ENGINE_RATE};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const VOICE_MODEL: &str = "eleven_multilingual_v2";
pub const MAX_DIALOGUE: usize = 5_000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecordingOrigin {
    #[default]
    Generated,
    Imported,
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

impl VoiceOrder {
    pub fn validate(&self, assets: &HashSet<&str>) -> Result<()> {
        if self.at > 24 * 60 * 60 * 30 {
            return Err(ProjectError::Invalid(
                "order position exceeds one day".into(),
            ));
        }
        if self.dialogue.chars().count() > MAX_DIALOGUE
            || self
                .synthesis_text
                .as_ref()
                .is_some_and(|s| s.trim().is_empty() || s.chars().count() > MAX_DIALOGUE)
        {
            return Err(ProjectError::Invalid("dialogue is too long".into()));
        }
        if let Some(g) = &self.generated {
            validate_voice(Some(&g.voice))?;
            if !assets.contains(g.asset.as_str())
                || g.samples == 0
                || g.samples > u64::from(ENGINE_RATE) * 3600
                || (g.origin == RecordingOrigin::Generated && g.model != VOICE_MODEL)
                || (g.origin == RecordingOrigin::Imported && !g.model.is_empty())
                || g.dialogue.chars().count() > MAX_DIALOGUE
                || g.synthesis_text
                    .as_ref()
                    .is_some_and(|s| s.chars().count() > MAX_DIALOGUE)
                || self.trim_start.saturating_add(self.trim_end) >= g.samples
            {
                return Err(ProjectError::Invalid("invalid generated voice".into()));
            }
        } else if self.trim_start != 0 || self.trim_end != 0 {
            return Err(ProjectError::Invalid(
                "cannot trim an unrecorded order".into(),
            ));
        }
        Ok(())
    }

    pub fn status(&self, voice: Option<&str>) -> VoiceStatus {
        match &self.generated {
            None => VoiceStatus::Ungenerated,
            Some(g)
                if g.dialogue == self.dialogue
                    && (g.origin == RecordingOrigin::Imported
                        || (Some(g.voice.as_str()) == voice
                            && g.model == VOICE_MODEL
                            && g.synthesis_text == self.synthesis_text)) =>
            {
                VoiceStatus::Synced
            }
            Some(_) => VoiceStatus::Stale,
        }
    }

    /// Half-open sample interval; None for an ungenerated order or one before the origin.
    pub fn interval(&self, clock: Clock) -> Option<(u64, u64)> {
        let samples = self.play_samples()?;
        let anchor = clock.frames_to_samples(self.at);
        match self.alignment {
            Alignment::Start => Some((anchor, anchor.checked_add(samples)?)),
            Alignment::End => Some((anchor.checked_sub(samples)?, anchor)),
        }
    }

    pub fn play_samples(&self) -> Option<u64> {
        self.generated
            .as_ref()?
            .samples
            .checked_sub(self.trim_start)?
            .checked_sub(self.trim_end)
            .filter(|n| *n > 0)
    }
}

pub fn effective_voice<'a>(
    project: &'a Project,
    track: &'a Track,
    local: Option<&'a str>,
) -> (Option<&'a str>, &'static str) {
    if let TrackKind::Operator { voice: Some(v), .. } = &track.kind {
        (Some(v), "track")
    } else if let Some(v) = &project.voice {
        (Some(v), "project")
    } else {
        (local, "flux")
    }
}

/// Pin inherited installation choices while leaving explicit project/track overrides intact.
pub fn freeze_voices(project: &mut Project, local: Option<&str>) -> Result<()> {
    let needs_voice = project
        .timelines
        .iter()
        .flat_map(|t| &t.tracks)
        .any(|t| matches!(&t.kind, TrackKind::Operator { voice: None, .. }));
    if needs_voice && project.voice.is_none() {
        let voice = local.ok_or_else(|| ProjectError::Invalid("missing inherited voice".into()))?;
        validate_voice(Some(voice))?;
        project.voice = Some(voice.to_owned());
    }
    Ok(())
}

pub fn views(project: &Project, local: Option<&str>) -> Vec<VoiceView> {
    let mut views = Vec::new();
    for timeline in &project.timelines {
        let clock = Clock::new(ENGINE_RATE, timeline.timecode.frame_rate);
        for track in &timeline.tracks {
            if let TrackKind::Operator { orders, .. } = &track.kind {
                let (voice, source) = effective_voice(project, track, local);
                for order in orders {
                    let interval = order.interval(clock);
                    let to_frames = |samples| {
                        samples as f64 / f64::from(ENGINE_RATE)
                            * (timeline.timecode.frame_rate.fraction().0 as f64
                                / timeline.timecode.frame_rate.fraction().1 as f64)
                    };
                    views.push(VoiceView {
                        id: order.id.clone(),
                        voice: voice.map(str::to_owned),
                        source,
                        status: order.status(voice),
                        before_origin: order.generated.is_some() && interval.is_none(),
                        start: interval.map(|(from, _)| to_frames(from)),
                        length: order.play_samples().map(to_frames),
                        trim_start_ms: (order.trim_start / (u64::from(ENGINE_RATE) / 1000))
                            .to_string(),
                        trim_end_ms: (order.trim_end / (u64::from(ENGINE_RATE) / 1000)).to_string(),
                    });
                }
            }
        }
    }
    views
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum OperatorOp {
    SetPronunciation {
        id: String,
        text: String,
    },
    SetTrim {
        id: String,
        #[serde(default)]
        start_ms: Option<String>,
        #[serde(default)]
        end_ms: Option<String>,
    },
    AddTrack {
        name: String,
    },
    AddOrder {
        track_id: String,
        at: u64,
    },
    EditOrder {
        id: String,
        #[serde(default)]
        dialogue: Option<String>,
        #[serde(default)]
        at: Option<u64>,
        #[serde(default)]
        alignment: Option<Alignment>,
    },
    MoveOrder {
        id: String,
        at: u64,
    },
    RemoveOrder {
        id: String,
    },
    DuplicateOrder {
        id: String,
    },
    SetVoice {
        track_id: Option<String>,
        voice: Option<String>,
    },
    SetGain {
        track_id: String,
        gain_db: f32,
    },
}

pub fn apply(project: &mut Project, active: &str, op: OperatorOp) -> Result<()> {
    if let OperatorOp::SetVoice {
        track_id: None,
        voice,
    } = op
    {
        validate_voice(voice.as_deref())?;
        project.voice = voice;
        return Ok(());
    }
    let timeline = project
        .timelines
        .iter_mut()
        .find(|t| t.id == active)
        .ok_or_else(|| ProjectError::Invalid("unknown timeline".into()))?;
    if let OperatorOp::AddTrack { name } = op {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > crate::MAX_TRACK_NAME {
            return Err(ProjectError::Invalid("invalid track name".into()));
        }
        timeline.tracks.push(Track {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![],
            },
        });
        return Ok(());
    }
    for track in &mut timeline.tracks {
        let TrackKind::Operator { voice, orders } = &mut track.kind else {
            continue;
        };
        match &op {
            OperatorOp::SetPronunciation { id, text } if orders.iter().any(|o| o.id == *id) => {
                if let Some(order) = orders.iter_mut().find(|o| o.id == *id) {
                    order.synthesis_text = (!text.trim().is_empty()).then(|| text.clone());
                }
            }
            OperatorOp::SetTrim {
                id,
                start_ms,
                end_ms,
            } if orders.iter().any(|o| o.id == *id) => {
                let parse = |s: &str| {
                    s.trim()
                        .parse::<u64>()
                        .ok()
                        .and_then(|n| n.checked_mul(u64::from(ENGINE_RATE) / 1000))
                        .ok_or_else(|| ProjectError::Invalid("invalid trim".into()))
                };
                if let Some(order) = orders.iter_mut().find(|o| o.id == *id) {
                    if let Some(start) = start_ms {
                        order.trim_start = parse(start)?;
                    }
                    if let Some(end) = end_ms {
                        order.trim_end = parse(end)?;
                    }
                }
            }
            OperatorOp::AddOrder { track_id, at } if track.id == *track_id => {
                orders.push(VoiceOrder {
                    synthesis_text: None,
                    trim_start: 0,
                    trim_end: 0,
                    id: Uuid::new_v4().to_string(),
                    at: *at,
                    dialogue: String::new(),
                    alignment: Alignment::Start,
                    generated: None,
                })
            }
            OperatorOp::SetVoice {
                track_id: Some(id),
                voice: new,
            } if track.id == *id => {
                validate_voice(new.as_deref())?;
                voice.clone_from(new);
            }
            OperatorOp::SetGain { track_id, gain_db } if track.id == *track_id => {
                if !gain_db.is_finite() {
                    return Err(ProjectError::Invalid("invalid gain".into()));
                }
                track.gain_db = Some(*gain_db);
            }
            OperatorOp::RemoveOrder { id } if orders.iter().any(|o| o.id == *id) => {
                orders.retain(|o| o.id != *id)
            }
            OperatorOp::DuplicateOrder { id } if orders.iter().any(|o| o.id == *id) => {
                let mut copy = orders
                    .iter()
                    .find(|o| o.id == *id)
                    .cloned()
                    .ok_or_else(|| ProjectError::Invalid("unknown order".into()))?;
                copy.id = Uuid::new_v4().to_string();
                orders.push(copy);
            }
            OperatorOp::MoveOrder { id, at } if orders.iter().any(|o| o.id == *id) => {
                if let Some(o) = orders.iter_mut().find(|o| o.id == *id) {
                    o.at = *at;
                }
            }
            OperatorOp::EditOrder {
                id,
                dialogue,
                at,
                alignment,
            } if orders.iter().any(|o| o.id == *id) => {
                if let Some(o) = orders.iter_mut().find(|o| o.id == *id) {
                    if let Some(dialogue) = dialogue {
                        o.dialogue.clone_from(dialogue);
                    }
                    if let Some(at) = at {
                        o.at = *at;
                    }
                    if let Some(alignment) = alignment {
                        o.alignment = *alignment;
                    }
                }
            }
            _ => continue,
        }
        orders.sort_by_key(|o| o.at);
        return Ok(());
    }
    Err(ProjectError::Invalid(
        "unknown operator track or order".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameRate, Session};

    fn order() -> VoiceOrder {
        VoiceOrder {
            synthesis_text: None,
            trim_start: 0,
            trim_end: 0,
            id: "order".into(),
            at: 100,
            dialogue: "Stand by lights".into(),
            alignment: Alignment::Start,
            generated: Some(GeneratedVoice {
                origin: crate::operator::RecordingOrigin::Generated,
                synthesis_text: None,
                asset: "audio".into(),
                dialogue: "Stand by lights".into(),
                voice: "voice".into(),
                model: VOICE_MODEL.into(),
                samples: 48_001,
            }),
        }
    }

    #[test]
    fn pronunciation_changes_are_reversible_and_imported_audio_ignores_voice_choices() {
        let mut o = order();
        o.synthesis_text = Some("Stand by lights, pronounced differently".into());
        assert_eq!(o.status(Some("voice")), VoiceStatus::Stale);
        o.generated
            .as_mut()
            .unwrap()
            .synthesis_text
            .clone_from(&o.synthesis_text);
        assert_eq!(o.status(Some("voice")), VoiceStatus::Synced);
        o.synthesis_text = None;
        assert_eq!(o.status(Some("voice")), VoiceStatus::Stale);
        let g = o.generated.as_mut().unwrap();
        g.origin = RecordingOrigin::Imported;
        g.model.clear();
        g.synthesis_text = None;
        assert_eq!(o.status(None), VoiceStatus::Synced);
        o.dialogue = "Different words".into();
        assert_eq!(o.status(Some("other")), VoiceStatus::Stale);
    }

    #[test]
    fn trims_preserve_anchor_and_source_at_all_frame_rates() {
        for rate in [
            FrameRate::Fps24,
            FrameRate::Fps25,
            FrameRate::Fps2997Drop,
            FrameRate::Fps30,
        ] {
            let clock = Clock::new(ENGINE_RATE, rate);
            let mut o = order();
            o.trim_start = 4800;
            o.trim_end = 2400;
            let anchor = clock.frames_to_samples(o.at);
            assert_eq!(o.interval(clock), Some((anchor, anchor + 40_801)));
            o.alignment = Alignment::End;
            assert_eq!(o.interval(clock), Some((anchor - 40_801, anchor)));
            assert_eq!(o.generated.as_ref().unwrap().samples, 48_001);
            o.validate(&HashSet::from(["audio"])).unwrap();
            o.trim_end = 48_001;
            assert!(o.validate(&HashSet::from(["audio"])).is_err());
        }
    }

    #[test]
    fn pinning_voices_survives_another_installation_and_missing_voice_is_atomic() {
        let mut p = Project::new("Show", FrameRate::Fps25);
        let active = p.timelines[0].id.clone();
        apply(
            &mut p,
            &active,
            OperatorOp::AddTrack {
                name: "Operator".into(),
            },
        )
        .unwrap();
        let before = p.clone();
        assert!(freeze_voices(&mut p, None).is_err());
        assert_eq!(p, before);
        freeze_voices(&mut p, Some("installation")).unwrap();
        assert_eq!(
            effective_voice(&p, &p.timelines[0].tracks[0], Some("another")).0,
            Some("installation")
        );
    }

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
    fn content_status_tracks_dialogue_and_effective_voice_without_deleting_audio() {
        let mut order = order();
        assert_eq!(order.status(Some("voice")), VoiceStatus::Synced);
        order.dialogue = "Go lights".into();
        assert_eq!(order.status(Some("voice")), VoiceStatus::Stale);
        assert!(order.generated.is_some());
        order.dialogue = "Stand by lights".into();
        assert_eq!(order.status(Some("voice")), VoiceStatus::Synced);
        assert_eq!(order.status(Some("other")), VoiceStatus::Stale);
        order.generated = None;
        assert_eq!(order.status(None), VoiceStatus::Ungenerated);
    }

    #[test]
    fn partial_edits_preserve_other_fields_and_generated_content() {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].tracks.push(Track {
            id: "track".into(),
            name: "Operator".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Operator {
                voice: Some("voice".into()),
                orders: vec![order()],
            },
        });
        let active = project.timelines[0].id.clone();
        apply(
            &mut project,
            &active,
            OperatorOp::EditOrder {
                id: "order".into(),
                dialogue: None,
                at: Some(200),
                alignment: None,
            },
        )
        .unwrap();
        apply(
            &mut project,
            &active,
            OperatorOp::EditOrder {
                id: "order".into(),
                dialogue: None,
                at: None,
                alignment: Some(Alignment::End),
            },
        )
        .unwrap();
        let TrackKind::Operator { orders, .. } = &project.timelines[0].tracks[0].kind else {
            panic!()
        };
        assert_eq!(orders[0].at, 200);
        assert_eq!(orders[0].alignment, Alignment::End);
        assert_eq!(orders[0].dialogue, order().dialogue);
        assert_eq!(orders[0].generated, order().generated);
        assert_eq!(orders[0].status(Some("voice")), VoiceStatus::Synced);
    }

    #[test]
    fn both_alignments_preserve_anchor_and_exact_samples_at_every_rate() {
        for rate in [
            FrameRate::Fps24,
            FrameRate::Fps25,
            FrameRate::Fps2997Drop,
            FrameRate::Fps30,
        ] {
            let clock = Clock::new(ENGINE_RATE, rate);
            let mut order = order();
            let anchor = clock.frames_to_samples(order.at);
            assert_eq!(order.interval(clock), Some((anchor, anchor + 48_001)));
            order.alignment = Alignment::End;
            assert_eq!(order.interval(clock), Some((anchor - 48_001, anchor)));
            order.generated.as_mut().unwrap().samples = 72_001;
            assert_eq!(order.interval(clock), Some((anchor - 72_001, anchor)));
            order.at = 1;
            assert_eq!(order.interval(clock), None);
            assert_eq!(order.generated.as_ref().unwrap().samples, 72_001);
        }
    }

    #[test]
    fn voices_inherit_track_then_project_then_installation() {
        let mut project = Project::new("Show", FrameRate::Fps25);
        let active = project.timelines[0].id.clone();
        apply(
            &mut project,
            &active,
            OperatorOp::AddTrack {
                name: "Operator".into(),
            },
        )
        .unwrap();
        assert_eq!(
            effective_voice(&project, &project.timelines[0].tracks[0], Some("local")),
            (Some("local"), "flux")
        );
        project.voice = Some("project".into());
        assert_eq!(
            effective_voice(&project, &project.timelines[0].tracks[0], Some("local")),
            (Some("project"), "project")
        );
        let track_id = project.timelines[0].tracks[0].id.clone();
        apply(
            &mut project,
            &active,
            OperatorOp::SetVoice {
                track_id: Some(track_id),
                voice: Some("track".into()),
            },
        )
        .unwrap();
        assert_eq!(
            effective_voice(&project, &project.timelines[0].tracks[0], Some("local")),
            (Some("track"), "track")
        );
    }

    #[test]
    fn archive_and_recovery_keep_generated_audio_and_pending_orders() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("voice.mp3");
        std::fs::write(&audio, b"test audio bytes").unwrap();
        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        let active = project.timelines[0].id.clone();
        let asset = session.import_audio(&audio).unwrap();
        let mut ready = order();
        ready.generated.as_mut().unwrap().asset = asset.id.clone();
        let pending = VoiceOrder {
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
    fn rejected_edits_preserve_the_session_and_previous_generation() {
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        session
            .operator_edit(OperatorOp::AddTrack {
                name: "Operator".into(),
            })
            .unwrap();
        let before = session.document().unwrap();
        assert!(
            session
                .operator_edit(OperatorOp::SetVoice {
                    track_id: None,
                    voice: Some("../bad".into())
                })
                .is_err()
        );
        assert_eq!(session.document().unwrap(), before);
    }

    #[test]
    fn old_generations_remain_readable_after_save_prune_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let voice = dir.path().join("voice.mp3");
        std::fs::write(&voice, b"old generation").unwrap();
        let mut session = Session::default();
        let mut original = session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_audio(&voice).unwrap();
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
