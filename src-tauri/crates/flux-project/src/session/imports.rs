//! Importing, dropping and replacing the media of the project.

use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{ProjectState, Session, no_project, timeline_mut};
use crate::archive::AssetSource;
use crate::dropped::{self, Dropped, MediaKind, Staged};
use crate::error::{ProjectError, Result};
use crate::model::{
    AUDIO_DIR, AudioAsset, MediaLink, Track, TrackKind, VIDEO_DIR, VideoAsset, VideoClip,
    check_asset_path, check_video_path,
};

/// The absolute path of `file`, which must be a file that exists.
fn existing_file(file: &Path) -> Result<PathBuf> {
    let file = std::path::absolute(file)?;
    if !fs::metadata(&file)?.is_file() {
        return Err(ProjectError::Invalid(format!(
            "not a file: {}",
            file.display()
        )));
    }
    Ok(file)
}

/// A new video asset linked to the file at `file`: its bytes stay where they are.
fn video_asset(file: &Path) -> Result<VideoAsset> {
    let file = existing_file(file)?;
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ProjectError::Invalid("video file name is not valid UTF-8".into()))?;
    let id = Uuid::new_v4().to_string();
    let asset = VideoAsset {
        name: file.file_stem().map_or_else(
            || file_name.to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        ),
        file: format!("{VIDEO_DIR}{id}_{file_name}"),
        link: Some(MediaLink {
            path: file.to_string_lossy().into_owned(),
            relative: None,
        }),
        id,
    };
    Ok(asset)
}

