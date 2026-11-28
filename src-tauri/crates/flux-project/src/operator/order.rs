//! Reading an order: its voice, its freezing and its view.

use std::collections::HashSet;

use flux_core::{Clock, ENGINE_RATE};

use super::*;
use crate::{Project, ProjectError, Result, Track, TrackKind};

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
                || (g.origin != RecordingOrigin::Generated && !g.model.is_empty())
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
                    && (g.origin != RecordingOrigin::Generated
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
                        audio_offset_seconds: order.trim_start as f64 / f64::from(ENGINE_RATE),
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::operator::support::*;
    use crate::{FrameRate, Project, Track, TrackKind};
    use flux_core::ENGINE_RATE;

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
    fn waveform_view_keeps_exact_source_offset_and_trimmed_duration_for_both_alignments() {
        let mut project = Project::new("Show", FrameRate::Fps25);
        let mut order = order();
        order.trim_start = 4_801;
        order.trim_end = 2_400;
        let expected_samples = order.play_samples().unwrap();
        project.timelines[0].tracks.push(Track {
            id: "operator".into(),
            name: "Operator".into(),
            muted: false,
            gain_db: None,
            color: None,
            audio_output: Default::default(),
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![order],
            },
        });
        for alignment in [Alignment::Start, Alignment::End] {
            let TrackKind::Operator { orders, .. } = &mut project.timelines[0].tracks[0].kind
            else {
                panic!()
            };
            orders[0].alignment = alignment;
            let view = views(&project, Some("voice")).remove(0);
            let frames = expected_samples as f64 / f64::from(ENGINE_RATE) * 25.0;
            assert_eq!(view.audio_offset_seconds, 4_801.0 / f64::from(ENGINE_RATE));
            assert_eq!(view.length, Some(frames));
            assert_eq!(
                view.start,
                Some(if alignment == Alignment::Start {
                    100.0
                } else {
                    100.0 - frames
                })
            );
        }
    }
}
