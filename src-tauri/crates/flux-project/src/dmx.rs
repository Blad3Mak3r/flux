//! The DMX track: lighting scenes at points of the timeline, sent over Art-Net or sACN, and the
//! operations that edit them.
//!
//! A scene sets some channels to a value at its time, fading from what the lights show then.
//! Channels a scene does not name keep their value (tracking, as lighting consoles do) unless the
//! scene releases the others. What the lights show at a position, and the packets, are the
//! engine's (`flux-engine/src/dmx.rs`); this module is the data and its rules.
//!
//! Universes are the Art-Net **Port-Address**, counted from 0 as the protocol does; sACN sends the
//! same universe as `universe + 1`, since sACN has no universe 0.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::{ProjectError, Result};
use super::model::{MAX_CLIP_NAME, Project, Track, TrackKind};
use crate::serde_util::{is_false, is_zero};

/// The highest universe: the 15 bits of an Art-Net Port-Address.
pub const MAX_UNIVERSE: u16 = 32_767;
/// Channels in a universe.
pub const UNIVERSE_SIZE: u16 = 512;
/// The most channels one scene can set, so a file cannot ask for an absurd amount of work.
pub const MAX_SCENE_CHANNELS: usize = 4_096;

/// One channel of a scene: the value it fades to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DmxChannel {
    /// From 0, as Art-Net numbers them.
    pub universe: u16,
    /// From 1 to 512.
    pub channel: u16,
    pub value: u8,
}

/// A lighting state to go to at `at`, over `fade` frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DmxScene {
    pub id: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// How many frames the lights take to get there, in a straight line; 0 is at once.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub fade: u64,
    /// Channels the scene does not name go to 0 instead of keeping their value.
    #[serde(default, skip_serializing_if = "is_false")]
    pub release_others: bool,
    /// Sorted by universe and channel.
    pub channels: Vec<DmxChannel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DmxProtocol {
    Artnet,
    Sacn,
}

/// A receiver of DMX over UDP (a node, a console, or a broadcast address), defined once for the
/// project and named by the tracks that use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DmxDestination {
    pub id: String,
    pub name: String,
    pub protocol: DmxProtocol,
    /// An address or a name: a node's, or a broadcast one such as `255.255.255.255`.
    pub host: String,
}

fn invalid(what: impl std::fmt::Display) -> ProjectError {
    ProjectError::Invalid(what.to_string())
}

impl DmxDestination {
    pub(super) fn validate(&self) -> Result<()> {
        let host_ok = !self.host.is_empty() && !self.host.chars().any(char::is_whitespace);
        if self.id.is_empty() || !host_ok {
            return Err(invalid(format!("DMX destination {} needs a host", self.id)));
        }
        Ok(())
    }
}

impl DmxScene {
    pub(super) fn validate(&self) -> Result<()> {
        let id = &self.id;
        if self.name.chars().count() > MAX_CLIP_NAME {
            return Err(invalid(format!(
                "scene {id}: a name has at most {MAX_CLIP_NAME} characters"
            )));
        }
        if self.channels.len() > MAX_SCENE_CHANNELS {
            return Err(invalid(format!(
                "scene {id}: at most {MAX_SCENE_CHANNELS} channels"
            )));
        }
        let mut seen = HashSet::new();
        for c in &self.channels {
            if c.universe > MAX_UNIVERSE || !(1..=UNIVERSE_SIZE).contains(&c.channel) {
                return Err(invalid(format!(
                    "scene {id}: universes are 0 to {MAX_UNIVERSE} and channels 1 to {UNIVERSE_SIZE}"
                )));
            }
            if !seen.insert((c.universe, c.channel)) {
                return Err(invalid(format!(
                    "scene {id}: channel {} of universe {} is set twice",
                    c.channel, c.universe
                )));
            }
        }
        Ok(())
    }
}

