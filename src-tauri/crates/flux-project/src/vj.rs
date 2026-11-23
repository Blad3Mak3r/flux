//! The VJ grid of a project, played like a media server's clip deck: layers stacked one over the
//! other, columns across them, and in each cell at most one clip, a video the user launches by hand.
//! A project holds its timelines and its grid at once; its `mode` only says which of the two the
//! window shows, so switching between them loses nothing.
//!
//! The layers are listed in drawing order: the first is at the bottom, the last on top. Opacity,
//! volume and the crossfader side of a layer are stored with the project; which clip is playing is
//! not, it belongs to the show running.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{ENVELOPE_MAX_DB, ENVELOPE_MIN_DB, MAX_CLIP_NAME, Project, VideoFit};
use crate::{ProjectError, Result};

/// Most layers a grid has.
pub const MAX_LAYERS: usize = 16;
/// Most columns a grid has.
pub const MAX_COLUMNS: usize = 64;
/// The grid a project gets the first time it is shown in VJ mode.
pub const DEFAULT_LAYERS: usize = 3;
pub const DEFAULT_COLUMNS: usize = 8;

/// Which part of the project the window shows and plays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectMode {
    /// The timelines, their tracks and cues, and the timecode.
    #[default]
    Timeline,
    /// The VJ grid.
    Vj,
}

impl ProjectMode {
    pub(crate) fn is_timeline(&self) -> bool {
        *self == Self::Timeline
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vj {
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// Left first.
    pub columns: Vec<Column>,
}

impl Vj {
    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.columns.is_empty()
    }

    fn layer_mut(&mut self, id: &str) -> Result<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
    }

