//! The edits the window asks for, each applied to a copy and kept only when still valid.

use uuid::Uuid;

use super::{ProjectState, Session, audio_clips, no_project, timeline_mut, unknown_clip};
use crate::bulk_ops::{self, Extent, ItemsOp, Lengths};
use crate::clips::{self, ClipOp};
use crate::cue_ops::{self, CueEdited, CueOp};
use crate::error::{ProjectError, Result};
use crate::item_ops::{self, ItemOp};
use crate::model::{AudioAsset, TrackKind, VideoAsset};
use crate::video_clips::{self, VideoClipOp};
use flux_core::FrameRate;

impl Session {
    /// The asset a clip of the active timeline plays, and the frame rate of that timeline.
    pub fn clip_asset(&self, clip_id: &str) -> Result<(AudioAsset, FrameRate)> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let project = &document.project;
        let timeline = self.active_timeline(project)?;
        let clip = audio_clips(timeline)
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| unknown_clip(clip_id))?;
        let asset = project
            .assets
            .iter()
            .find(|asset| asset.id == clip.asset)
            .ok_or_else(|| ProjectError::Invalid(format!("unknown asset {}", clip.asset)))?;
        Ok((asset.clone(), timeline.timecode.frame_rate))
    }

    /// Trims, fades or splits a clip of the active timeline (see [`ClipOp`]) at `frame`. The asset
    /// it plays lasts `asset_frames`.
    pub fn edit_clip(
        &mut self,
        clip_id: &str,
        op: ClipOp,
        frame: u64,
        asset_frames: u64,
    ) -> Result<ProjectState> {
        self.change(|project, active| {
            let timeline = timeline_mut(project, active)?;
            for track in &mut timeline.tracks {
                let TrackKind::Audio { clips } = &mut track.kind else {
                    continue;
                };
                if let Some(index) = clips.iter().position(|clip| clip.id == clip_id) {
                    let edited = clips::apply(&clips[index], op, frame, asset_frames, || {
                        Uuid::new_v4().to_string()
                    })?;
                    clips.splice(index..=index, edited);
                    return Ok(());
                }
            }
            Err(unknown_clip(clip_id))
        })
    }

    /// The video a clip of the active timeline shows, and the frame rate of that timeline.
    pub fn video_clip_asset(&self, clip_id: &str) -> Result<(VideoAsset, FrameRate)> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let timeline = self.active_timeline(&document.project)?;
        let clip = timeline
            .tracks
            .iter()
            .filter_map(|track| match &track.kind {
                TrackKind::Video { clips } => Some(clips),
                _ => None,
            })
            .flatten()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| unknown_clip(clip_id))?;
        let asset = document
            .project
            .videos
            .iter()
            .find(|video| video.id == clip.asset)
            .ok_or_else(|| ProjectError::MissingAsset(clip.asset.clone()))?;
        Ok((asset.clone(), timeline.timecode.frame_rate))
    }

    /// Moves, trims, splits or removes a video clip of the active timeline (see [`VideoClipOp`])
    /// at `frame`. Its video lasts `video_frames` at the timeline's rate. A removed clip takes its
    /// track with it when it was the last one; the video file goes at the next save if nothing
    /// else shows it.
    pub fn edit_video_clip(
        &mut self,
        clip_id: &str,
        op: VideoClipOp,
        frame: u64,
        video_frames: u64,
    ) -> Result<ProjectState> {
        self.change(|project, active| {
            let timeline = timeline_mut(project, active)?;
            for index in 0..timeline.tracks.len() {
                let TrackKind::Video { clips } = &mut timeline.tracks[index].kind else {
                    continue;
                };
                if let Some(at) = clips.iter().position(|clip| clip.id == clip_id) {
                    let edited = video_clips::apply(&clips[at], op, frame, video_frames, || {
                        Uuid::new_v4().to_string()
                    })?;
                    clips.splice(at..=at, edited);
                    if clips.is_empty() {
                        timeline.tracks.remove(index);
                    }
                    return Ok(());
                }
            }
            Err(unknown_clip(clip_id))
        })
    }

    /// Renames a track or a clip, or changes a clip's fade curves or volume envelope (see [`ItemOp`]).
    pub fn item_edit(&mut self, op: ItemOp) -> Result<ProjectState> {
        self.change(|project, active| item_ops::apply(project, active, op))
    }

    /// Edits the markers, regions and tempo changes of the active timeline (see [`MarksOp`]).
    pub fn marks_edit(&mut self, op: crate::MarksOp) -> Result<ProjectState> {
        self.change(|project, active| crate::marks::apply(project, active, op))
    }

    /// Moves or removes many items of the active timeline at once, with ripple if asked (see
    /// [`ItemsOp`]); `lengths` is what [`Session::length_assets`] asked for.
    pub fn items_edit(&mut self, op: ItemsOp, lengths: &Lengths) -> Result<ProjectState> {
        self.change(|project, active| bulk_ops::apply(project, active, op, lengths))
    }

    /// The stretch of the active timeline the items `ids` cover, or all its content when empty.
    pub fn items_extent(&self, ids: &[String], lengths: &Lengths) -> Result<Option<Extent>> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        bulk_ops::extent(&document.project, &document.active, ids, lengths)
    }

    /// The media of the clips of the active timeline that play all of it, whose length the
    /// backend must read to know where those clips end, and the frame rate of the timeline.
    pub fn length_assets(&self) -> Result<(Vec<AudioAsset>, Vec<VideoAsset>, FrameRate)> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let project = &document.project;
        let timeline = self.active_timeline(project)?;
        let (mut audio, mut video) = (Vec::new(), Vec::new());
        for track in &timeline.tracks {
            match &track.kind {
                TrackKind::Audio { clips } => audio.extend(
                    clips
                        .iter()
                        .filter(|clip| clip.length.is_none())
                        .filter_map(|clip| project.assets.iter().find(|a| a.id == clip.asset)),
                ),
                TrackKind::Video { clips } => video.extend(
                    clips
                        .iter()
                        .filter(|clip| clip.length.is_none())
                        .filter_map(|clip| project.videos.iter().find(|v| v.id == clip.asset)),
                ),
                _ => {}
            }
        }
        audio.sort_by(|a, b| a.id.cmp(&b.id));
        audio.dedup_by(|a, b| a.id == b.id);
        video.sort_by(|a, b| a.id.cmp(&b.id));
        video.dedup_by(|a, b| a.id == b.id);
        Ok((
            audio.into_iter().cloned().collect(),
            video.into_iter().cloned().collect(),
            timeline.timecode.frame_rate,
        ))
    }

    /// Edits the MIDI and OSC tracks and the OSC destinations (see [`CueOp`]).
    pub fn cue_edit(&mut self, op: CueOp) -> Result<CueEdited> {
        let mut id = None;
        let state = self.change(|project, active| {
            id = cue_ops::apply(project, active, op)?;
            Ok(())
        })?;
        Ok(CueEdited { state, id })
    }

    /// Edits the DMX tracks and destinations (see [`crate::DmxOp`]).
    pub fn dmx_edit(&mut self, op: crate::DmxOp) -> Result<CueEdited> {
        let mut id = None;
        let state = self.change(|project, _| {
            id = crate::dmx::apply(project, op)?;
            Ok(())
        })?;
        Ok(CueEdited { state, id })
    }

    pub fn operator_edit(&mut self, op: crate::OperatorOp) -> Result<ProjectState> {
        self.change(|project, active| crate::operator::apply(project, active, op))
    }

    pub fn edit_audio_routing(
        &mut self,
        op: crate::routing::AudioRoutingOp,
    ) -> Result<ProjectState> {
        self.change(|project, active| crate::routing::apply(project, active, op))
    }

    /// Edits the slices of one screen of the video output (see [`crate::mapping`]).
    pub fn edit_mapping(
        &mut self,
        screen: &str,
        op: crate::mapping::MappingOp,
    ) -> Result<ProjectState> {
        self.change(|project, _| {
            let screen = crate::mapping::screen_mut(&mut project.output.screens, screen)?;
            crate::mapping::apply(&mut screen.slices, op, || Uuid::new_v4().to_string())
        })
    }

    /// Edits the masks of one screen of the video output.
    pub fn edit_masks(&mut self, screen: &str, op: crate::mapping::MaskOp) -> Result<ProjectState> {
        self.change(|project, _| {
            let screen = crate::mapping::screen_mut(&mut project.output.screens, screen)?;
            crate::mapping::edit_masks(&mut screen.masks, op, || Uuid::new_v4().to_string())
        })
    }

    /// Adds, removes or renames a screen of the video output.
    pub fn edit_screens(&mut self, op: crate::mapping::ScreenOp) -> Result<ProjectState> {
        self.change(|project, _| {
            crate::mapping::edit_screens(&mut project.output.screens, op, || {
                Uuid::new_v4().to_string()
            })
        })
    }

    /// Sets the size of the video composition from two typed numbers of pixels.
    /// Sets the size of the composition; both sides empty go back to the first video's size.
    pub fn set_composition(&mut self, width: &str, height: &str) -> Result<ProjectState> {
        let composition = if width.trim().is_empty() && height.trim().is_empty() {
            None
        } else {
            Some(crate::mapping::Composition::parse(width, height)?)
        };
        self.change(|project, _| {
            project.output.composition = composition;
            Ok(())
        })
    }

    pub fn edit_signal(&mut self, op: crate::SignalOp) -> Result<ProjectState> {
        self.change(|project, _| project.output.edit(op))
    }

    /// Edits the VJ grid of a VJ project (see [`crate::vj`]).
    pub fn edit_vj(&mut self, op: crate::VjOp) -> Result<ProjectState> {
        self.change(|project, _| crate::vj::apply(project, op, || Uuid::new_v4().to_string()))
    }

    /// Puts the mappings of a `.flxmap` file into the VJ grid (see [`crate::vj_mapfile`]); the
    /// answer is the project with them and how many were taken and skipped.
    pub fn import_vj_mappings(
        &mut self,
        file: &crate::vj_mapfile::MapFile,
        choice: &crate::vj_mapfile::ImportChoice,
    ) -> Result<(ProjectState, crate::vj_mapfile::ImportReport)> {
        let mut report = crate::vj_mapfile::ImportReport::default();
        let state = self.change(|project, _| {
            if project.mode != crate::ProjectMode::Vj {
                return Err(ProjectError::Invalid("only a VJ show has mappings".into()));
            }
            report = crate::vj_mapfile::import(&mut project.vj, file, choice, || {
                Uuid::new_v4().to_string()
            });
            Ok(())
        })?;
        Ok((state, report))
    }

    /// What importing the file would do to the VJ grid, without doing it.
    pub fn plan_vj_mappings(
        &self,
        file: &crate::vj_mapfile::MapFile,
    ) -> Result<crate::vj_mapfile::ImportPlan> {
        let grid = self
            .grid_ref()
            .ok_or_else(|| ProjectError::Invalid("only a VJ show has mappings".into()))?;
        Ok(crate::vj_mapfile::plan(grid, file))
    }

    /// The mappings of the VJ grid as the text of a `.flxmap` file.
    pub fn export_vj_mappings(&self) -> Result<String> {
        let grid = self
            .grid_ref()
            .ok_or_else(|| ProjectError::Invalid("only a VJ show has mappings".into()))?;
        crate::vj_mapfile::export(grid)
    }

    /// Moves a layer's level or a clip's transform while a fader or a drag goes, on the project
    /// held, without the copy and full check of an edit: what the screens show follows, and the
    /// edit sent when the hand lets go is the one kept. Any other op is refused.
    pub fn preview_vj(&mut self, op: crate::VjOp) -> Result<()> {
        use crate::VjOp as Op;
        let level = op.is_preview();
        if !level {
            return Err(ProjectError::Invalid(
                "only a level, a transform, a generator, an effect or a modulation is previewed"
                    .into(),
            ));
        }
        if let Op::ClipGenerator { generator, .. } = &op {
            generator.clone().tidy().validate()?;
        }
        self.ensure_editable()?;
        let document = self.document.as_mut().ok_or_else(no_project)?;
        crate::vj::apply(&mut document.project, op, String::new)?;
        // The screens copy the grid again when this moves.
        self.recovery_revision += 1;
        Ok(())
    }
}
