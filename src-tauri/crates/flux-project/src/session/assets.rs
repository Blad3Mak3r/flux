//! Reading the bytes of the assets, the media list and the stored waveforms.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{ProjectState, Session, no_project};
use crate::archive::{self, AssetSource};
use crate::error::{ProjectError, Result};
use crate::media;
use crate::model::{AudioAsset, VIDEO_DIR, VideoAsset};
use crate::preview::{AudioStamp, Preview, StoredPreview};

impl Session {
    /// The bytes of an asset, from wherever the session knows them to be.
    pub fn asset_bytes(&self, asset: &AudioAsset) -> Result<Vec<u8>> {
        if let Some(link) = &asset.link {
            return fs::read(&link.path).map_err(|_| ProjectError::MissingAsset(link.path.clone()));
        }
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

    /// A file FFmpeg can open for a video: the imported file itself, or the entry of the archive it
    /// lives in, copied once into `cache` (named by the asset's id, which never changes its bytes).
    pub fn video_path(&self, asset: &VideoAsset, cache: &Path) -> Result<PathBuf> {
        if let Some(link) = &asset.link {
            // Offline media is an error for this one video, which its caller shows as offline.
            return Path::new(&link.path)
                .is_file()
                .then(|| PathBuf::from(&link.path))
                .ok_or_else(|| ProjectError::MissingAsset(link.path.clone()));
        }
        match self.sources.get(&asset.id) {
            Some(AssetSource::Disk(path)) => Ok(path.clone()),
            Some(AssetSource::Archive(path)) => {
                let name = asset.file.strip_prefix(VIDEO_DIR).unwrap_or(&asset.file);
                let target = cache.join(name);
                if target.is_file() {
                    return Ok(target);
                }
                fs::create_dir_all(cache)?;
                let (_, mut archive) = archive::open(path)?;
                // Written aside first, so a copy cut short is never taken for the video.
                let partial = cache.join(format!("{name}.part"));
                std::io::copy(
                    &mut archive.video_reader(&asset.file)?,
                    &mut fs::File::create(&partial)?,
                )?;
                fs::rename(&partial, &target)?;
                Ok(target)
            }
            None => Err(ProjectError::MissingAsset(asset.id.clone())),
        }
    }

    /// Like [`Self::video_path`], for a video of the document known by its id.
    pub fn video_file(&self, id: &str, cache: &Path) -> Result<PathBuf> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let asset = document
            .project
            .videos
            .iter()
            .find(|video| video.id == id)
            .ok_or_else(|| ProjectError::MissingAsset(id.to_owned()))?;
        self.video_path(asset, cache)
    }

    /// The media of the document, linked or embedded, and which of them are offline.
    pub fn media_list(&self) -> media::MediaList {
        self.document.as_ref().map_or(
            media::MediaList {
                items: Vec::new(),
                missing: 0,
            },
            |d| media::list(&d.project),
        )
    }

    /// Links the media `id` to the file at `file`, and the other offline media found beside it.
    /// Answers with how many it linked.
    pub fn media_relink(&mut self, id: &str, file: &Path) -> Result<(ProjectState, usize)> {
        let mut count = 0;
        let state = self.change(|project, _| {
            count = media::relink(project, id, file)?;
            Ok(())
        })?;
        Ok((state, count))
    }

    /// Looks for the offline media in `folder` and under it. Answers with how many it found and how
    /// many are still offline.
    pub fn media_search(&mut self, folder: &Path) -> Result<(ProjectState, usize, usize)> {
        let (mut found, mut left) = (0, 0);
        let state = self.change(|project, _| {
            (found, left) = media::search_folder(project, folder)?;
            Ok(())
        })?;
        Ok((state, found, left))
    }

    /// What collecting the media would copy, for [`media::CollectPlan::run`] to do without the
    /// session. The project must be saved: the media goes beside its file.
    pub fn media_collect_plan(&self) -> Result<media::CollectPlan> {
        let document = self.document.as_ref().ok_or_else(no_project)?;
        let flx = document
            .path
            .as_deref()
            .ok_or_else(|| ProjectError::Invalid("media.error.unsaved".into()))?;
        media::plan_collect(
            &document.project,
            flx,
            |id| self.sources.get(id).cloned(),
            archive::entry_size,
        )
    }

    /// Links the media to the files [`media::CollectPlan::run`] made. The project is changed, so it
    /// must be saved to keep them; that save leaves the media out of the archive.
    pub fn media_collect_apply(&mut self, placed: &[(String, PathBuf)]) -> Result<ProjectState> {
        let flx = self.path().map(Path::to_path_buf);
        let embedded: HashSet<String> = self
            .document
            .iter()
            .flat_map(|d| {
                let audio = d.project.assets.iter().filter(|a| a.link.is_none());
                let video = d.project.videos.iter().filter(|v| v.link.is_none());
                audio
                    .map(|a| a.id.clone())
                    .chain(video.map(|v| v.id.clone()))
            })
            .collect();
        let state = self.change(|project, _| {
            media::apply_collected(project, placed);
            if let Some(flx) = &flx {
                media::relativize(project, flx);
            }
            Ok(())
        })?;
        // An undo can bring the embedded asset back: it re-embeds from the collected copy.
        for (id, dest) in placed.iter().filter(|(id, _)| embedded.contains(id)) {
            self.sources
                .insert(id.clone(), AssetSource::Disk(dest.clone()));
        }
        Ok(state)
    }

    /// The stored waveform of an asset, without decoding it: one generated in this session, or the one
    /// in the `.fxtc` the asset is read from if it was made from the same audio.
    pub fn preview(&self, asset: &AudioAsset) -> Result<Option<Preview>> {
        if let Some(stored) = self.previews.get(&asset.id) {
            return Ok(Some(stored.preview.clone()));
        }
        if asset.link.is_some() {
            // Linked audio has no entry in the archive, so its stamp is the file's own.
            let (Some(stamp), Some(path)) = (self.stamp_of(asset, None), self.path()) else {
                return Ok(None);
            };
            return archive::read_preview_of(path, asset, Some(stamp)).or(Ok(None));
        }
        match self.sources.get(&asset.id) {
            Some(AssetSource::Archive(path)) => archive::read_preview(path, asset),
            _ => Ok(None),
        }
    }

    /// The file the document was opened from or saved to.
    pub(super) fn path(&self) -> Option<&Path> {
        self.document.as_ref()?.path.as_deref()
    }

    /// What identifies the audio a preview was made from: the size and CRC of its bytes, or for
    /// linked audio (read without the bytes) its size and modification time. `None` when a linked
    /// file is gone.
    pub fn stamp_of(&self, asset: &AudioAsset, bytes: Option<&[u8]>) -> Option<AudioStamp> {
        let Some(link) = &asset.link else {
            return bytes.map(AudioStamp::of);
        };
        let meta = fs::metadata(&link.path).ok()?;
        let seconds = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs());
        Some(AudioStamp {
            size: meta.len(),
            crc32: crc32fast::hash(&seconds.to_le_bytes()),
        })
    }

    /// Keeps the waveform of the audio with this `stamp` for the next save.
    pub fn remember_preview(&mut self, asset: &AudioAsset, stamp: AudioStamp, preview: Preview) {
        self.previews
            .insert(asset.id.clone(), StoredPreview { stamp, preview });
    }
}