    fn clip_mut(&mut self, id: &str) -> Result<&mut Clip> {
        self.layers
            .iter_mut()
            .flat_map(|layer| &mut layer.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no clip {id}")))
    }

    fn ensure_column(&self, id: &str) -> Result<()> {
        if self.columns.iter().any(|column| column.id == id) {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!("no column {id}")))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub id: String,
    /// Absent is the column's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    /// Absent is the layer's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// How much of the layer covers the ones below, 0 to 1; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub opacity: f32,
    /// The volume of the sound of the layer's clips, in dB; absent is 0 dB.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gain_db: f32,
    /// The side of the crossfader the layer is on; absent is neither, so the crossfader leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossfade: Option<Side>,
    /// At most one per column, in no particular order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
}

fn full() -> f32 {
    1.0
}

fn is_full(value: &f32) -> bool {
    *value == 1.0
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

impl Layer {
    fn new(id: String) -> Self {
        Self {
            id,
            name: None,
            opacity: 1.0,
            gain_db: 0.0,
            crossfade: None,
            clips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    A,
    B,
}

/// A video in a cell of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    /// Id of the column the clip is in; the layer is the one that lists it.
    pub column: String,
    /// Id of an entry in `Project::videos`.
    pub asset: String,
    /// Absent is the video's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the clip does when it reaches its end; absent is [`Playback::Loop`].
    #[serde(default, skip_serializing_if = "Playback::is_loop")]
    pub playback: Playback,
    /// How the video is sized into the composition; absent is [`VideoFit::Fit`].
    #[serde(default, skip_serializing_if = "is_fit")]
    pub fit: VideoFit,
}

fn is_fit(fit: &VideoFit) -> bool {
    *fit == VideoFit::Fit
}

/// What a launched clip does when it reaches the end of its video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Playback {
    /// Starts again from the beginning.
    #[default]
    Loop,
    /// Stops, and the layer shows nothing.
    Once,
    /// Plays backwards to the beginning, then forwards again.
    PingPong,
}

impl Playback {
    fn is_loop(&self) -> bool {
        *self == Self::Loop
    }
}

/// What the user can do to the grid. Every id names something of the grid; a name that is empty
/// after trimming goes back to the default one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VjOp {
    /// Shows the timelines or the grid. The first time a project shows the grid it gets
    /// [`DEFAULT_LAYERS`] layers and [`DEFAULT_COLUMNS`] columns.
    SetMode {
        mode: ProjectMode,
    },
    /// A new empty layer on top of the others.
    AddLayer,
    /// Takes the layer and its clips away; the last layer stays.
    RemoveLayer {
        layer: String,
    },
    /// Puts the layer at `to` of the drawing order, 0 being the bottom.
    MoveLayer {
        layer: String,
        to: usize,
    },
    RenameLayer {
        layer: String,
        name: String,
    },
    LayerOpacity {
        layer: String,
        opacity: f32,
    },
    LayerGain {
        layer: String,
        gain_db: f32,
    },
    LayerCrossfade {
        layer: String,
        side: Option<Side>,
    },
    /// A new empty column on the right.
    AddColumn,
    /// Takes the column and its clips away; the last column stays.
    RemoveColumn {
        column: String,
    },
    MoveColumn {
        column: String,
        to: usize,
    },
    RenameColumn {
        column: String,
        name: String,
    },
    /// Puts a video of the project in a cell, in place of the clip that was there.
    PlaceVideo {
        asset: String,
        layer: String,
        column: String,
    },
    /// Moves a clip to a cell; a clip already there takes the place the moved one left.
    MoveClip {
        clip: String,
        layer: String,
        column: String,
    },
    RemoveClip {
        clip: String,
    },
    RenameClip {
        clip: String,
        name: String,
    },
    ClipPlayback {
        clip: String,
        playback: Playback,
    },
    ClipFit {
        clip: String,
        fit: VideoFit,
    },
}

/// Applies `op` to the project; `new_id` names whatever it creates. The caller validates the result.
pub fn apply(project: &mut Project, op: VjOp, mut new_id: impl FnMut() -> String) -> Result<()> {
    let vj = &mut project.vj;
    match op {
        VjOp::SetMode { mode } => {
            if mode == ProjectMode::Vj && vj.is_empty() {
                vj.layers = (0..DEFAULT_LAYERS).map(|_| Layer::new(new_id())).collect();
                vj.columns = (0..DEFAULT_COLUMNS)
                    .map(|_| Column {
                        id: new_id(),
                        name: None,
                    })
                    .collect();
            }
            project.mode = mode;
        }
        VjOp::AddLayer => {
            if vj.layers.len() >= MAX_LAYERS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_LAYERS} layers"
                )));
            }
            vj.layers.push(Layer::new(new_id()));
        }
        VjOp::RemoveLayer { layer } => {
            let index = layer_index(vj, &layer)?;
            if vj.layers.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a layer".into()));
            }
            vj.layers.remove(index);
        }
        VjOp::MoveLayer { layer, to } => {
            let index = layer_index(vj, &layer)?;
            let moved = vj.layers.remove(index);
            vj.layers.insert(to.min(vj.layers.len()), moved);
        }
        VjOp::RenameLayer { layer, name } => vj.layer_mut(&layer)?.name = typed_name(&name)?,
        VjOp::LayerOpacity { layer, opacity } => vj.layer_mut(&layer)?.opacity = opacity,
        VjOp::LayerGain { layer, gain_db } => vj.layer_mut(&layer)?.gain_db = gain_db,
        VjOp::LayerCrossfade { layer, side } => vj.layer_mut(&layer)?.crossfade = side,
        VjOp::AddColumn => {
            if vj.columns.len() >= MAX_COLUMNS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_COLUMNS} columns"
                )));
            }
            vj.columns.push(Column {
                id: new_id(),
                name: None,
            });
        }
        VjOp::RemoveColumn { column } => {
            let index = column_index(vj, &column)?;
            if vj.columns.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a column".into()));
            }
            vj.columns.remove(index);
            for layer in &mut vj.layers {
                layer.clips.retain(|clip| clip.column != column);
            }
        }
        VjOp::MoveColumn { column, to } => {
            let index = column_index(vj, &column)?;
            let moved = vj.columns.remove(index);
            vj.columns.insert(to.min(vj.columns.len()), moved);
        }
        VjOp::RenameColumn { column, name } => {
            let index = column_index(vj, &column)?;
            vj.columns[index].name = typed_name(&name)?;
        }
        VjOp::PlaceVideo {
            asset,
            layer,
            column,
        } => {
            if !project.videos.iter().any(|video| video.id == asset) {
                return Err(ProjectError::MissingAsset(asset));
            }
            vj.ensure_column(&column)?;
            let layer = vj.layer_mut(&layer)?;
            layer.clips.retain(|clip| clip.column != column);
            layer.clips.push(Clip {
                id: new_id(),
                column,
                asset,
                name: None,
                playback: Playback::Loop,
                fit: VideoFit::Fit,
            });
        }
        VjOp::MoveClip {
            clip,
            layer,
            column,
        } => {
            vj.ensure_column(&column)?;
            layer_index(vj, &layer)?;
            let (from_layer, from_column) = vj
                .layers
                .iter()
                .find_map(|l| {
                    l.clips
                        .iter()
                        .find(|c| c.id == clip)
                        .map(|c| (l.id.clone(), c.column.clone()))
                })
                .ok_or_else(|| ProjectError::Invalid(format!("no clip {clip}")))?;
            let moved = take_clip(vj, &from_layer, |c| c.id == clip);
            let occupant = take_clip(vj, &layer, |c| c.column == column);
            if let Some(mut occupant) = occupant {
                occupant.column = from_column;
                vj.layer_mut(&from_layer)?.clips.push(occupant);
            }
            if let Some(mut moved) = moved {
                moved.column = column;
                vj.layer_mut(&layer)?.clips.push(moved);
            }
        }
        VjOp::RemoveClip { clip } => {
            vj.clip_mut(&clip)?;
            for layer in &mut vj.layers {
                layer.clips.retain(|c| c.id != clip);
            }
        }
        VjOp::RenameClip { clip, name } => vj.clip_mut(&clip)?.name = typed_name(&name)?,
        VjOp::ClipPlayback { clip, playback } => vj.clip_mut(&clip)?.playback = playback,
        VjOp::ClipFit { clip, fit } => vj.clip_mut(&clip)?.fit = fit,
    }
    Ok(())
}

