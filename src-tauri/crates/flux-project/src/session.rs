use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::archive::{self, AssetSource};
use super::clips::{self, ClipOp};
use super::cue_ops::{self, CueEdited, CueOp};
use super::error::{ProjectError, Result};
use super::item_ops::{self, ItemOp};
use super::model::{
    AUDIO_DIR, AudioAsset, AudioClip, FORMAT_VERSION, Project, TimecodeSettings, Timeline,
    TrackKind, check_asset_path,
};
use super::preview::{AudioStamp, Preview, StoredPreview};
use flux_core::FrameRate;

/// The document being edited, as the backend last heard of it. The window that edits it can be
/// reloaded, and picks it up from here again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub project: Project,
    /// Where the `.fxtc` lives, or `None` for a project that was never saved.
    pub path: Option<PathBuf>,
    /// The project as it was last opened or saved, or as it started for a new one: the document is
    /// unsaved when `project` differs from it.
    pub baseline: Project,
    /// Id of the timeline being edited and played. Only the backend chooses it.
    pub active: String,
}

impl Document {
    fn new(project: Project, path: Option<PathBuf>) -> Self {
        Self {
            active: first_timeline(&project),
            baseline: project.clone(),
            project,
            path,
        }
    }

    /// Makes `id` the active timeline when the project has it.
    fn select(&mut self, id: &str) {
        if self.project.timeline(id).is_some() {
            id.clone_into(&mut self.active);
        }
    }

    /// An undo in the window can take the active timeline away: the first one takes over.
    fn settle(&mut self) {
        if self.project.timeline(&self.active).is_none() {
            self.active = first_timeline(&self.project);
        }
    }
}

fn first_timeline(project: &Project) -> String {
    project
        .timelines
        .first()
        .map(|timeline| timeline.id.clone())
        .unwrap_or_default()
}

/// A project and the timeline that is active in it: what the window shows after the backend
/// changed something.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectState {
    pub project: Project,
    pub active: String,
}

/// Backend state for the project being edited.
///
/// The frontend edits the `Project` and sends each state of it to the session, which keeps the
/// latest as the [`Document`]. For the audio, the session remembers where the bytes of each asset
/// live: in the `.fxtc` it was opened or saved from, or in a file imported from disk that is not in
/// any archive yet. It also holds the waveforms generated since the project was opened, which the
/// next save writes into the file.
#[derive(Debug, Default)]
pub struct Session {
    pub execution_lock: std::sync::Arc<std::sync::atomic::AtomicBool>,
    recovery_id: String,
    recovery_revision: u64,
    document: Option<Document>,
    sources: HashMap<String, AssetSource>,
    voice_sources: HashMap<String, AssetSource>,
    previews: HashMap<String, StoredPreview>,
}

impl Session {
    /// Lets the worker skip cloning an unchanged document on each timer tick.
    pub fn recovery_revision(&self) -> (&str, u64) {
        (&self.recovery_id, self.recovery_revision)
    }

    /// Captures metadata and source paths only; never reads audio or preview bytes.
    pub fn recovery(&self) -> Option<super::recovery::Recovery> {
        self.document.as_ref().map(|document| {
            super::recovery::Recovery::new(
                self.recovery_id.clone(),
                document.clone(),
                self.sources.clone(),
            )
        })
    }

    pub fn restore_recovery(&mut self, recovery: super::recovery::Recovery) -> Result<Document> {
        self.ensure_editable()?;
        recovery.validate()?;
        self.recovery_id.clone_from(&recovery.id);
        self.recovery_revision += 1;
        self.sources = recovery.sources;
        self.voice_sources = self
            .sources
            .iter()
            .filter(|(_, s)| matches!(s, AssetSource::Disk(_)))
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect();
        self.previews.clear();
        self.document = Some(recovery.document.clone());
        Ok(recovery.document)
    }

    pub fn set_autosave(&mut self, autosave: super::model::Autosave) -> Result<ProjectState> {
        autosave.validate()?;
        self.change(|project, _| {
            project.autosave = autosave;
            Ok(())
        })
    }

