//! What the user can do to the names of tracks and clips, the shape of a clip's fades and its
//! volume envelope. The window only says which operation and with what; the trimming of names, the
//! order of the points, their limits and the lookups live here, and
//! [`Session::item_edit`](super::Session::item_edit) refuses anything that would leave the project
//! invalid.

use serde::Deserialize;

use super::error::{ProjectError, Result};
use super::model::{
    AudioClip, ENVELOPE_MAX_DB, ENVELOPE_MIN_DB, EnvCurve, EnvPoint, FadeCurve, MAX_CLIP_NAME,
    MAX_ENVELOPE_POINTS, Project, TrackKind,
};

/// The most characters a track's name can have.
pub const MAX_TRACK_NAME: usize = 64;

/// Which end of a clip a fade is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Fade {
    In,
    Out,
}

/// An operation on a clip or a track of the active timeline. Positions of envelope points are in
/// thousandths of a frame from the start of the clip's audio file (see [`EnvPoint::at`]).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ItemOp {
    /// A track needs a name: a blank one is refused.
    RenameTrack {
        id: String,
        name: String,
    },
    /// A blank name takes the clip's own name away, so it goes by its audio file's name again.
    RenameClip {
        id: String,
        name: String,
    },
    SetFadeCurve {
        id: String,
        fade: Fade,
        curve: FadeCurve,
    },
    /// A point at `at`, in its place in the order. Refused where there is one already, or when the
    /// envelope is full.
    AddEnvelopePoint {
        id: String,
        at: i64,
        gain_db: f32,
    },
    /// Moves the point `index`, which stays between its neighbours.
    MoveEnvelopePoint {
        id: String,
        index: usize,
        at: i64,
        gain_db: f32,
    },
    RemoveEnvelopePoint {
        id: String,
        index: usize,
    },
    /// The curve from the point `index` to the next one.
    SetEnvelopeCurve {
        id: String,
        index: usize,
        curve: EnvCurve,
    },
    ClearEnvelope {
        id: String,
    },
}

/// `name` without the spaces around it, cut to `max` characters (not UTF-16 units or bytes).
fn tidy(name: &str, max: usize) -> String {
    name.trim().chars().take(max).collect()
}

/// A gain within the range of an envelope, to a tenth of a dB.
fn gain(db: f32) -> Result<f32> {
    if !db.is_finite() {
        return Err(ProjectError::Invalid("a gain must be a number".into()));
    }
    Ok(((db * 10.0).round() / 10.0).clamp(ENVELOPE_MIN_DB, ENVELOPE_MAX_DB))
}

fn clip_mut<'a>(project: &'a mut Project, active: &str, id: &str) -> Result<&'a mut AudioClip> {
    project
        .timelines
        .iter_mut()
        .filter(|timeline| timeline.id == active)
        .flat_map(|timeline| &mut timeline.tracks)
        .filter_map(|track| match &mut track.kind {
            TrackKind::Audio { clips } => Some(clips),
            _ => None,
        })
        .flatten()
        .find(|clip| clip.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("there is no clip {id}")))
}

fn point_mut(clip: &mut AudioClip, index: usize) -> Result<&mut EnvPoint> {
    let count = clip.envelope.len();
    clip.envelope.get_mut(index).ok_or_else(|| {
        ProjectError::Invalid(format!(
            "the envelope of clip {} has no point {index} (it has {count})",
            clip.id
        ))
    })
}