fn layer_index(vj: &Vj, id: &str) -> Result<usize> {
    vj.layers
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
}

fn column_index(vj: &Vj, id: &str) -> Result<usize> {
    vj.columns
        .iter()
        .position(|column| column.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no column {id}")))
}

fn take_clip(vj: &mut Vj, layer: &str, which: impl Fn(&Clip) -> bool) -> Option<Clip> {
    let layer = vj.layers.iter_mut().find(|l| l.id == layer)?;
    let index = layer.clips.iter().position(which)?;
    Some(layer.clips.remove(index))
}

/// A typed name, or `None` for an empty one (the default name).
fn typed_name(name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.chars().count() > MAX_CLIP_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name has at most {MAX_CLIP_NAME} characters"
        )));
    }
    Ok((!name.is_empty()).then(|| name.to_owned()))
}

/// Checks the grid: unique ids (a clip's is unique among every item of the project, in `item_ids`),
/// each clip in a known column, at most one clip per cell, clips showing known videos, levels in
/// range, and a grid in VJ mode.
pub(crate) fn validate<'a>(
    project: &'a Project,
    video_ids: &HashSet<&str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    let vj = &project.vj;
    if project.mode == ProjectMode::Vj && (vj.layers.is_empty() || vj.columns.is_empty()) {
        return Err(ProjectError::Invalid(
            "a project in VJ mode needs a layer and a column".into(),
        ));
    }
    if vj.layers.len() > MAX_LAYERS || vj.columns.len() > MAX_COLUMNS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_LAYERS} layers and {MAX_COLUMNS} columns"
        )));
    }
    let mut ids = HashSet::new();
    let mut columns = HashSet::new();
    for column in &vj.columns {
        unique(&mut ids, &column.id)?;
        columns.insert(column.id.as_str());
        check_name(column.name.as_deref())?;
    }
    for layer in &vj.layers {
        unique(&mut ids, &layer.id)?;
        check_name(layer.name.as_deref())?;
        if !(0.0..=1.0).contains(&layer.opacity) {
            return Err(ProjectError::Invalid(format!(
                "layer {} needs an opacity of 0 to 1",
                layer.id
            )));
        }
        if !(ENVELOPE_MIN_DB..=ENVELOPE_MAX_DB).contains(&layer.gain_db) {
            return Err(ProjectError::Invalid(format!(
                "layer {} needs a volume of {ENVELOPE_MIN_DB} to {ENVELOPE_MAX_DB} dB",
                layer.id
            )));
        }
        let mut cells = HashSet::new();
        for clip in &layer.clips {
            if !item_ids.insert(&clip.id) {
                return Err(ProjectError::Invalid(format!(
                    "duplicate clip id: {}",
                    clip.id
                )));
            }
            if !columns.contains(clip.column.as_str()) {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is in no column of the grid",
                    clip.id
                )));
            }
            if !cells.insert(clip.column.as_str()) {
                return Err(ProjectError::Invalid(format!(
                    "layer {} has two clips in column {}",
                    layer.id, clip.column
                )));
            }
            if !video_ids.contains(clip.asset.as_str()) {
                return Err(ProjectError::MissingAsset(clip.asset.clone()));
            }
            check_name(clip.name.as_deref())?;
        }
    }
    Ok(())
}

fn unique<'a>(seen: &mut HashSet<&'a str>, id: &'a str) -> Result<()> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate grid id: {id}")))
    }
}