    /// Forgets the open document and its audio sources without touching any project files.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn new_project(&mut self, name: &str, frame_rate: FrameRate) -> Project {
        self.recovery_id = Uuid::new_v4().to_string();
        self.recovery_revision += 1;
        self.sources.clear();
        self.voice_sources.clear();
        self.previews.clear();
        let project = Project::new(name, frame_rate);
        self.document = Some(Document::new(project.clone(), None));
        project
    }

    /// Opens and migrates a project in memory. Only an explicit save upgrades the archive on disk.
    pub fn open(&mut self, path: &Path) -> Result<Project> {
        self.ensure_editable()?;
        let path = std::path::absolute(path)?;
        let (project, _archive) = archive::open(&path)?;
        self.recovery_id = Uuid::new_v4().to_string();
        self.recovery_revision += 1;
        self.sources = archive_sources(&project, &path);
        self.voice_sources.clear();
        self.previews.clear();
        self.document = Some(Document::new(project.clone(), Some(path.to_path_buf())));
        Ok(project)
    }

    /// Remembers the latest state of the project being edited.
    pub fn update(&mut self, project: Project) {
        if self.ensure_editable().is_err() {
            return;
        }
        if self
            .document
            .as_ref()
            .is_some_and(|doc| doc.project == project)
        {
            return;
        }
        self.recovery_revision += 1;
        match &mut self.document {
            Some(document) => {
                document.project = project;
                document.settle();
            }
            None => {
                self.recovery_id = Uuid::new_v4().to_string();
                self.document = Some(Document::new(project, None));
            }
        }
    }

    /// A delayed window sync must not resurrect a closed document or replace a different one.
    pub fn sync(&mut self, project: Project) -> Result<String> {
        if self
            .document
            .as_ref()
            .is_none_or(|doc| doc.project.meta.id != project.meta.id)
        {
            return Err(ProjectError::Invalid(
                "the document is no longer open".into(),
            ));
        }
        if self.document.as_ref().is_some_and(|d| d.project != project) {
            self.ensure_editable()?;
        }
        self.update(project);
        Ok(self.active())
    }

    /// The id of the active timeline, empty before a document was started.
    pub fn active(&self) -> String {
        self.document
            .as_ref()
            .map(|document| document.active.clone())
            .unwrap_or_default()
    }

    /// The timeline of `project` that is active in the document, else its first one. The engine
    /// plays the project the window sends, and this says which of its timelines.
    pub fn active_timeline<'a>(&self, project: &'a Project) -> Result<&'a Timeline> {
        self.document
            .as_ref()
            .and_then(|document| project.timeline(&document.active))
            .or_else(|| project.timelines.first())
            .ok_or_else(no_timeline)
    }

    /// Adds a timeline called `"{label} {n}"` with the frame rate of the active one, and activates
    /// it. The window supplies the translated `label`; the number is the new count.
    pub fn add_timeline(&mut self, label: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            let rate = project
                .timeline(active)
                .or(project.timelines.first())
                .ok_or_else(no_timeline)?
                .timecode
                .frame_rate;
            let timeline = Timeline::new(format!("{label} {}", project.timelines.len() + 1), rate);
            *active = timeline.id.clone();
            project.timelines.push(timeline);
            Ok(())
        })
    }

    pub fn rename_timeline(&mut self, id: &str, name: &str) -> Result<ProjectState> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProjectError::Invalid("a timeline needs a name".into()));
        }
        self.change(|project, _| {
            timeline_mut(project, id)?.name = name.to_owned();
            Ok(())
        })
    }

    /// Removes a timeline and the audio no clip uses any more. The one that takes its place becomes
    /// active when it was the active one.
    pub fn remove_timeline(&mut self, id: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            if project
                .setlists
                .iter()
                .flat_map(|s| &s.entries)
                .any(|e| e.timeline_id == id)
            {
                return Err(ProjectError::Invalid("show.error.referenced".into()));
            }
            let index = project
                .timelines
                .iter()
                .position(|timeline| timeline.id == id)
                .ok_or_else(|| unknown_timeline(id))?;
            if project.timelines.len() == 1 {
                return Err(no_timeline());
            }
            project.timelines.remove(index);
            let used: HashSet<String> = project
                .used_assets()
                .into_iter()
                .map(String::from)
                .collect();
            project.assets.retain(|asset| used.contains(&asset.id));
            if active == id {
                let neighbour = index.min(project.timelines.len() - 1);
                *active = project.timelines[neighbour].id.clone();
            }
            Ok(())
        })
    }

    /// Puts the timeline at index `to` of the order (clamped to the last). The active one stays.
    pub fn move_timeline(&mut self, id: &str, to: usize) -> Result<ProjectState> {
        self.change(|project, _| {
            let from = project
                .timelines
                .iter()
                .position(|timeline| timeline.id == id)
                .ok_or_else(|| unknown_timeline(id))?;
            let timeline = project.timelines.remove(from);
            project
                .timelines
                .insert(to.min(project.timelines.len()), timeline);
            Ok(())
        })
    }

    /// Makes the timeline after the active one the active one, as when a show ends and the next
    /// is made ready. `None` when the active one is the last: the show stays where it is.
    pub fn next_timeline(&mut self) -> Result<Option<ProjectState>> {
        self.step_timeline(1)
    }

    /// Makes the timeline `delta` places from the active one the active one. `None` when there is
    /// none there (the first has no previous, the last no next).
    pub fn step_timeline(&mut self, delta: isize) -> Result<Option<ProjectState>> {
        let Some(document) = &self.document else {
            return Err(ProjectError::Invalid("no project is open".into()));
        };
        let timelines = &document.project.timelines;
        let target = timelines
            .iter()
            .position(|timeline| timeline.id == document.active)
            .and_then(|at| at.checked_add_signed(delta))
            .and_then(|at| timelines.get(at))
            .map(|timeline| timeline.id.clone());
        target.map(|id| self.select_timeline(&id)).transpose()
    }

    /// Makes the timeline in this place, from 1, the active one. `None` when there is no such place.
    pub fn select_timeline_at(&mut self, place: usize) -> Result<Option<ProjectState>> {
        let Some(document) = &self.document else {
            return Err(ProjectError::Invalid("no project is open".into()));
        };
        let target = place
            .checked_sub(1)
            .and_then(|at| document.project.timelines.get(at))
            .map(|timeline| timeline.id.clone());
        target.map(|id| self.select_timeline(&id)).transpose()
    }

    /// Where the active timeline is among the project's, from 0, and how many there are, `None`
    /// before a document was started. The remote control lights its timeline pads from it.
    pub fn timeline_place(&self) -> Option<(usize, usize)> {
        let document = self.document.as_ref()?;
        let timelines = &document.project.timelines;
        let at = timelines
            .iter()
            .position(|timeline| timeline.id == document.active)?;
        Some((at, timelines.len()))
    }

    /// The frame rate and start of the active timeline, `None` before a document was started.
    pub fn active_timecode(&self) -> Option<TimecodeSettings> {
        let document = self.document.as_ref()?;
        let timeline = document
            .project
            .timeline(&document.active)
            .or(document.project.timelines.first())?;
        Some(timeline.timecode.clone())
    }

    pub fn select_timeline(&mut self, id: &str) -> Result<ProjectState> {
        self.change(|project, active| {
            timeline_mut(project, id)?;
            id.clone_into(active);
            Ok(())
        })
    }

    /// Sets the frame rate and the start of the active timeline.
    pub fn set_timecode(&mut self, frame_rate: FrameRate, start: u64) -> Result<ProjectState> {
        self.change(|project, active| {
            let timecode = &mut timeline_mut(project, active)?.timecode;
            timecode.frame_rate = frame_rate;
            timecode.start = start;
            Ok(())
        })
    }

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

    /// Renames a track or a clip, or changes a clip's fade curves or volume envelope (see [`ItemOp`]).
    pub fn item_edit(&mut self, op: ItemOp) -> Result<ProjectState> {
        self.change(|project, active| item_ops::apply(project, active, op))
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

    pub fn operator_edit(&mut self, op: crate::OperatorOp) -> Result<ProjectState> {
        self.change(|project, active| crate::operator::apply(project, active, op))
    }

    pub fn edit_audio_routing(
        &mut self,
        op: crate::routing::AudioRoutingOp,
    ) -> Result<ProjectState> {
        self.change(|project, active| crate::routing::apply(project, active, op))
    }

    pub fn edit_signal(&mut self, op: crate::SignalOp) -> Result<ProjectState> {
        self.change(|project, _| project.output.edit(op))
    }

    /// Applies `edit` to a copy of the project and the active id, and keeps the result only when it
    /// is still a valid project, so a refused change leaves the document as it was.
    pub fn execution_select(&mut self, id: &str) {
        if let Some(document) = &mut self.document
            && document.project.timelines.iter().any(|t| t.id == id)
        {
            document.active = id.into();
        }
    }

    pub fn ensure_editable(&self) -> Result<()> {
        if self
            .execution_lock
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(ProjectError::Invalid("show.error.locked".into()));
        }
        Ok(())
    }

    pub fn change(
        &mut self,
        edit: impl FnOnce(&mut Project, &mut String) -> Result<()>,
    ) -> Result<ProjectState> {
        self.ensure_editable()?;
        let document = self.document.as_mut().ok_or_else(no_project)?;
        let mut project = document.project.clone();
        let mut active = document.active.clone();
        edit(&mut project, &mut active)?;
        project.validate()?;
        document.project = project.clone();
        document.active.clone_from(&active);
        self.recovery_revision += 1;
        Ok(ProjectState { project, active })
    }

    /// The document being edited, if one was started.
    pub fn document(&self) -> Option<Document> {
        self.document.clone()
    }

    /// Registers an audio file from disk and returns the asset to add to the project.
    pub fn import_audio(&mut self, file: &Path) -> Result<AudioAsset> {
        let file = std::path::absolute(file)?;
        if !fs::metadata(&file)?.is_file() {
            return Err(ProjectError::Invalid(format!(
                "not a file: {}",
                file.display()
            )));
        }
        let file_name = file
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| ProjectError::Invalid("audio file name is not valid UTF-8".into()))?;

        let id = Uuid::new_v4().to_string();
        let asset = AudioAsset {
            name: file.file_stem().map_or_else(
                || file_name.to_owned(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
            file: format!("{AUDIO_DIR}{id}_{file_name}"),
            id,
            bpm: None,
        };
        check_asset_path(&asset.file)?;
        self.sources
            .insert(asset.id.clone(), AssetSource::Disk(file.to_path_buf()));
        self.recovery_revision += 1;
        Ok(asset)
    }

    /// Registers a durable speech job with a stable identity, so attaching it is idempotent.
    pub fn import_voice_job(&mut self, file: &Path, id: &str, name: &str) -> Result<AudioAsset> {
        if !id.starts_with("job-") || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(ProjectError::Invalid("invalid speech job".into()));
        }
        let path = std::path::absolute(file)?;
        if !path.is_file() {
            return Err(ProjectError::Invalid("missing speech audio".into()));
        }
        let asset = AudioAsset {
            id: id.into(),
            name: name.chars().take(64).collect(),
            file: format!(
                "audio/{id}.{}",
                file.extension()
                    .and_then(|e| e.to_str())
                    .ok_or_else(|| ProjectError::Invalid("missing recording format".into()))?
            ),
            bpm: None,
        };
        let source = AssetSource::Disk(path);
        self.sources.insert(id.into(), source.clone());
        self.voice_sources.insert(id.into(), source);
        Ok(asset)
    }

    /// Keeps a generated version readable across saves and undo, even after it leaves the archive.
    pub fn retain_voice_asset(&mut self, asset: &AudioAsset, directory: &Path) -> Result<()> {
        if self.voice_sources.contains_key(&asset.id) {
            return Ok(());
        }
        let source = match self.sources.get(&asset.id) {
            Some(source @ AssetSource::Disk(_)) => source.clone(),
            _ => {
                let bytes = self.asset_bytes(asset)?;
                fs::create_dir_all(directory)?;
                let path = directory.join(format!(
                    "{}.{}",
                    Uuid::new_v4(),
                    Path::new(&asset.file)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("mp3")
                ));
                fs::write(&path, bytes)?;
                AssetSource::Disk(path)
            }
        };
        self.sources.insert(asset.id.clone(), source.clone());
        self.voice_sources.insert(asset.id.clone(), source);
        Ok(())
    }

    /// The bytes of an asset, from wherever the session knows them to be.
    pub fn asset_bytes(&self, asset: &AudioAsset) -> Result<Vec<u8>> {
        match self.sources.get(&asset.id) {
            Some(AssetSource::Disk(path)) => Ok(fs::read(path)?),
            Some(AssetSource::Archive(path)) => {
                let (_, mut archive) = archive::open(path)?;
                let mut bytes = Vec::new();
                archive.asset_reader(&asset.file)?.read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            None => Err(ProjectError::MissingAsset(asset.id.clone())),
        }
    }

    /// The stored waveform of an asset, without decoding it: one generated in this session, or the one
    /// in the `.fxtc` the asset is read from if it was made from the same audio.
    pub fn preview(&self, asset: &AudioAsset) -> Result<Option<Preview>> {
        if let Some(stored) = self.previews.get(&asset.id) {
            return Ok(Some(stored.preview.clone()));
        }
        match self.sources.get(&asset.id) {
            Some(AssetSource::Archive(path)) => archive::read_preview(path, asset),
            _ => Ok(None),
        }
    }

    /// Keeps the waveform of the audio with this `stamp` for the next save.
    pub fn remember_preview(&mut self, asset: &AudioAsset, stamp: AudioStamp, preview: Preview) {
        self.previews
            .insert(asset.id.clone(), StoredPreview { stamp, preview });
    }

    /// Writes `project` to `path` and returns it as written, with the save recorded in its
    /// metadata. Afterwards every asset is read from that file.
    pub fn save(&mut self, project: &Project, path: &Path) -> Result<Project> {
        let path = std::path::absolute(path)?;
        let mut saved = project.clone();
        saved.format_version = FORMAT_VERSION;
        saved.meta.stamp();
        archive::save(&path, &saved, &self.sources, &self.previews)?;
        self.sources = archive_sources(&saved, &path);
        self.sources.extend(self.voice_sources.clone());
        // They are in the file now, which is where they are read from from here on.
        self.previews.clear();
        let mut document = Document::new(saved.clone(), Some(path.to_path_buf()));
        if let Some(previous) = &self.document {
            document.select(&previous.active);
        }
        self.document = Some(document);
        self.recovery_revision += 1;
        Ok(saved)
    }
}

