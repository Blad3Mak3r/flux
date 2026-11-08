//! Portable audio destinations and track assignments; hardware is supplied by the installation.
use crate::{AudioOutput, Project, Result, Track, TrackKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, thiserror::Error, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum RoutingError {
    #[error("invalid audio output identity")]
    InvalidId,
    #[error("audio output names must be nonempty and at most 64 characters")]
    InvalidName,
    #[error("audio output names must be unique")]
    DuplicateName,
    #[error("unknown audio output")]
    UnknownOutput,
    #[error("this track has no audio output")]
    UnsupportedTrack,
    #[error("unknown audio track")]
    UnknownTrack,
    #[error("the project could not be updated")]
    InvalidProject,
}

pub const PROGRAMME: &str = "programme";
pub const OPERATOR: &str = "operator";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AudioAssignment {
    #[default]
    Inherit,
    Silent,
    Output {
        id: String,
    },
}
impl AudioAssignment {
    pub fn is_inherit(&self) -> bool {
        matches!(self, Self::Inherit)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioBus {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedAudioBus {
    pub id: String,
    pub route: Option<AudioOutput>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRoute {
    pub track_id: String,
    pub output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AudioChoice {
    pub id: String,
    pub name: Option<String>,
    pub users: usize,
}
pub fn choices(project: &Project) -> Vec<AudioChoice> {
    [
        AudioBus {
            id: PROGRAMME.into(),
            name: String::new(),
        },
        AudioBus {
            id: OPERATOR.into(),
            name: String::new(),
        },
    ]
    .into_iter()
    .chain(project.output.audio.iter().cloned())
    .map(|b| AudioChoice {
        users: project
            .timelines
            .iter()
            .flat_map(|t| &t.tracks)
            .filter(|t| target(t) == Some(b.id.as_str()))
            .count(),
        id: b.id,
        name: (!b.name.is_empty()).then_some(b.name),
    })
    .collect()
}

pub fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub fn audible(track: &Track) -> bool {
    matches!(
        track.kind,
        TrackKind::Audio { .. } | TrackKind::Operator { .. }
    )
}
pub fn target(track: &Track) -> Option<&str> {
    if !audible(track) {
        return None;
    }
    match &track.audio_output {
        AudioAssignment::Silent => None,
        AudioAssignment::Output { id } => Some(id),
        AudioAssignment::Inherit => Some(if matches!(track.kind, TrackKind::Operator { .. }) {
            OPERATOR
        } else {
            PROGRAMME
        }),
    }
}
pub fn validate(project: &Project) -> Result<()> {
    let mut ids = std::collections::HashSet::from([PROGRAMME, OPERATOR]);
    let mut names = std::collections::HashSet::new();
    for bus in &project.output.audio {
        if !valid_id(&bus.id) || !ids.insert(bus.id.as_str()) {
            return Err(RoutingError::InvalidId.into());
        }
        if bus.name.trim().is_empty() || bus.name.chars().count() > crate::MAX_TRACK_NAME {
            return Err(RoutingError::InvalidName.into());
        }
        if !names.insert(bus.name.trim().to_lowercase()) {
            return Err(RoutingError::DuplicateName.into());
        }
    }
    for track in project.timelines.iter().flat_map(|t| &t.tracks) {
        if !audible(track) && track.audio_output != AudioAssignment::Inherit {
            return Err(RoutingError::UnsupportedTrack.into());
        }
        if let AudioAssignment::Output { id } = &track.audio_output
            && !ids.contains(id.as_str())
        {
            return Err(RoutingError::UnknownOutput.into());
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AudioRoutingOp {
    Add {
        name: String,
    },
    Rename {
        id: String,
        name: String,
    },
    Remove {
        id: String,
    },
    Assign {
        track_id: String,
        output: AudioAssignment,
    },
}
pub fn apply(project: &mut Project, active: &str, op: AudioRoutingOp) -> Result<()> {
    match op {
        AudioRoutingOp::Add { name } => project.output.audio.push(AudioBus {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.trim().into(),
        }),
        AudioRoutingOp::Rename { id, name } => {
            project
                .output
                .audio
                .iter_mut()
                .find(|b| b.id == id)
                .ok_or(RoutingError::UnknownOutput)?
                .name = name.trim().into()
        }
        AudioRoutingOp::Remove { id } => {
            if !project.output.audio.iter().any(|b| b.id == id) {
                return Err(RoutingError::UnknownOutput.into());
            }
            project.output.audio.retain(|b| b.id != id);
            for track in project.timelines.iter_mut().flat_map(|t| &mut t.tracks) {
                if matches!(&track.audio_output, AudioAssignment::Output { id: target } if *target == id)
                {
                    track.audio_output = AudioAssignment::Silent;
                }
            }
        }
        AudioRoutingOp::Assign { track_id, output } => {
            let track = project
                .timelines
                .iter_mut()
                .find(|t| t.id == active)
                .into_iter()
                .flat_map(|t| &mut t.tracks)
                .find(|t| t.id == track_id)
                .ok_or(RoutingError::UnknownTrack)?;
            if !audible(track) {
                return Err(RoutingError::UnsupportedTrack.into());
            }
            track.audio_output = output;
        }
    }
    validate(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(id: &str, kind: TrackKind) -> Track {
        Track {
            id: id.into(),
            name: id.into(),
            muted: false,
            gain_db: None,
            color: None,
            kind,
            audio_output: AudioAssignment::Inherit,
        }
    }
    #[test]
    fn inheritance_and_explicit_assignments_are_independent_of_voice() {
        let mut audio = track("a", TrackKind::Audio { clips: vec![] });
        let voice = track(
            "o",
            TrackKind::Operator {
                voice: None,
                orders: vec![],
            },
        );
        assert_eq!(target(&audio), Some(PROGRAMME));
        assert_eq!(target(&voice), Some(OPERATOR));
        audio.audio_output = AudioAssignment::Output {
            id: OPERATOR.into(),
        };
        assert_eq!(target(&audio), Some(OPERATOR));
        audio.audio_output = AudioAssignment::Silent;
        assert_eq!(target(&audio), None);
    }
    #[test]
    fn remove_retargets_every_timeline_and_history_and_recovery_restore_routes() {
        let mut session = crate::Session::default();
        let mut p = session.new_project("Show", crate::FrameRate::Fps25);
        let active = p.timelines[0].id.clone();
        apply(
            &mut p,
            &active,
            AudioRoutingOp::Add {
                name: "Monitors".into(),
            },
        )
        .unwrap();
        let id = p.output.audio[0].id.clone();
        let mut audio = track("a", TrackKind::Audio { clips: vec![] });
        audio.audio_output = AudioAssignment::Output { id: id.clone() };
        p.timelines[0].tracks.push(audio);
        let mut other = p.timelines[0].clone();
        other.id = uuid::Uuid::new_v4().to_string();
        other.tracks[0].id = "b".into();
        p.timelines.push(other);
        session.update(p.clone());
        assert_eq!(choices(&p)[2].users, 2);
        let removed = session
            .edit_audio_routing(AudioRoutingOp::Remove { id })
            .unwrap()
            .project;
        assert!(
            removed
                .timelines
                .iter()
                .all(|t| t.tracks[0].audio_output == AudioAssignment::Silent)
        );
        session.update(p.clone()); // undo applies the previous document through the same session boundary
        let recovery = session.recovery().unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::recovery::write(dir.path(), &recovery).unwrap();
        assert_eq!(
            crate::recovery::read(dir.path(), &recovery.id)
                .unwrap()
                .document
                .project,
            p
        );
        let file = dir.path().join("routed.flx");
        crate::save(&file, &p, &Default::default(), &Default::default()).unwrap();
        assert_eq!(crate::open(&file).unwrap().0, p);
        let mut restored = crate::Session::default();
        assert_eq!(restored.restore_recovery(recovery).unwrap().project, p);
        session.update(removed.clone());
        assert_eq!(session.document().unwrap().project, removed);
    }
    #[test]
    fn invalid_edits_leave_the_document_intact() {
        let mut session = crate::Session::default();
        session.new_project("Show", crate::FrameRate::Fps25);
        session
            .edit_audio_routing(AudioRoutingOp::Add {
                name: "Monitor".into(),
            })
            .unwrap();
        let before = session.document().unwrap();
        assert!(
            session
                .edit_audio_routing(AudioRoutingOp::Add {
                    name: " monitor ".into()
                })
                .is_err()
        );
        assert!(
            session
                .edit_audio_routing(AudioRoutingOp::Remove {
                    id: PROGRAMME.into()
                })
                .is_err()
        );
        assert_eq!(session.document().unwrap().project, before.project);
    }
    #[test]
    fn version_nine_archives_open_with_inherited_routes_and_save_as_ten() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::new("Legacy", crate::FrameRate::Fps25);
        p.timelines[0]
            .tracks
            .push(track("audio", TrackKind::Audio { clips: vec![] }));
        let mut value = serde_json::to_value(&p).unwrap();
        value["formatVersion"] = serde_json::json!(9);
        value["timelines"][0]["tracks"][0]
            .as_object_mut()
            .unwrap()
            .remove("audioOutput");
        let path = dir.path().join("old.fxtc");
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("project.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
        zip.finish().unwrap();
        let (opened, _) = crate::open(&path).unwrap();
        assert_eq!(opened.format_version, 11);
        assert_eq!(target(&opened.timelines[0].tracks[0]), Some(PROGRAMME));
        assert!(opened.output.audio.is_empty());
    }
}