fn check_name(name: Option<&str>) -> Result<()> {
    match name {
        Some(name) if name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME => {
            Err(ProjectError::Invalid(format!(
                "a grid name has 1 to {MAX_CLIP_NAME} characters; leave it out instead"
            )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::video_project;

    fn ids() -> impl FnMut() -> String {
        let mut next = 0;
        move || {
            next += 1;
            format!("g{next}")
        }
    }

    /// A project shown in VJ mode, with its default grid (layers g1 to g3, columns g4 to g11).
    fn vj_project() -> (Project, impl FnMut() -> String) {
        let mut project = video_project();
        let mut new_id = ids();
        apply(
            &mut project,
            VjOp::SetMode {
                mode: ProjectMode::Vj,
            },
            &mut new_id,
        )
        .unwrap();
        (project, new_id)
    }

    fn place(project: &mut Project, layer: &str, column: &str, new_id: impl FnMut() -> String) {
        apply(
            project,
            VjOp::PlaceVideo {
                asset: "v1".into(),
                layer: layer.into(),
                column: column.into(),
            },
            new_id,
        )
        .unwrap();
    }

    #[test]
    fn set_mode_should_give_the_first_vj_view_a_default_grid_and_keep_the_timelines() {
        let (project, _) = vj_project();
        assert_eq!(project.mode, ProjectMode::Vj);
        assert_eq!(project.vj.layers.len(), DEFAULT_LAYERS);
        assert_eq!(project.vj.columns.len(), DEFAULT_COLUMNS);
        assert_eq!(project.timelines, video_project().timelines);
        project.validate().unwrap();
    }

    #[test]
    fn set_mode_should_keep_the_grid_when_going_back_and_forth() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id);
        let grid = project.vj.clone();
        for mode in [ProjectMode::Timeline, ProjectMode::Vj] {
            apply(&mut project, VjOp::SetMode { mode }, &mut new_id).unwrap();
        }
        assert_eq!(project.vj, grid);
    }

    #[test]
    fn place_video_should_replace_the_clip_of_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id);
        place(&mut project, "g1", "g4", &mut new_id);
        let clips = &project.vj.layers[0].clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "g13");
        project.validate().unwrap();
    }

    #[test]
    fn place_video_should_refuse_an_unknown_video() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::PlaceVideo {
            asset: "nope".into(),
            layer: "g1".into(),
            column: "g4".into(),
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn move_clip_should_swap_with_the_clip_already_in_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id); // g12
        place(&mut project, "g2", "g5", &mut new_id); // g13
        let op = VjOp::MoveClip {
            clip: "g12".into(),
            layer: "g2".into(),
            column: "g5".into(),
        };
        apply(&mut project, op, &mut new_id).unwrap();
        let cell = |layer: usize, id: &str| {
            project.vj.layers[layer]
                .clips
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.column.clone())
        };
        assert_eq!(cell(1, "g12").as_deref(), Some("g5"));
        assert_eq!(cell(0, "g13").as_deref(), Some("g4"));
        project.validate().unwrap();
    }

    #[test]
    fn remove_column_should_take_its_clips_and_keep_the_last_column() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id);
        apply(
            &mut project,
            VjOp::RemoveColumn {
                column: "g4".into(),
            },
            &mut new_id,
        )
        .unwrap();
        assert!(project.vj.layers[0].clips.is_empty());
        project.vj.columns.truncate(1);
        let last = project.vj.columns[0].id.clone();
        let op = VjOp::RemoveColumn { column: last };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn move_layer_should_clamp_the_place() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::MoveLayer {
            layer: "g1".into(),
            to: 99,
        };
        apply(&mut project, op, &mut new_id).unwrap();
        let order: Vec<_> = project.vj.layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(order, ["g2", "g3", "g1"]);
    }

    #[test]
    fn rename_should_take_an_empty_name_back_to_the_default() {
        let (mut project, mut new_id) = vj_project();
        for (name, expected) in [(" Intro ", Some("Intro")), ("  ", None)] {
            let op = VjOp::RenameLayer {
                layer: "g1".into(),
                name: name.into(),
            };
            apply(&mut project, op, &mut new_id).unwrap();
            assert_eq!(project.vj.layers[0].name.as_deref(), expected);
        }
    }

    #[test]
    fn validate_should_refuse_levels_out_of_range() {
        let (mut project, _) = vj_project();
        project.vj.layers[0].opacity = 1.5;
        assert!(project.validate().is_err());
        project.vj.layers[0].opacity = 0.5;
        project.vj.layers[0].gain_db = f32::NAN;
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_refuse_a_clip_id_used_by_a_timeline_clip() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id);
        project.vj.layers[0].clips[0].id = "cv".into();
        assert!(project.validate().is_err());
    }

    #[test]
    fn json_should_leave_out_a_timeline_project_without_a_grid() {
        let value = serde_json::to_value(video_project()).unwrap();
        assert!(value.get("mode").is_none());
        assert!(value.get("vj").is_none());
    }

    #[test]
    fn json_should_round_trip_a_grid() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g4", &mut new_id);
        project.vj.layers[0].crossfade = Some(Side::B);
        project.vj.layers[0].clips[0].playback = Playback::PingPong;
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains(r#""mode":"vj""#));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, project);
    }

    #[test]
    fn used_assets_should_count_the_clips_of_the_grid() {
        let (mut project, mut new_id) = vj_project();
        project.timelines[0].tracks.clear();
        place(&mut project, "g1", "g4", &mut new_id);
        assert!(project.used_assets().contains("v1"));
    }
}