/// Applies `op` to the active timeline `active` of `project`.
pub fn apply(project: &mut Project, active: &str, op: ItemOp) -> Result<()> {
    match op {
        ItemOp::RenameTrack { id, name } => {
            let name = tidy(&name, MAX_TRACK_NAME);
            if name.is_empty() {
                return Err(ProjectError::Invalid("a track needs a name".into()));
            }
            let track = project
                .timelines
                .iter_mut()
                .filter(|timeline| timeline.id == active)
                .flat_map(|timeline| &mut timeline.tracks)
                .find(|track| track.id == id)
                .ok_or_else(|| ProjectError::Invalid(format!("there is no track {id}")))?;
            track.name = name;
        }
        ItemOp::RenameClip { id, name } => {
            let name = tidy(&name, MAX_CLIP_NAME);
            clip_mut(project, active, &id)?.name = (!name.is_empty()).then_some(name);
        }
        ItemOp::SetFadeCurve { id, fade, curve } => {
            let clip = clip_mut(project, active, &id)?;
            match fade {
                Fade::In => clip.fade_in_curve = curve,
                Fade::Out => clip.fade_out_curve = curve,
            }
        }
        ItemOp::AddEnvelopePoint { id, at, gain_db } => {
            let clip = clip_mut(project, active, &id)?;
            let at = at.max(0) as u64;
            if clip.envelope.len() >= MAX_ENVELOPE_POINTS {
                return Err(ProjectError::Invalid(format!(
                    "an envelope has at most {MAX_ENVELOPE_POINTS} points"
                )));
            }
            let place = clip.envelope.partition_point(|point| point.at < at);
            if clip.envelope.get(place).is_some_and(|point| point.at == at) {
                return Err(ProjectError::Invalid(
                    "there is a point there already".into(),
                ));
            }
            clip.envelope.insert(
                place,
                EnvPoint {
                    at,
                    gain_db: gain(gain_db)?,
                    curve: EnvCurve::Linear,
                },
            );
        }
        ItemOp::MoveEnvelopePoint {
            id,
            index,
            at,
            gain_db,
        } => {
            let clip = clip_mut(project, active, &id)?;
            point_mut(clip, index)?;
            // Between its neighbours, one thousandth of a frame from each; with no room it stays.
            let low = index.checked_sub(1).map_or(0, |i| clip.envelope[i].at + 1);
            let high = clip
                .envelope
                .get(index + 1)
                .map_or(u64::MAX, |next| next.at - 1);
            let point = &mut clip.envelope[index];
            if low <= high {
                point.at = (at.max(0) as u64).clamp(low, high);
            }
            point.gain_db = gain(gain_db)?;
        }
        ItemOp::RemoveEnvelopePoint { id, index } => {
            let clip = clip_mut(project, active, &id)?;
            point_mut(clip, index)?;
            clip.envelope.remove(index);
        }
        ItemOp::SetEnvelopeCurve { id, index, curve } => {
            point_mut(clip_mut(project, active, &id)?, index)?.curve = curve;
        }
        ItemOp::ClearEnvelope { id } => clip_mut(project, active, &id)?.envelope.clear(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::sample_project;
    use crate::session::Session;

    fn session() -> Session {
        let mut session = Session::default();
        session.update(sample_project());
        session
    }

    fn edit(session: &mut Session, op: ItemOp) -> Result<Project> {
        session.item_edit(op).map(|state| state.project)
    }

    fn clip(project: &Project) -> &AudioClip {
        let TrackKind::Audio { clips } = &project.timelines[0].tracks[0].kind else {
            panic!("the first track is audio");
        };
        &clips[0]
    }

    fn add(session: &mut Session, at: i64, gain_db: f32) -> Result<Project> {
        edit(
            session,
            ItemOp::AddEnvelopePoint {
                id: "c1".into(),
                at,
                gain_db,
            },
        )
    }

    fn points(project: &Project) -> Vec<(u64, f32)> {
        clip(project)
            .envelope
            .iter()
            .map(|point| (point.at, point.gain_db))
            .collect()
    }

    #[test]
    fn rename_track_should_trim_cut_and_refuse_a_blank_name() {
        let mut session = session();
        let rename = |name: &str| ItemOp::RenameTrack {
            id: "t1".into(),
            name: name.into(),
        };
        let project = edit(&mut session, rename("  Click  ")).unwrap();
        assert_eq!(project.timelines[0].tracks[0].name, "Click");
        let project = edit(&mut session, rename(&"🎵".repeat(80))).unwrap();
        assert_eq!(
            project.timelines[0].tracks[0].name.chars().count(),
            MAX_TRACK_NAME
        );
        let kept = project.timelines[0].tracks[0].name.clone();
        assert!(edit(&mut session, rename("   ")).is_err());
        assert_eq!(
            session.document().unwrap().project.timelines[0].tracks[0].name,
            kept
        );
    }

    #[test]
    fn rename_track_should_refuse_a_track_that_is_not_there() {
        let mut session = session();
        let op = ItemOp::RenameTrack {
            id: "nope".into(),
            name: "x".into(),
        };
        assert!(edit(&mut session, op).is_err());
    }

    #[test]
    fn rename_clip_should_name_it_and_a_blank_name_should_take_the_name_away() {
        let mut session = session();
        let rename = |name: &str| ItemOp::RenameClip {
            id: "c1".into(),
            name: name.into(),
        };
        let project = edit(&mut session, rename(" Intro ")).unwrap();
        assert_eq!(clip(&project).name.as_deref(), Some("Intro"));
        let project = edit(&mut session, rename("  ")).unwrap();
        assert_eq!(clip(&project).name, None);
        let project = edit(&mut session, rename(&"x".repeat(200))).unwrap();
        assert_eq!(
            clip(&project).name.as_deref().map(str::len),
            Some(MAX_CLIP_NAME)
        );
    }

    #[test]
    fn set_fade_curve_should_set_the_curve_of_one_end() {
        let mut session = session();
        let set = |fade, curve| ItemOp::SetFadeCurve {
            id: "c1".into(),
            fade,
            curve,
        };
        let project = edit(&mut session, set(Fade::In, FadeCurve::SCurve)).unwrap();
        assert_eq!(
            (clip(&project).fade_in_curve, clip(&project).fade_out_curve),
            (FadeCurve::SCurve, FadeCurve::Linear)
        );
        let project = edit(&mut session, set(Fade::Out, FadeCurve::Exponential)).unwrap();
        assert_eq!(clip(&project).fade_out_curve, FadeCurve::Exponential);
    }

    #[test]
    fn add_point_should_keep_the_points_in_order_and_round_the_gain() {
        let mut session = session();
        add(&mut session, 2000, -6.0).unwrap();
        add(&mut session, 0, 0.0).unwrap();
        let project = add(&mut session, 1000, -3.04).unwrap();
        assert_eq!(points(&project), [(0, 0.0), (1000, -3.0), (2000, -6.0)]);
    }

    #[test]
    fn add_point_should_clamp_the_gain_and_the_place() {
        let mut session = session();
        add(&mut session, -500, 90.0).unwrap();
        let project = add(&mut session, 10, -400.0).unwrap();
        assert_eq!(
            points(&project),
            [(0, ENVELOPE_MAX_DB), (10, ENVELOPE_MIN_DB)]
        );
    }

    #[test]
    fn add_point_should_refuse_a_taken_place_a_full_envelope_and_a_gain_that_is_not_a_number() {
        let mut session = session();
        add(&mut session, 1000, 0.0).unwrap();
        assert!(add(&mut session, 1000, -3.0).is_err());
        assert!(add(&mut session, 5, f32::NAN).is_err());
        for at in 0..(MAX_ENVELOPE_POINTS as i64 - 1) {
            add(&mut session, 2000 + at, 0.0).unwrap();
        }
        assert!(add(&mut session, 9_000_000, 0.0).is_err());
        assert_eq!(
            clip(&session.document().unwrap().project).envelope.len(),
            MAX_ENVELOPE_POINTS
        );
    }

    #[test]
    fn move_point_should_stay_between_its_neighbours_and_in_range() {
        let mut session = session();
        for (at, db) in [(0, 0.0), (1000, -6.0), (2000, 0.0)] {
            add(&mut session, at, db).unwrap();
        }
        let mv = |session: &mut Session, index, at, gain_db| {
            edit(
                session,
                ItemOp::MoveEnvelopePoint {
                    id: "c1".into(),
                    index,
                    at,
                    gain_db,
                },
            )
        };
        assert_eq!(
            points(&mv(&mut session, 1, 1500, -12.0).unwrap()),
            [(0, 0.0), (1500, -12.0), (2000, 0.0)]
        );
        assert_eq!(
            points(&mv(&mut session, 1, 5000, -12.0).unwrap())[1].0,
            1999
        );
        assert_eq!(points(&mv(&mut session, 1, -5000, -12.0).unwrap())[1].0, 1);
        assert_eq!(points(&mv(&mut session, 0, -100, 0.0).unwrap())[0].0, 0);
        assert_eq!(
            points(&mv(&mut session, 1, 1000, -400.0).unwrap())[1].1,
            ENVELOPE_MIN_DB
        );
        assert_eq!(
            points(&mv(&mut session, 1, 1000, 400.0).unwrap())[1].1,
            ENVELOPE_MAX_DB
        );
        assert!(mv(&mut session, 9, 0, 0.0).is_err());
    }

    #[test]
    fn move_point_should_stay_put_when_its_neighbours_leave_no_room() {
        let mut session = session();
        for at in [10, 11, 12] {
            add(&mut session, at, 0.0).unwrap();
        }
        let project = edit(
            &mut session,
            ItemOp::MoveEnvelopePoint {
                id: "c1".into(),
                index: 1,
                at: 500,
                gain_db: -3.0,
            },
        )
        .unwrap();
        assert_eq!(points(&project)[1], (11, -3.0));
    }

    #[test]
    fn remove_point_and_set_curve_and_clear_should_edit_the_envelope() {
        let mut session = session();
        add(&mut session, 0, 0.0).unwrap();
        add(&mut session, 1000, -6.0).unwrap();
        let project = edit(
            &mut session,
            ItemOp::SetEnvelopeCurve {
                id: "c1".into(),
                index: 0,
                curve: EnvCurve::Smooth,
            },
        )
        .unwrap();
        assert_eq!(clip(&project).envelope[0].curve, EnvCurve::Smooth);
        let remove = |index| ItemOp::RemoveEnvelopePoint {
            id: "c1".into(),
            index,
        };
        let project = edit(&mut session, remove(0)).unwrap();
        assert_eq!(points(&project), [(1000, -6.0)]);
        assert!(edit(&mut session, remove(3)).is_err());
        let project = edit(&mut session, ItemOp::ClearEnvelope { id: "c1".into() }).unwrap();
        assert!(clip(&project).envelope.is_empty());
    }

    #[test]
    fn a_refused_edit_should_leave_the_document_as_it_was() {
        let mut session = session();
        add(&mut session, 1000, 0.0).unwrap();
        let before = session.document().unwrap().project;
        assert!(add(&mut session, 1000, -3.0).is_err());
        assert_eq!(session.document().unwrap().project, before);
    }

    #[test]
    fn ops_should_read_from_the_camel_case_the_window_sends() {
        let op: ItemOp = serde_json::from_str(
            r#"{"op":"moveEnvelopePoint","id":"c1","index":2,"at":1500,"gainDb":-6.5}"#,
        )
        .unwrap();
        assert_eq!(
            op,
            ItemOp::MoveEnvelopePoint {
                id: "c1".into(),
                index: 2,
                at: 1500,
                gain_db: -6.5
            }
        );
        let fade: ItemOp = serde_json::from_str(
            r#"{"op":"setFadeCurve","id":"c1","fade":"out","curve":"equalPower"}"#,
        )
        .unwrap();
        assert_eq!(
            fade,
            ItemOp::SetFadeCurve {
                id: "c1".into(),
                fade: Fade::Out,
                curve: FadeCurve::EqualPower
            }
        );
    }
}