fn no_project() -> ProjectError {
    ProjectError::Invalid("no project is open".into())
}

fn no_timeline() -> ProjectError {
    ProjectError::Invalid("a project needs a timeline".into())
}

fn unknown_timeline(id: &str) -> ProjectError {
    ProjectError::Invalid(format!("unknown timeline: {id}"))
}

fn audio_clips(timeline: &Timeline) -> impl Iterator<Item = &AudioClip> {
    timeline.tracks.iter().flat_map(|track| match &track.kind {
        TrackKind::Audio { clips } => clips.as_slice(),
        _ => &[],
    })
}

fn unknown_clip(id: &str) -> ProjectError {
    ProjectError::Invalid(format!("there is no clip {id}"))
}

fn timeline_mut<'a>(project: &'a mut Project, id: &str) -> Result<&'a mut Timeline> {
    project
        .timelines
        .iter_mut()
        .find(|timeline| timeline.id == id)
        .ok_or_else(|| unknown_timeline(id))
}

fn archive_sources(project: &Project, path: &Path) -> HashMap<String, AssetSource> {
    project
        .assets
        .iter()
        .map(|asset| (asset.id.clone(), AssetSource::Archive(path.to_path_buf())))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn delayed_sync_should_not_resurrect_a_closed_or_replaced_document() {
        let mut session = super::Session::default();
        let old = session.new_project("Old", flux_core::FrameRate::Fps25);
        session.clear();
        assert!(session.sync(old.clone()).is_err());
        assert!(session.document().is_none());
        let current = session.new_project("Current", flux_core::FrameRate::Fps25);
        assert!(session.sync(old).is_err());
        assert_eq!(session.document().unwrap().project, current);
    }

    #[test]
    fn imported_relative_sources_should_be_absolute_in_recovery() {
        let mut session = super::Session::default();
        let mut project = session.new_project("Show", flux_core::FrameRate::Fps25);
        project.assets.push(
            session
                .import_audio(std::path::Path::new("Cargo.toml"))
                .unwrap(),
        );
        session.update(project);
        let record = session.recovery().unwrap();
        record.validate().unwrap();
        assert!(record.sources.values().all(|source| match source {
            crate::AssetSource::Disk(path) | crate::AssetSource::Archive(path) =>
                path.is_absolute(),
        }));
    }

    #[test]
    fn autosave_options_should_validate_roundtrip_and_follow_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("show.fxtc");
        let mut session = super::Session::default();
        session.new_project("Show", flux_core::FrameRate::Fps25);
        for interval_seconds in [15, 30, 60] {
            let prefs = crate::Autosave {
                enabled: false,
                interval_seconds,
            };
            let state = session.set_autosave(prefs).unwrap();
            session.save(&state.project, &file).unwrap();
            session.new_project("Other", flux_core::FrameRate::Fps25);
            assert_eq!(
                session.document().unwrap().project.autosave,
                crate::Autosave::default()
            );
            assert_eq!(session.open(&file).unwrap().autosave, prefs);
        }
        let before = session.document();
        assert!(
            session
                .set_autosave(crate::Autosave {
                    enabled: true,
                    interval_seconds: 17
                })
                .is_err()
        );
        assert_eq!(session.document(), before);
    }

    use std::io::Read;

    use tempfile::TempDir;

    use super::*;
    use crate::model::{AudioClip, Track, TrackKind};

    const AUDIO_BYTES: &[u8] = b"RIFF....fake wav payload";

    /// A session with one imported audio file used by a clip in a new project.
    fn imported_project(dir: &TempDir) -> (Session, Project) {
        let wav = dir.path().join("click.wav");
        fs::write(&wav, AUDIO_BYTES).unwrap();

        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_audio(&wav).unwrap();
        project.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "t1".into(),
            name: "Backing".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Audio {
                clips: vec![AudioClip::new("c1", asset.id.clone(), 0)],
            },
        });
        project.assets.push(asset);
        (session, project)
    }

    fn read_asset(path: &Path, file: &str) -> Vec<u8> {
        let (_, mut archive) = archive::open(path).unwrap();
        let mut bytes = Vec::new();
        archive
            .asset_reader(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    #[test]
    fn signal_edit_should_preserve_document_on_invalid_level() {
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        session.edit_signal(crate::SignalOp::AddLtc).unwrap();
        let before = session.document().unwrap();
        assert!(
            session
                .edit_signal(crate::SignalOp::SetLtcLevel {
                    slot: 1,
                    text: "2".into()
                })
                .is_err()
        );
        assert_eq!(session.document().unwrap(), before);
    }

    #[test]
    fn document_should_be_none_until_a_project_is_started() {
        assert_eq!(Session::default().document(), None);
    }

    #[test]
    fn new_project_should_start_an_unsaved_document() {
        let mut session = Session::default();
        let project = session.new_project("Show", FrameRate::Fps25);

        let document = session.document().unwrap();
        assert_eq!(document.project, project);
        assert_eq!(document.baseline, project);
        assert_eq!(document.path, None);
    }

    #[test]
    fn reopening_after_closing_a_new_project_should_load_the_original_archive() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let path = dir.path().join("Show.fxtc");
        let saved = session.save(&project, &path).unwrap();
        let archive = fs::read(&path).unwrap();

        session.new_project("Untitled", FrameRate::Fps25);
        session.clear();

        // A fresh editor only reads the requested file when there is no session to restore.
        assert!(session.document().is_none());
        assert_eq!(session.open(&path).unwrap(), saved);
        assert_eq!(fs::read(&path).unwrap(), archive);
    }

    #[test]
    fn update_should_keep_the_latest_project_and_leave_the_baseline() {
        let mut session = Session::default();
        let started = session.new_project("Show", FrameRate::Fps25);
        let mut edited = started.clone();
        edited.name = "Edited".into();

        session.update(edited.clone());

        let document = session.document().unwrap();
        assert_eq!(document.project, edited);
        assert_eq!(document.baseline, started);
    }

    #[test]
    fn update_should_start_a_document_when_there_is_none() {
        let mut session = Session::default();
        let project = Project::new("Show", FrameRate::Fps25);

        session.update(project.clone());

        let document = session.document().unwrap();
        assert_eq!(
            (document.project, document.baseline),
            (project.clone(), project)
        );
    }

    #[test]
    fn save_should_make_the_saved_project_the_baseline_and_remember_the_path() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project) = imported_project(&dir);
        project.name = "Saved".into();
        let path = dir.path().join("show.fxtc");

        let saved = session.save(&project, &path).unwrap();

        let document = session.document().unwrap();
        assert_eq!(document.baseline, saved);
        assert_eq!(document.project, saved);
        assert_eq!(document.path, Some(path));
    }

    #[test]
    fn save_should_record_the_version_and_date_in_the_project_it_returns_and_writes() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project) = imported_project(&dir);
        project.meta.modified_at = "2000-01-01T00:00:00Z".into();
        let path = dir.path().join("show.fxtc");

        let saved = session.save(&project, &path).unwrap();

        assert_eq!(saved.meta.id, project.meta.id);
        assert_eq!(saved.meta.created_at, project.meta.created_at);
        assert_eq!(saved.meta.saved_with, crate::meta::APP_VERSION);
        assert_ne!(saved.meta.modified_at, "2000-01-01T00:00:00Z");
        assert_eq!(archive::open(&path).unwrap().0, saved);
    }

    #[test]
    fn open_should_leave_an_old_archive_unchanged_until_explicit_save() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r##"{ "formatVersion": 2, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
            "assets": [], "tracks": [], "output": { "ltc": null, "mtc": { "portName": "loopMIDI Port" } } }"##;
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("project.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, old.as_bytes()).unwrap();
        zip.finish().unwrap();

        let before = fs::read(&path).unwrap();
        let mut session = Session::default();
        let opened = session.open(&path).unwrap();
        assert!(!opened.meta.is_stamped());
        assert_eq!(fs::read(&path).unwrap(), before);
        session.save(&opened, &path).unwrap();
        let again = Session::default().open(&path).unwrap();
        assert!(again.meta.is_stamped());
        assert_eq!(again.meta.id, opened.meta.id);
        assert_eq!(again.output.mtc.len(), 1);
    }

    #[test]
    fn open_should_still_open_a_file_it_cannot_write_back() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r##"{ "formatVersion": 1, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
            "assets": [], "tracks": [], "output": {} }"##;
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("project.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, old.as_bytes()).unwrap();
        zip.finish().unwrap();
        // The save writes `old.fxtc.tmp` next to the file; a directory in its place makes it fail.
        fs::create_dir(dir.path().join("old.fxtc.tmp")).unwrap();

        let opened = Session::default().open(&path).unwrap();

        assert!(!opened.meta.is_stamped());
    }

    #[test]
    fn a_failed_save_should_leave_the_document_as_it_was() {
        let dir = TempDir::new().unwrap();
        let (_, project) = imported_project(&dir);
        let mut session = Session::default();
        session.update(Project::new("Other", FrameRate::Fps25));
        let before = session.document();

        assert!(
            session
                .save(&project, &dir.path().join("show.fxtc"))
                .is_err()
        );

        assert_eq!(session.document(), before);
    }

    #[test]
    fn open_should_start_a_document_at_the_opened_file() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let path = dir.path().join("show.fxtc");
        session.save(&project, &path).unwrap();

        let mut reopened = Session::default();
        let opened = reopened.open(&path).unwrap();

        let document = reopened.document().unwrap();
        assert_eq!(
            (document.project, document.baseline),
            (opened.clone(), opened)
        );
        assert_eq!(document.path, Some(path));
    }

    #[test]
    fn import_audio_should_name_the_asset_after_the_file_inside_the_audio_folder() {
        let dir = TempDir::new().unwrap();
        let (_, project) = imported_project(&dir);

        let asset = &project.assets[0];
        assert_eq!(asset.name, "click");
        assert!(asset.file.starts_with("audio/") && asset.file.ends_with("_click.wav"));
    }

    #[test]
    fn import_audio_should_give_distinct_ids_to_files_with_the_same_name() {
        let dir = TempDir::new().unwrap();
        let wav = dir.path().join("click.wav");
        fs::write(&wav, AUDIO_BYTES).unwrap();
        let mut session = Session::default();

        let first = session.import_audio(&wav).unwrap();
        let second = session.import_audio(&wav).unwrap();

        assert_ne!(first.id, second.id);
        assert_ne!(first.file, second.file);
    }

    #[test]
    fn import_audio_should_fail_for_a_missing_file() {
        let dir = TempDir::new().unwrap();

        let result = Session::default().import_audio(&dir.path().join("nope.wav"));

        assert!(matches!(result, Err(ProjectError::Io(_))));
    }

    #[test]
    fn import_audio_should_reject_a_directory() {
        let dir = TempDir::new().unwrap();

        let result = Session::default().import_audio(dir.path());

        assert!(matches!(result, Err(ProjectError::Invalid(_))));
    }

    #[test]
    fn save_then_open_should_round_trip_an_imported_project() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let path = dir.path().join("show.fxtc");

        let saved = session.save(&project, &path).unwrap();
        let opened = Session::default().open(&path).unwrap();

        assert_eq!(opened, saved);
        assert_eq!(read_asset(&path, &project.assets[0].file), AUDIO_BYTES);
    }

    #[test]
    fn save_should_keep_the_audio_when_saving_again_to_the_same_file() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project) = imported_project(&dir);
        let path = dir.path().join("show.fxtc");
        session.save(&project, &path).unwrap();

        project.name = "Renamed".into();
        session.save(&project, &path).unwrap();

        let opened = Session::default().open(&path).unwrap();
        assert_eq!(opened.name, "Renamed");
        assert_eq!(read_asset(&path, &project.assets[0].file), AUDIO_BYTES);
    }

    #[test]
    fn save_should_copy_legacy_project_and_audio_to_flx_without_changing_the_original() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let original = dir.path().join("show.fxtc");
        session.save(&project, &original).unwrap();
        let original_bytes = fs::read(&original).unwrap();

        let mut reopened = Session::default();
        let opened = reopened.open(&original).unwrap();
        let copy = dir.path().join("copy.flx");
        let saved = reopened.save(&opened, &copy).unwrap();

        assert_eq!(read_asset(&copy, &opened.assets[0].file), AUDIO_BYTES);
        assert_eq!(Session::default().open(&copy).unwrap(), saved);
        assert_eq!(fs::read(&original).unwrap(), original_bytes);
    }

    #[test]
    fn save_should_fail_for_an_asset_that_was_never_imported_or_opened() {
        let dir = TempDir::new().unwrap();
        let (_, project) = imported_project(&dir);
        let path = dir.path().join("show.fxtc");

        let result = Session::default().save(&project, &path);

        assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
        assert!(!path.exists());
    }

    #[test]
    fn asset_bytes_should_read_an_imported_file_and_then_the_saved_archive() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let asset = &project.assets[0];
        assert_eq!(session.asset_bytes(asset).unwrap(), AUDIO_BYTES);

        session
            .save(&project, &dir.path().join("show.fxtc"))
            .unwrap();

        assert_eq!(session.asset_bytes(asset).unwrap(), AUDIO_BYTES);
    }

    #[test]
    fn asset_bytes_should_fail_for_an_asset_the_session_never_saw() {
        let dir = TempDir::new().unwrap();
        let (_, project) = imported_project(&dir);

        let result = Session::default().asset_bytes(&project.assets[0]);

        assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
    }

    #[test]
    fn new_project_should_forget_the_assets_of_the_previous_project() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);

        session.new_project("Another", FrameRate::Fps30);
        let result = session.save(&project, &dir.path().join("show.fxtc"));

        assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
    }

    fn sample_preview() -> Preview {
        Preview {
            duration_seconds: 2.0,
            source_rate: 44_100,
            peaks_per_second: 100,
            peaks: vec![-5, 5],
        }
    }

    #[test]
    fn preview_should_survive_saving_as_another_file() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let asset = &project.assets[0];
        session.remember_preview(asset, AudioStamp::of(AUDIO_BYTES), sample_preview());
        assert_eq!(session.preview(asset).unwrap(), Some(sample_preview()));

        let original = dir.path().join("show.fxtc");
        session.save(&project, &original).unwrap();
        let mut reopened = Session::default();
        let opened = reopened.open(&original).unwrap();
        let copy = dir.path().join("copy.fxtc");
        reopened.save(&opened, &copy).unwrap();

        let mut from_copy = Session::default();
        from_copy.open(&copy).unwrap();
        assert_eq!(from_copy.preview(asset).unwrap(), Some(sample_preview()));
    }

    #[test]
    fn preview_should_be_dropped_when_the_audio_it_was_made_from_is_not_the_saved_audio() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let asset = &project.assets[0];
        session.remember_preview(asset, AudioStamp::of(b"other audio"), sample_preview());
        let path = dir.path().join("show.fxtc");

        session.save(&project, &path).unwrap();

        assert_eq!(session.preview(asset).unwrap(), None);
    }

    #[test]
    fn preview_should_be_none_for_an_imported_file_nobody_analysed() {
        let dir = TempDir::new().unwrap();
        let (session, project) = imported_project(&dir);

        assert_eq!(session.preview(&project.assets[0]).unwrap(), None);
    }

    /// A session holding a new 25 fps project that has a second timeline; the first is active.
    fn with_two_timelines() -> (Session, String, String) {
        let mut session = Session::default();
        let first = session.new_project("Show", FrameRate::Fps25).timelines[0]
            .id
            .clone();
        let second = session.add_timeline("Timeline").unwrap().active;
        session.select_timeline(&first).unwrap();
        (session, first, second)
    }

    #[test]
    fn new_project_should_activate_its_first_timeline() {
        let mut session = Session::default();
        let project = session.new_project("Show", FrameRate::Fps25);

        assert_eq!(session.active(), project.timelines[0].id);
    }

    #[test]
    fn add_timeline_should_number_it_and_activate_it() {
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);

        let state = session.add_timeline("Timeline").unwrap();

        assert_eq!(state.project.timelines[1].name, "Timeline 2");
        assert_eq!(state.active, state.project.timelines[1].id);
    }

    #[test]
    fn add_timeline_should_copy_the_frame_rate_of_the_active_one() {
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps30);

        let state = session.add_timeline("Timeline").unwrap();

        assert_eq!(
            state.project.timelines[1].timecode.frame_rate,
            FrameRate::Fps30
        );
    }

    #[test]
    fn add_timeline_should_start_the_new_one_at_zero_with_no_tracks() {
        let mut session = Session::default();
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].timecode.start = 90_000;
        session.update(project);

        let added = &session.add_timeline("Timeline").unwrap().project.timelines[1];

        assert_eq!((added.timecode.start, added.tracks.len()), (0, 0));
    }

    #[test]
    fn timeline_operations_should_fail_without_a_document() {
        let mut session = Session::default();

        assert!(session.add_timeline("Timeline").is_err());
        assert!(session.select_timeline("x").is_err());
    }

    #[test]
    fn rename_timeline_should_trim_the_name() {
        let (mut session, first, _) = with_two_timelines();

        let state = session.rename_timeline(&first, "  Song 1 ").unwrap();

        assert_eq!(state.project.timelines[0].name, "Song 1");
    }

    #[test]
    fn rename_timeline_should_refuse_an_empty_name() {
        let (mut session, first, _) = with_two_timelines();

        assert!(matches!(
            session.rename_timeline(&first, "   "),
            Err(ProjectError::Invalid(_))
        ));
    }

    #[test]
    fn rename_timeline_should_refuse_an_unknown_id() {
        let (mut session, ..) = with_two_timelines();

        assert!(session.rename_timeline("nope", "Song").is_err());
    }

    #[test]
    fn select_timeline_should_change_the_active_one() {
        let (mut session, _, second) = with_two_timelines();

        let state = session.select_timeline(&second).unwrap();

        assert_eq!((state.active, session.active()), (second.clone(), second));
    }

    #[test]
    fn select_timeline_should_refuse_an_unknown_id_and_keep_the_active_one() {
        let (mut session, first, _) = with_two_timelines();

        assert!(session.select_timeline("nope").is_err());
        assert_eq!(session.active(), first);
    }

    #[test]
    fn remove_timeline_should_refuse_the_last_one() {
        let mut session = Session::default();
        let only = session.new_project("Show", FrameRate::Fps25).timelines[0]
            .id
            .clone();

        assert!(session.remove_timeline(&only).is_err());
        assert_eq!(session.document().unwrap().project.timelines.len(), 1);
    }

    #[test]
    fn remove_timeline_should_activate_the_one_that_takes_its_place() {
        let (mut session, first, second) = with_two_timelines();

        let state = session.remove_timeline(&first).unwrap();

        assert_eq!(state.active, second);
    }

    #[test]
    fn remove_timeline_should_keep_the_active_one_when_another_goes() {
        let (mut session, first, second) = with_two_timelines();

        let state = session.remove_timeline(&second).unwrap();

        assert_eq!(state.active, first);
    }

    #[test]
    fn remove_timeline_should_drop_the_audio_no_clip_uses_any_more() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let first = project.timelines[0].id.clone();
        session.update(project);
        session.add_timeline("Timeline").unwrap();

        let state = session.remove_timeline(&first).unwrap();

        assert!(state.project.assets.is_empty());
    }

    #[test]
    fn remove_timeline_should_keep_the_audio_another_timeline_uses() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project) = imported_project(&dir);
        let first = project.timelines[0].id.clone();
        let mut second = Timeline::new("Second", FrameRate::Fps25);
        let mut track = project.timelines[0].tracks[0].clone();
        track.id = "t2".into();
        if let TrackKind::Audio { clips } = &mut track.kind {
            clips[0].id = "c2".into();
        }
        second.tracks.push(track);
        project.timelines.push(second);
        session.update(project);

        let state = session.remove_timeline(&first).unwrap();

        assert_eq!(state.project.assets.len(), 1);
    }

    #[test]
    fn set_timecode_should_change_the_active_timeline_only() {
        let (mut session, first, second) = with_two_timelines();
        session.select_timeline(&second).unwrap();

        let state = session.set_timecode(FrameRate::Fps24, 86_400).unwrap();

        let timecode = |id: &str| state.project.timeline(id).unwrap().timecode.clone();
        assert_eq!(
            (timecode(&second).frame_rate, timecode(&second).start),
            (FrameRate::Fps24, 86_400)
        );
        assert_eq!(timecode(&first).frame_rate, FrameRate::Fps25);
    }

    #[test]
    fn update_should_fall_back_to_the_first_timeline_when_the_active_one_is_gone() {
        let (mut session, first, second) = with_two_timelines();
        session.select_timeline(&second).unwrap();
        let mut project = session.document().unwrap().project;
        project.timelines.retain(|timeline| timeline.id == first);

        session.update(project);

        assert_eq!(session.active(), first);
    }

    #[test]
    fn save_should_keep_the_active_timeline() {
        let dir = TempDir::new().unwrap();
        let (mut session, _, second) = with_two_timelines();
        session.select_timeline(&second).unwrap();
        let project = session.document().unwrap().project;

        session
            .save(&project, &dir.path().join("show.fxtc"))
            .unwrap();

        assert_eq!(session.active(), second);
    }

    #[test]
    fn active_timeline_should_follow_the_selection_in_the_project_it_is_given() {
        let (mut session, _, second) = with_two_timelines();
        let project = session.document().unwrap().project;
        session.select_timeline(&second).unwrap();

        let timeline = session.active_timeline(&project).unwrap();

        assert_eq!(timeline.id, second);
    }

    #[test]
    fn active_timeline_should_fall_back_to_the_first_when_the_project_lacks_it() {
        let (session, first, _) = with_two_timelines();
        let mut project = session.document().unwrap().project;
        project.timelines.reverse();
        project.timelines.retain(|timeline| timeline.id == first);

        assert_eq!(session.active_timeline(&project).unwrap().id, first);
    }

    #[test]
    fn next_timeline_should_activate_the_one_after_the_active_one() {
        let (mut session, _, second) = with_two_timelines();

        let state = session.next_timeline().unwrap().unwrap();

        assert_eq!(state.active, second);
    }

    #[test]
    fn next_timeline_should_stay_on_the_last_one() {
        let (mut session, _, second) = with_two_timelines();
        session.select_timeline(&second).unwrap();

        assert_eq!(session.next_timeline().unwrap(), None);
        assert_eq!(session.active(), second);
    }

    #[test]
    fn next_timeline_should_fail_without_a_document() {
        assert!(Session::default().next_timeline().is_err());
    }

    #[test]
    fn step_timeline_should_go_back_and_stop_at_both_ends() {
        let (mut session, first, second) = with_two_timelines();

        assert_eq!(
            session.step_timeline(-1).unwrap(),
            None,
            "no one before the first"
        );
        assert_eq!(session.step_timeline(1).unwrap().unwrap().active, second);
        assert_eq!(
            session.step_timeline(1).unwrap(),
            None,
            "no one after the last"
        );
        assert_eq!(session.step_timeline(-1).unwrap().unwrap().active, first);
        assert_eq!(session.step_timeline(-5).unwrap(), None);
    }

    #[test]
    fn select_timeline_at_should_count_places_from_one() {
        let (mut session, first, second) = with_two_timelines();

        assert_eq!(
            session.select_timeline_at(2).unwrap().unwrap().active,
            second
        );
        assert_eq!(
            session.select_timeline_at(1).unwrap().unwrap().active,
            first
        );
        assert_eq!(session.select_timeline_at(0).unwrap(), None);
        assert_eq!(session.select_timeline_at(3).unwrap(), None);
        assert_eq!(session.active(), first);
    }

    #[test]
    fn step_and_select_should_fail_without_a_document() {
        assert!(Session::default().step_timeline(1).is_err());
        assert!(Session::default().select_timeline_at(1).is_err());
        assert_eq!(Session::default().active_timecode(), None);
    }

    #[test]
    fn active_timecode_should_be_the_one_of_the_active_timeline() {
        let (mut session, _, _) = with_two_timelines();
        let state = session.set_timecode(FrameRate::Fps30, 500).unwrap();

        let timecode = session.active_timecode().unwrap();

        assert_eq!(timecode.frame_rate, FrameRate::Fps30);
        assert_eq!(timecode.start, 500);
        assert_eq!(
            state.project.timeline(&state.active).unwrap().timecode,
            timecode
        );
    }

    fn order(state: &ProjectState) -> Vec<&str> {
        state
            .project
            .timelines
            .iter()
            .map(|timeline| timeline.id.as_str())
            .collect()
    }

    #[test]
    fn move_timeline_should_put_it_at_the_new_index() {
        let (mut session, first, second) = with_two_timelines();

        let state = session.move_timeline(&first, 1).unwrap();

        assert_eq!(order(&state), [second.as_str(), first.as_str()]);
    }

    #[test]
    fn move_timeline_should_clamp_the_index_to_the_last_place() {
        let (mut session, first, second) = with_two_timelines();

        let state = session.move_timeline(&first, 99).unwrap();

        assert_eq!(order(&state), [second.as_str(), first.as_str()]);
    }

    #[test]
    fn move_timeline_should_leave_the_order_when_it_is_already_there() {
        let (mut session, first, second) = with_two_timelines();

        let state = session.move_timeline(&first, 0).unwrap();

        assert_eq!(order(&state), [first.as_str(), second.as_str()]);
    }

    #[test]
    fn move_timeline_should_keep_the_active_timeline() {
        let (mut session, first, _) = with_two_timelines();

        let state = session.move_timeline(&first, 1).unwrap();

        assert_eq!(state.active, first);
    }

    #[test]
    fn move_timeline_should_refuse_an_unknown_id() {
        let (mut session, ..) = with_two_timelines();

        assert!(session.move_timeline("nope", 0).is_err());
    }
}