/// What the user can do to the DMX tracks and destinations. The window only says which and with
/// what; the defaults, the order and the clean-up of references are decided here.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DmxOp {
    SetScene {
        id: String,
        name: String,
        fade: u64,
        release_others: bool,
    },
    /// Adds the first channel after the scene's last one (channel 1 of universe 0 for the first).
    AddChannel { id: String },
    /// Changes the channel `from_universe`/`from_channel` of the scene to the other three values.
    SetChannel {
        id: String,
        from_universe: u16,
        from_channel: u16,
        universe: u16,
        channel: u16,
        value: u8,
    },
    RemoveChannel {
        id: String,
        universe: u16,
        channel: u16,
    },
    SetDestination {
        track_id: String,
        destination: Option<String>,
    },
    AddDestination {
        name: String,
        protocol: DmxProtocol,
        host: String,
    },
    UpdateDestination {
        id: String,
        name: String,
        protocol: DmxProtocol,
        host: String,
    },
    /// Tracks that sent to it are left without a destination.
    RemoveDestination { id: String },
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn missing(what: &str, id: &str) -> ProjectError {
    ProjectError::Invalid(format!("there is no {what} {id}"))
}

fn tracks_mut(project: &mut Project) -> impl Iterator<Item = &mut Track> {
    project
        .timelines
        .iter_mut()
        .flat_map(|timeline| timeline.tracks.iter_mut())
}

fn scene_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut DmxScene> {
    tracks_mut(project)
        .find_map(|track| match &mut track.kind {
            TrackKind::Dmx { scenes, .. } => scenes.iter_mut().find(|scene| scene.id == id),
            _ => None,
        })
        .ok_or_else(|| missing("scene", id))
}

fn destination_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut DmxDestination> {
    project
        .output
        .dmx
        .iter_mut()
        .find(|destination| destination.id == id)
        .ok_or_else(|| missing("DMX destination", id))
}

/// A track with no scenes. A project with a single receiver needs no choosing.
pub(crate) fn empty_kind(project: &Project) -> TrackKind {
    TrackKind::Dmx {
        destination: project.output.dmx.first().map(|d| d.id.clone()),
        scenes: Vec::new(),
    }
}

/// A scene that changes nothing yet, to start editing from.
pub(crate) fn blank_scene(id: String, at: u64) -> DmxScene {
    DmxScene {
        id,
        at,
        name: String::new(),
        fade: 0,
        release_others: false,
        channels: Vec::new(),
    }
}

/// Scenes in the order the show reaches them (stable), channels by universe and number.
pub(crate) fn tidy(project: &mut Project) {
    for track in tracks_mut(project) {
        if let TrackKind::Dmx { scenes, .. } = &mut track.kind {
            scenes.sort_by_key(|scene| scene.at);
            for scene in scenes {
                scene.channels.sort_by_key(|c| (c.universe, c.channel));
            }
        }
    }
}

