use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use super::archive::{self, AssetSource};
use super::error::{ProjectError, Result};
use super::model::{AUDIO_DIR, AudioAsset, FORMAT_VERSION, FrameRate, Project, check_asset_path};
use super::preview::{AudioStamp, Preview, StoredPreview};

/// The document being edited, as the backend last heard of it. The window that edits it can be
/// reloaded, and picks it up from here again.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub project: Project,
    /// Where the `.fxtc` lives, or `None` for a project that was never saved.
    pub path: Option<PathBuf>,
    /// The project as it was last opened or saved, or as it started for a new one: the document is
    /// unsaved when `project` differs from it.
    pub baseline: Project,
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
    document: Option<Document>,
    sources: HashMap<String, AssetSource>,
    previews: HashMap<String, StoredPreview>,
}

impl Session {
    pub fn new_project(&mut self, name: &str, frame_rate: FrameRate) -> Project {
        self.sources.clear();
        self.previews.clear();
        let project = Project::new(name, frame_rate);
        self.document = Some(Document {
            baseline: project.clone(),
            project: project.clone(),
            path: None,
        });
        project
    }

    /// Opens a `.fxtc`. A file written before projects carried metadata got an id while opening,
    /// and is saved right away so that the id is the one it keeps; when it cannot be written (a
    /// read-only disk) the project is still opened, and the next save does it.
    pub fn open(&mut self, path: &Path) -> Result<Project> {
        let (project, _archive) = archive::open(path)?;
        self.sources = archive_sources(&project, path);
        self.previews.clear();
        self.document = Some(Document {
            baseline: project.clone(),
            project: project.clone(),
            path: Some(path.to_path_buf()),
        });
        if !project.meta.is_stamped()
            && let Ok(saved) = self.save(&project, path)
        {
            return Ok(saved);
        }
        Ok(project)
    }

    /// Remembers the latest state of the project being edited.
    pub fn update(&mut self, project: Project) {
        match &mut self.document {
            Some(document) => document.project = project,
            None => {
                self.document = Some(Document {
                    baseline: project.clone(),
                    project,
                    path: None,
                });
            }
        }
    }

    /// The document being edited, if one was started.
    pub fn document(&self) -> Option<Document> {
        self.document.clone()
    }

    /// Registers an audio file from disk and returns the asset to add to the project.
    pub fn import_audio(&mut self, file: &Path) -> Result<AudioAsset> {
        if !fs::metadata(file)?.is_file() {
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
        };
        check_asset_path(&asset.file)?;
        self.sources
            .insert(asset.id.clone(), AssetSource::Disk(file.to_path_buf()));
        Ok(asset)
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
        let mut saved = project.clone();
        saved.format_version = FORMAT_VERSION;
        saved.meta.stamp();
        archive::save(path, &saved, &self.sources, &self.previews)?;
        self.sources = archive_sources(&saved, path);
        // They are in the file now, which is where they are read from from here on.
        self.previews.clear();
        self.document = Some(Document {
            project: saved.clone(),
            path: Some(path.to_path_buf()),
            baseline: saved.clone(),
        });
        Ok(saved)
    }
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
    use std::io::Read;

    use tempfile::TempDir;

    use super::*;
    use crate::project::model::{AudioClip, Track, TrackKind};

    const AUDIO_BYTES: &[u8] = b"RIFF....fake wav payload";

    /// A session with one imported audio file used by a clip in a new project.
    fn imported_project(dir: &TempDir) -> (Session, Project) {
        let wav = dir.path().join("click.wav");
        fs::write(&wav, AUDIO_BYTES).unwrap();

        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_audio(&wav).unwrap();
        project.tracks.push(Track {
            id: "t1".into(),
            name: "Backing".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Audio {
                clips: vec![AudioClip {
                    id: "c1".into(),
                    asset: asset.id.clone(),
                    start: 0,
                    gain_db: 0.0,
                }],
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
        assert_eq!(saved.meta.saved_with, crate::project::meta::APP_VERSION);
        assert_ne!(saved.meta.modified_at, "2000-01-01T00:00:00Z");
        assert_eq!(archive::open(&path).unwrap().0, saved);
    }

    #[test]
    fn open_should_save_a_file_without_meta_so_it_keeps_the_id_it_was_given() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r##"{ "formatVersion": 2, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
            "assets": [], "tracks": [], "output": { "ltc": null, "mtc": { "portName": "loopMIDI Port" } } }"##;
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file("project.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, old.as_bytes()).unwrap();
        zip.finish().unwrap();

        let opened = Session::default().open(&path).unwrap();
        let again = Session::default().open(&path).unwrap();

        assert!(opened.meta.is_stamped());
        assert_eq!(again.meta, opened.meta);
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
    fn save_should_copy_audio_from_the_opened_file_when_saving_as_another_file() {
        let dir = TempDir::new().unwrap();
        let (mut session, project) = imported_project(&dir);
        let original = dir.path().join("show.fxtc");
        session.save(&project, &original).unwrap();

        let mut reopened = Session::default();
        let opened = reopened.open(&original).unwrap();
        let copy = dir.path().join("copy.fxtc");
        reopened.save(&opened, &copy).unwrap();

        assert_eq!(read_asset(&copy, &opened.assets[0].file), AUDIO_BYTES);
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
}
