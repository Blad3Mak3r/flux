use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::error::{ProjectError, Result};
use super::model::{FORMAT_VERSION, Project, check_asset_path};

const PROJECT_JSON: &str = "project.json";

type FileArchive = ZipArchive<BufReader<File>>;

/// Where `save` takes the bytes of an asset from.
#[derive(Debug, Clone)]
pub enum AssetSource {
    /// A newly imported file on disk.
    Disk(PathBuf),
    /// An existing `.fxtc`; the entry named like the asset's `file` is copied without recompressing.
    Archive(PathBuf),
}

/// An opened `.fxtc`. Audio is streamed from the archive entry by entry, never loaded whole.
pub struct ProjectArchive {
    archive: FileArchive,
}

impl ProjectArchive {
    /// Streams the audio entry stored at `file` (an asset's archive path).
    pub fn asset_reader(&mut self, file: &str) -> Result<impl Read + '_> {
        check_asset_path(file)?;
        by_name(&mut self.archive, file)
    }
}

/// Opens a `.fxtc`, validating `project.json`, the archive entry names and the presence of every asset.
pub fn open(path: &Path) -> Result<(Project, ProjectArchive)> {
    let mut archive = open_archive(path)?;

    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.enclosed_name().is_none() {
            let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
            return Err(ProjectError::InvalidAssetPath(name));
        }
    }

    let value: serde_json::Value = match archive.by_name(PROJECT_JSON) {
        Ok(entry) => serde_json::from_reader(entry)?,
        Err(ZipError::FileNotFound) => return Err(ProjectError::MissingProjectJson),
        Err(err) => return Err(err.into()),
    };

    // Check the version before the full parse: a newer file may not match this shape at all.
    let found = value
        .get("formatVersion")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| ProjectError::Invalid("missing or invalid formatVersion".into()))?;
    if found > FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found,
            supported: FORMAT_VERSION,
        });
    }

    let project: Project = serde_json::from_value(value)?;
    project.validate()?;
    for asset in &project.assets {
        by_name(&mut archive, &asset.file)?;
    }

    Ok((project, ProjectArchive { archive }))
}

/// Writes `project` and its assets to `path`. `sources` maps each asset id to its bytes.
///
/// The archive is written to a temporary file next to `path` and renamed over it, so a failed
/// save leaves any existing file untouched.
pub fn save(path: &Path, project: &Project, sources: &HashMap<String, AssetSource>) -> Result<()> {
    project.validate()?;

    let tmp = tmp_path(path);
    let written = write_archive(&tmp, project, sources).and_then(|()| Ok(fs::rename(&tmp, path)?));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn write_archive(
    path: &Path,
    project: &Project,
    sources: &HashMap<String, AssetSource>,
) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(path)?);

    zip.start_file(
        PROJECT_JSON,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    serde_json::to_writer_pretty(&mut zip, project)?;

    // Audio is large and usually compressed already, so it is stored as is.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut open_sources: HashMap<&Path, FileArchive> = HashMap::new();
    for asset in &project.assets {
        let source = sources
            .get(&asset.id)
            .ok_or_else(|| ProjectError::MissingAsset(asset.file.clone()))?;
        match source {
            AssetSource::Disk(file) => {
                zip.start_file(&asset.file, stored)?;
                io::copy(&mut BufReader::new(File::open(file)?), &mut zip)?;
            }
            AssetSource::Archive(archive_path) => {
                let archive = match open_sources.entry(archive_path.as_path()) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(open_archive(archive_path)?),
                };
                zip.raw_copy_file(by_name(archive, &asset.file)?)?;
            }
        }
    }

    // Close the source handles before the caller renames over a possible source archive.
    drop(open_sources);
    zip.finish()?;
    Ok(())
}

fn open_archive(path: &Path) -> Result<FileArchive> {
    Ok(ZipArchive::new(BufReader::new(File::open(path)?))?)
}

