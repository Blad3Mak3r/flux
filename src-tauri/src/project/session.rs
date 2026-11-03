use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use uuid::Uuid;

use super::archive::{self, AssetSource};
use super::error::{ProjectError, Result};
use super::model::{AUDIO_DIR, AudioAsset, FrameRate, Project, check_asset_path};

/// Backend state for the project being edited.
///
/// The frontend owns the `Project` value and sends it back on save; the session only remembers
/// where the bytes of each asset live: in the `.fxtc` it was opened or saved from, or in a file
/// imported from disk that is not in any archive yet.
#[derive(Debug, Default)]
pub struct Session {
    sources: HashMap<String, AssetSource>,
}

impl Session {
    pub fn new_project(&mut self, name: &str, frame_rate: FrameRate) -> Project {
        self.sources.clear();
        Project::new(name, frame_rate)
    }

    pub fn open(&mut self, path: &Path) -> Result<Project> {
        let (project, _archive) = archive::open(path)?;
        self.sources = archive_sources(&project, path);
        Ok(project)
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

    /// Writes `project` to `path`. Afterwards every asset is read from that file.
    pub fn save(&mut self, project: &Project, path: &Path) -> Result<()> {
        archive::save(path, project, &self.sources)?;
        self.sources = archive_sources(project, path);
        Ok(())
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

        session.save(&project, &path).unwrap();
        let opened = Session::default().open(&path).unwrap();

        assert_eq!(opened, project);
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
}
