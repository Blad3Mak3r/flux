//! Editing the orders of a track.

use flux_core::ENGINE_RATE;
use serde::Deserialize;
use uuid::Uuid;

use super::*;
use crate::{Project, ProjectError, Result, Track, TrackKind};

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
    SetMode {
        id: String,
        mode: VoiceMode,
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
            audio_output: Default::default(),
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
            OperatorOp::SetMode { id, mode } if orders.iter().any(|o| o.id == *id) => {
                if let Some(order) = orders.iter_mut().find(|o| o.id == *id) {
                    order.mode = *mode;
                }
            }
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
                    mode: Default::default(),
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

    use crate::operator::support::*;
    use crate::{FrameRate, Project, Session, Track, TrackKind};
    use flux_core::{Clock, ENGINE_RATE};
    use std::collections::HashSet;

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
    fn partial_edits_preserve_other_fields_and_generated_content() {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
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
    fn recorded_audio_ignores_provider_settings_and_mode_changes_preserve_it() {
        let mut p = Project::new("Show", FrameRate::Fps25);
        let active = p.timelines[0].id.clone();
        let mut o = order();
        let id = o.id.clone();
        o.generated.as_mut().unwrap().origin = RecordingOrigin::Recorded;
        assert_eq!(o.status(None), VoiceStatus::Synced);
        p.timelines[0].tracks.push(Track {
            id: "operator".into(),
            name: "Operator".into(),
            muted: false,
            gain_db: None,
            color: None,
            audio_output: Default::default(),
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![o.clone()],
            },
        });
        apply(
            &mut p,
            &active,
            OperatorOp::SetMode {
                id: id.clone(),
                mode: VoiceMode::Manual,
            },
        )
        .unwrap();
        apply(
            &mut p,
            &active,
            OperatorOp::SetMode {
                id,
                mode: VoiceMode::Generated,
            },
        )
        .unwrap();
        let TrackKind::Operator { orders, .. } = &p.timelines[0].tracks[0].kind else {
            panic!()
        };
        assert_eq!(orders[0], o);
    }
}