impl Session {
    /// Links an audio file from disk and returns the asset to add to the project; the file stays
    /// where it is.
    pub fn import_audio(&mut self, file: &Path) -> Result<AudioAsset> {
        self.ensure_editable()?;
        let file = existing_file(file)?;
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
            link: Some(MediaLink {
                path: file.to_string_lossy().into_owned(),
                relative: None,
            }),
            id,
            bpm: None,
        };
        check_asset_path(&asset.file)?;
        self.recovery_revision += 1;
        Ok(asset)
    }

    /// Like [`Self::import_audio`] but the asset is kept inside the project, as a legacy one is.
    #[cfg(test)]
    pub(crate) fn import_embedded(&mut self, file: &Path) -> Result<AudioAsset> {
        let mut asset = self.import_audio(file)?;
        asset.link = None;
        self.sources.insert(
            asset.id.clone(),
            AssetSource::Disk(std::path::absolute(file)?),
        );
        Ok(asset)
    }

    /// Adds a video file from disk to the project, on a new video track of the active timeline with a
    /// clip at `at` (frames from the timeline start). The caller has checked that it is a video.
    pub fn import_video(&mut self, file: &Path, at: u64) -> Result<ProjectState> {
        let asset = video_asset(file)?;
        let track = Track {
            audio_output: Default::default(),
            id: Uuid::new_v4().to_string(),
            name: asset.name.clone(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Video {
                clips: vec![VideoClip {
                    id: Uuid::new_v4().to_string(),
                    asset: asset.id.clone(),
                    start: at,
                    name: None,
                    offset: 0,
                    length: None,
                    fit: Default::default(),
                    fade_in: 0,
                    fade_out: 0,
                }],
            },
        };
        self.change(|project, active| {
            timeline_mut(project, active)?.tracks.push(track);
            project.videos.push(asset);
            Ok(())
        })
    }

    /// Adds a video file from disk to the project, in the cell of the VJ grid at `layer` and
    /// `column`, in place of the clip that was there; with no `column`, in the first empty cell of
    /// the layer in the deck shown. The caller has checked that it is a video.
    pub fn import_vj_video(
        &mut self,
        file: &Path,
        layer: &str,
        column: Option<&str>,
    ) -> Result<ProjectState> {
        let asset = video_asset(file)?;
        self.change(|project, _| {
            let column = match column {
                Some(column) => column.to_owned(),
                None => project.vj.empty_column(layer).ok_or_else(|| {
                    ProjectError::Invalid("the layer has no empty cell in this deck".into())
                })?,
            };
            project.videos.push(asset.clone());
            let op = crate::VjOp::PlaceVideo {
                asset: asset.id.clone(),
                layer: layer.to_owned(),
                column,
            };
            crate::vj::apply(project, op, || Uuid::new_v4().to_string())
        })
    }

    /// Registers a file dragged in from the file explorer, to be placed by [`Session::drop_on_timeline`].
    pub fn stage_drop(&mut self, file: &Path, kind: MediaKind) -> Result<Dropped> {
        match kind {
            MediaKind::Audio => self.import_audio(file).map(Dropped::Audio),
            MediaKind::Video | MediaKind::Image => video_asset(file).map(Dropped::Video),
        }
    }

    /// Puts staged files on the active timeline one after another from frame `at` (see
    /// [`crate::dropped`]): on the track `track` when it takes them, else on a new track.
    pub fn drop_on_timeline(
        &mut self,
        files: Vec<Staged>,
        track: Option<&str>,
        at: u64,
    ) -> Result<ProjectState> {
        self.change(|project, active| {
            for file in &files {
                match &file.media {
                    Dropped::Audio(asset) => project.assets.push(asset.clone()),
                    Dropped::Video(asset) => project.videos.push(asset.clone()),
                }
            }
            let timeline = timeline_mut(project, active)?;
            dropped::place(timeline, files, track, at, || Uuid::new_v4().to_string())
        })
    }

    /// Puts video files in the cells of `layer` from `column` to the right in its deck, each in
    /// place of the clip there; the files that find no cell are left out. The answer is the project
    /// and how many were placed. The caller has checked that they are videos.
    pub fn drop_on_grid(
        &mut self,
        files: &[PathBuf],
        layer: &str,
        column: &str,
    ) -> Result<(ProjectState, usize)> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let columns: Vec<String> = document
            .project
            .vj
            .decks
            .iter()
            .find(|deck| deck.columns.iter().any(|c| c.id == column))
            .map(|deck| {
                deck.columns
                    .iter()
                    .skip_while(|c| c.id != column)
                    .map(|c| c.id.clone())
                    .collect()
            })
            .ok_or_else(|| ProjectError::Invalid(format!("unknown column {column}")))?;
        let mut placed = 0;
        let mut state = None;
        for (file, column) in files.iter().zip(&columns) {
            state = Some(self.import_vj_video(file, layer, Some(column))?);
            placed += 1;
        }
        let state = match state {
            Some(state) => state,
            None => self.change(|_, _| Ok(()))?,
        };
        Ok((state, placed))
    }

    /// Adds an image file from disk to the project's media, placed nowhere: the test card's logo
    /// uses it. The caller has checked that FFmpeg reads it.
    pub fn import_media(&mut self, file: &Path) -> Result<ProjectState> {
        let asset = video_asset(file)?;
        self.change(|project, _| {
            project.videos.push(asset);
            Ok(())
        })
    }

    /// Puts the video file at `file` (a converted copy, such as a HAP one) in place of the video
    /// `asset`: it becomes a new asset, in the same place of the project's list and under the same
    /// name, and every clip of every timeline that showed the old one shows the new one, keeping
    /// its place, trim and settings. The old file stays known, so undo brings it back; a save
    /// leaves it out once nothing uses it. The new file is linked, or with `link` off kept inside
    /// the project (a copy made from a video that was).
    pub fn replace_video(&mut self, asset: &str, file: &Path, link: bool) -> Result<ProjectState> {
        let file = existing_file(file)?;
        let extension = file
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| ProjectError::Invalid("video file has no extension".into()))?;
        let id = Uuid::new_v4().to_string();
        let link_to = link.then(|| MediaLink {
            path: file.to_string_lossy().into_owned(),
            relative: None,
        });
        let state = self.change(|project, _| {
            let slot = project
                .videos
                .iter_mut()
                .find(|video| video.id == asset)
                .ok_or_else(|| ProjectError::MissingAsset(asset.to_owned()))?;
            let stem: String = slot
                .file
                .strip_prefix(VIDEO_DIR)
                .and_then(|name| name.split_once('_').map(|(_, rest)| rest))
                .map(|name| {
                    Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(name)
                })
                .unwrap_or(&slot.name)
                .to_owned();
            *slot = VideoAsset {
                id: id.clone(),
                name: slot.name.clone(),
                file: format!("{VIDEO_DIR}{id}_{stem}.{extension}"),
                link: link_to,
            };
            check_video_path(&slot.file)?;
            for timeline in &mut project.timelines {
                for track in &mut timeline.tracks {
                    if let TrackKind::Video { clips } = &mut track.kind {
                        for clip in clips.iter_mut().filter(|clip| clip.asset == asset) {
                            clip.asset.clone_from(&id);
                        }
                    }
                }
            }
            let grid = project
                .vj
                .layers
                .iter_mut()
                .flat_map(|layer| &mut layer.clips);
            for clip in grid.filter(|clip| clip.asset == asset) {
                clip.asset.clone_from(&id);
            }
            Ok(())
        })?;
        if !link {
            self.sources.insert(id, AssetSource::Disk(file));
        }
        Ok(state)
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
            link: None,
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
}