/// Applies `op` to `project`, whose active timeline is `active`. Returns the id of what it created.
pub(crate) fn apply(project: &mut Project, op: DmxOp) -> Result<Option<String>> {
    let mut created = None;
    match op {
        DmxOp::SetScene {
            id,
            name,
            fade,
            release_others,
        } => {
            let scene = scene_mut(project, &id)?;
            scene.name = name.trim().to_owned();
            scene.fade = fade;
            scene.release_others = release_others;
        }
        DmxOp::AddChannel { id } => {
            let scene = scene_mut(project, &id)?;
            let next = scene
                .channels
                .iter()
                .max_by_key(|c| (c.universe, c.channel))
                .map_or((0, 1), |last| {
                    if last.channel < UNIVERSE_SIZE {
                        (last.universe, last.channel + 1)
                    } else {
                        (last.universe + 1, 1)
                    }
                });
            scene.channels.push(DmxChannel {
                universe: next.0,
                channel: next.1,
                value: 255,
            });
        }
        DmxOp::SetChannel {
            id,
            from_universe,
            from_channel,
            universe,
            channel,
            value,
        } => {
            let scene = scene_mut(project, &id)?;
            let at = scene
                .channels
                .iter()
                .position(|c| c.universe == from_universe && c.channel == from_channel)
                .ok_or_else(|| missing("channel", &format!("{from_universe}/{from_channel}")))?;
            scene.channels[at] = DmxChannel {
                universe,
                channel,
                value,
            };
        }
        DmxOp::RemoveChannel {
            id,
            universe,
            channel,
        } => scene_mut(project, &id)?
            .channels
            .retain(|c| !(c.universe == universe && c.channel == channel)),
        DmxOp::SetDestination {
            track_id,
            destination: chosen,
        } => {
            let track = tracks_mut(project)
                .find(|track| track.id == track_id)
                .ok_or_else(|| missing("track", &track_id))?;
            match &mut track.kind {
                TrackKind::Dmx { destination, .. } => *destination = chosen,
                _ => return Err(missing("DMX track", &track_id)),
            }
        }
        DmxOp::AddDestination {
            name,
            protocol,
            host,
        } => {
            let id = new_id();
            created = Some(id.clone());
            project.output.dmx.push(DmxDestination {
                id,
                name: name.trim().to_owned(),
                protocol,
                host: host.trim().to_owned(),
            });
        }
        DmxOp::UpdateDestination {
            id,
            name,
            protocol,
            host,
        } => {
            let destination = destination_mut(project, &id)?;
            destination.name = name.trim().to_owned();
            destination.protocol = protocol;
            destination.host = host.trim().to_owned();
        }
        DmxOp::RemoveDestination { id } => {
            destination_mut(project, &id)?;
            project
                .output
                .dmx
                .retain(|destination| destination.id != id);
            for track in tracks_mut(project) {
                if let TrackKind::Dmx { destination, .. } = &mut track.kind
                    && destination.as_deref() == Some(id.as_str())
                {
                    *destination = None;
                }
            }
        }
    }
    tidy(project);
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue_ops::{CueOp, CueTrackKind};
    use crate::model::tests::sample_project;
    use crate::session::Session;

    fn session() -> Session {
        let mut session = Session::default();
        session.update(sample_project());
        session
    }

    fn add_destination(s: &mut Session) -> String {
        s.dmx_edit(DmxOp::AddDestination {
            name: " Node ".into(),
            protocol: DmxProtocol::Artnet,
            host: "127.0.0.1".into(),
        })
        .unwrap()
        .id
        .unwrap()
    }

    fn scenes(project: &Project) -> Vec<DmxScene> {
        project
            .timelines
            .iter()
            .flat_map(|t| &t.tracks)
            .filter_map(|t| match &t.kind {
                TrackKind::Dmx { scenes, .. } => Some(scenes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn add_scene_at(s: &mut Session, at: u64) -> String {
        s.cue_edit(CueOp::AddEvent {
            track_id: None,
            kind: CueTrackKind::Dmx,
            at,
            track_name: "Lights".into(),
        })
        .unwrap()
        .id
        .unwrap()
    }

    #[test]
    fn a_new_scene_should_make_the_track_and_use_the_only_destination() {
        let mut s = session();
        let destination = add_destination(&mut s);
        let id = add_scene_at(&mut s, 50);
        let project = s.document().unwrap().project;
        let track = project.timelines[0].tracks.last().unwrap();
        assert_eq!(track.name, "Lights");
        assert!(
            matches!(&track.kind, TrackKind::Dmx { destination: Some(d), .. } if *d == destination)
        );
        assert_eq!(scenes(&project)[0].id, id);
        project.validate().unwrap();
    }

    #[test]
    fn scenes_should_stay_in_the_order_of_the_show_and_channels_sorted() {
        let mut s = session();
        let a = add_scene_at(&mut s, 100);
        let b = add_scene_at(&mut s, 10);
        for _ in 0..3 {
            s.dmx_edit(DmxOp::AddChannel { id: a.clone() }).unwrap();
        }
        let edited = s
            .dmx_edit(DmxOp::SetChannel {
                id: a.clone(),
                from_universe: 0,
                from_channel: 1,
                universe: 1,
                channel: 5,
                value: 7,
            })
            .unwrap();
        let all = scenes(&edited.state.project);
        assert_eq!(all[0].id, b);
        let keys: Vec<_> = all[1]
            .channels
            .iter()
            .map(|c| (c.universe, c.channel))
            .collect();
        assert_eq!(keys, [(0, 2), (0, 3), (1, 5)]);
        let moved = s
            .cue_edit(CueOp::MoveEvent {
                id: a.clone(),
                at: 1,
            })
            .unwrap();
        assert_eq!(scenes(&moved.state.project)[0].id, a);
    }

    #[test]
    fn a_channel_set_twice_or_out_of_range_should_be_refused_and_change_nothing() {
        let mut s = session();
        let id = add_scene_at(&mut s, 0);
        for _ in 0..2 {
            s.dmx_edit(DmxOp::AddChannel { id: id.clone() }).unwrap();
        }
        let before = s.document().unwrap().project;
        for (universe, channel) in [(0, 2), (0, 513), (0, 0), (40_000, 1)] {
            let refused = s.dmx_edit(DmxOp::SetChannel {
                id: id.clone(),
                from_universe: 0,
                from_channel: 1,
                universe,
                channel,
                value: 1,
            });
            assert!(refused.is_err(), "{universe}/{channel}");
        }
        assert_eq!(s.document().unwrap().project, before);
    }

    #[test]
    fn a_scene_should_round_trip_through_json() {
        let mut s = session();
        let id = add_scene_at(&mut s, 0);
        s.dmx_edit(DmxOp::SetScene {
            id,
            name: " Blue ".into(),
            fade: 50,
            release_others: true,
        })
        .unwrap();
        let scene = scenes(&s.document().unwrap().project).remove(0);
        assert_eq!(
            (scene.name.as_str(), scene.fade, scene.release_others),
            ("Blue", 50, true)
        );
        let json = serde_json::to_value(&scene).unwrap();
        assert_eq!(json["releaseOthers"], true);
        assert_eq!(serde_json::from_value::<DmxScene>(json).unwrap(), scene);
    }

    #[test]
    fn removing_a_destination_should_leave_its_tracks_without_one() {
        let mut s = session();
        let destination = add_destination(&mut s);
        add_scene_at(&mut s, 0);
        s.dmx_edit(DmxOp::RemoveDestination { id: destination })
            .unwrap();
        let project = s.document().unwrap().project;
        assert!(project.output.dmx.is_empty());
        assert!(matches!(
            &project.timelines[0].tracks.last().unwrap().kind,
            TrackKind::Dmx {
                destination: None,
                ..
            }
        ));
    }

    #[test]
    fn a_track_should_not_point_at_a_destination_that_does_not_exist() {
        let mut s = session();
        add_scene_at(&mut s, 0);
        let track = s.document().unwrap().project.timelines[0]
            .tracks
            .last()
            .unwrap()
            .id
            .clone();
        let refused = s.dmx_edit(DmxOp::SetDestination {
            track_id: track,
            destination: Some("ghost".into()),
        });
        assert!(refused.is_err());
    }

    #[test]
    fn a_version_16_project_should_load_unchanged_and_keep_no_dmx() {
        let mut value = serde_json::to_value(sample_project()).unwrap();
        value["formatVersion"] = 16.into();
        let before = value.clone();
        crate::migrate::upgrade(&mut value, "2026-01-01T00:00:00Z");
        assert_eq!(value["formatVersion"], crate::FORMAT_VERSION);
        value["formatVersion"] = 16.into();
        assert_eq!(value, before);
        let project: Project = serde_json::from_value(before).unwrap();
        assert!(project.output.dmx.is_empty());
        project.validate().unwrap();
    }

    #[test]
    fn a_project_without_dmx_should_serialize_without_it() {
        let json = serde_json::to_value(sample_project()).unwrap();
        assert!(json["output"].get("dmx").is_none());
    }
}