fn by_name<'a>(
    archive: &'a mut FileArchive,
    file: &str,
) -> Result<zip::read::ZipFile<'a, BufReader<File>>> {
    archive.by_name(file).map_err(|err| match err {
        ZipError::FileNotFound => ProjectError::MissingAsset(file.to_owned()),
        other => other.into(),
    })
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::TempDir;

    use super::*;
    use crate::project::model::tests::sample_project;

    const AUDIO_BYTES: &[u8] = b"RIFF....fake wav payload";

    /// Writes the sample audio to disk and returns the sources `save` needs for the sample project.
    fn disk_sources(dir: &TempDir) -> HashMap<String, AssetSource> {
        let wav = dir.path().join("click.wav");
        fs::write(&wav, AUDIO_BYTES).unwrap();
        HashMap::from([("a1".to_owned(), AssetSource::Disk(wav))])
    }

    fn read_asset(archive: &mut ProjectArchive, file: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        archive
            .asset_reader(file)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
    }

    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn save_then_open_should_round_trip_project_and_assets() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let project = sample_project();

        save(&path, &project, &disk_sources(&dir)).unwrap();
        let (opened, mut archive) = open(&path).unwrap();

        assert_eq!(opened, project);
        assert_eq!(read_asset(&mut archive, "audio/click.wav"), AUDIO_BYTES);
    }

    #[test]
    fn saved_archive_should_contain_project_json_and_the_audio_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(&path, &sample_project(), &disk_sources(&dir)).unwrap();

        let archive = open_archive(&path).unwrap();
        let mut names: Vec<_> = archive
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();

        assert_eq!(names, ["audio/click.wav", "project.json"]);
    }

    #[test]
    fn save_should_keep_assets_when_resaving_in_place_from_the_opened_archive() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(&path, &sample_project(), &disk_sources(&dir)).unwrap();

        let (mut project, archive) = open(&path).unwrap();
        project.name = "Renamed".into();
        let sources = HashMap::from([("a1".to_owned(), AssetSource::Archive(path.clone()))]);
        save(&path, &project, &sources).unwrap();
        drop(archive);

        let (reopened, mut archive) = open(&path).unwrap();
        assert_eq!(reopened.name, "Renamed");
        assert_eq!(read_asset(&mut archive, "audio/click.wav"), AUDIO_BYTES);
    }

    #[test]
    fn save_should_leave_the_existing_file_untouched_when_an_asset_source_is_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(&path, &sample_project(), &disk_sources(&dir)).unwrap();
        let before = fs::read(&path).unwrap();

        let sources = HashMap::from([(
            "a1".to_owned(),
            AssetSource::Disk(dir.path().join("does-not-exist.wav")),
        )]);
        let result = save(&path, &sample_project(), &sources);

        assert!(matches!(result, Err(ProjectError::Io(_))));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!tmp_path(&path).exists());
    }

    #[test]
    fn save_should_fail_without_a_source_for_every_asset() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");

        let result = save(&path, &sample_project(), &HashMap::new());

        assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
        assert!(!path.exists());
    }

    #[test]
    fn open_should_reject_an_archive_without_project_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        zip_with(&path, &[("audio/click.wav", AUDIO_BYTES)]);

        assert!(matches!(open(&path), Err(ProjectError::MissingProjectJson)));
    }

    #[test]
    fn open_should_reject_a_newer_format_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let newer = format!(
            r#"{{ "formatVersion": {}, "somethingNew": true }}"#,
            FORMAT_VERSION + 1
        );
        zip_with(&path, &[(PROJECT_JSON, newer.as_bytes())]);

        assert!(matches!(
            open(&path),
            Err(ProjectError::UnsupportedVersion { found, supported })
                if found == FORMAT_VERSION + 1 && supported == FORMAT_VERSION
        ));
    }

    #[test]
    fn open_should_reject_an_archive_missing_a_declared_asset() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let json = serde_json::to_vec(&sample_project()).unwrap();
        zip_with(&path, &[(PROJECT_JSON, &json)]);

        assert!(matches!(
            open(&path),
            Err(ProjectError::MissingAsset(file)) if file == "audio/click.wav"
        ));
    }

    #[test]
    fn open_should_reject_entries_that_escape_the_archive_root() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let json = serde_json::to_vec(&sample_project()).unwrap();
        zip_with(
            &path,
            &[
                (PROJECT_JSON, &json),
                ("audio/click.wav", AUDIO_BYTES),
                ("../evil.txt", b"x"),
            ],
        );

        assert!(matches!(
            open(&path),
            Err(ProjectError::InvalidAssetPath(name)) if name == "../evil.txt"
        ));
    }

    #[test]
    fn asset_reader_should_refuse_paths_outside_the_audio_folder() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(&path, &sample_project(), &disk_sources(&dir)).unwrap();
        let (_, mut archive) = open(&path).unwrap();

        assert!(matches!(
            archive.asset_reader("project.json"),
            Err(ProjectError::InvalidAssetPath(_))
        ));
    }
}
