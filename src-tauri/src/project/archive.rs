use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::error::{ProjectError, Result};
use super::meta;
use super::migrate;
use super::model::{AudioAsset, FORMAT_VERSION, FrameRate, Project, check_asset_path};
use super::preview::{AudioStamp, Preview, StoredPreview, preview_path};

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

    let mut value: serde_json::Value = match archive.by_name(PROJECT_JSON) {
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

    migrate::upgrade(&mut value, &file_date(path));
    let project: Project = serde_json::from_value(value)?;
    project.validate()?;
    for asset in &project.assets {
        by_name(&mut archive, &asset.file)?;
    }

    Ok((project, ProjectArchive { archive }))
}

/// When the file was last written, for a project that does not say when it was made.
fn file_date(path: &Path) -> String {
    let modified = fs::metadata(path)
        .and_then(|info| info.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    meta::format_time(modified)
}

/// What a list of shows displays, read from `project.json` alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub name: String,
    pub frame_rate: FrameRate,
    pub track_count: usize,
}

/// Reads the summary of a `.fxtc` without validating the project or looking at its audio.
///
/// Unknown fields are ignored, so a file from a newer version still lists as long as these three
/// fields keep their shape.
pub fn read_meta(path: &Path) -> Result<ProjectMeta> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        name: String,
        timecode: RawTimecode,
        tracks: Vec<serde::de::IgnoredAny>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RawTimecode {
        frame_rate: FrameRate,
    }

    let mut archive = open_archive(path)?;
    let raw: Raw = match archive.by_name(PROJECT_JSON) {
        Ok(entry) => serde_json::from_reader(entry)?,
        Err(ZipError::FileNotFound) => return Err(ProjectError::MissingProjectJson),
        Err(err) => return Err(err.into()),
    };
    Ok(ProjectMeta {
        name: raw.name,
        frame_rate: raw.timecode.frame_rate,
        track_count: raw.tracks.len(),
    })
}

/// The preview stored for `asset`, if there is one made from the audio the archive holds now.
///
/// A preview is only a cache, so one that is missing, unreadable or made from other audio is `None`.
pub fn read_preview(path: &Path, asset: &AudioAsset) -> Result<Option<Preview>> {
    let mut archive = open_archive(path)?;
    let audio = by_name(&mut archive, &asset.file)?;
    let stamp = AudioStamp {
        size: audio.size(),
        crc32: audio.crc32(),
    };
    drop(audio);

    let mut bytes = Vec::new();
    match archive.by_name(&preview_path(&asset.file)) {
        Ok(mut entry) => {
            if entry.read_to_end(&mut bytes).is_err() {
                return Ok(None);
            }
        }
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    Ok(StoredPreview::decode(&bytes)
        .filter(|stored| stored.stamp == stamp)
        .map(|stored| stored.preview))
}

/// Writes `project` and its assets to `path`. `sources` maps each asset id to its bytes.
///
/// `previews` are the waveforms generated in this session; an asset without one keeps the preview
/// of the archive it is copied from, if any.
///
/// The archive is written to a temporary file next to `path` and renamed over it, so a failed
/// save leaves any existing file untouched.
pub fn save(
    path: &Path,
    project: &Project,
    sources: &HashMap<String, AssetSource>,
    previews: &HashMap<String, StoredPreview>,
) -> Result<()> {
    project.validate()?;

    let tmp = tmp_path(path);
    let written =
        write_archive(&tmp, project, sources, previews).and_then(|()| Ok(fs::rename(&tmp, path)?));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn write_archive(
    path: &Path,
    project: &Project,
    sources: &HashMap<String, AssetSource>,
    previews: &HashMap<String, StoredPreview>,
) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(path)?);

    zip.start_file(
        PROJECT_JSON,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    serde_json::to_writer_pretty(&mut zip, project)?;

    // Audio is large and usually compressed already, so it is stored as is; previews are mostly noise.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut open_sources: HashMap<&Path, FileArchive> = HashMap::new();
    for asset in &project.assets {
        let source = sources
            .get(&asset.id)
            .ok_or_else(|| ProjectError::MissingAsset(asset.file.clone()))?;
        let fresh = previews.get(&asset.id);
        let preview_file = preview_path(&asset.file);
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
                if fresh.is_none() {
                    match archive.by_name(&preview_file) {
                        Ok(entry) => zip.raw_copy_file(entry)?,
                        Err(ZipError::FileNotFound) => {}
                        Err(err) => return Err(err.into()),
                    }
                }
            }
        }
        if let Some(fresh) = fresh {
            zip.start_file(&preview_file, stored)?;
            zip.write_all(&fresh.encode())?;
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
    use crate::project::preview::tests::stored;

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

        save(&path, &project, &disk_sources(&dir), &HashMap::new()).unwrap();
        let (opened, mut archive) = open(&path).unwrap();

        assert_eq!(opened, project);
        assert_eq!(read_asset(&mut archive, "audio/click.wav"), AUDIO_BYTES);
    }

    #[test]
    fn saved_archive_should_contain_project_json_and_the_audio_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();

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
        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();

        let (mut project, archive) = open(&path).unwrap();
        project.name = "Renamed".into();
        let sources = HashMap::from([("a1".to_owned(), AssetSource::Archive(path.clone()))]);
        save(&path, &project, &sources, &HashMap::new()).unwrap();
        drop(archive);

        let (reopened, mut archive) = open(&path).unwrap();
        assert_eq!(reopened.name, "Renamed");
        assert_eq!(read_asset(&mut archive, "audio/click.wav"), AUDIO_BYTES);
    }

    #[test]
    fn save_should_leave_the_existing_file_untouched_when_an_asset_source_is_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();
        let before = fs::read(&path).unwrap();

        let sources = HashMap::from([(
            "a1".to_owned(),
            AssetSource::Disk(dir.path().join("does-not-exist.wav")),
        )]);
        let result = save(&path, &sample_project(), &sources, &HashMap::new());

        assert!(matches!(result, Err(ProjectError::Io(_))));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!tmp_path(&path).exists());
    }

    #[test]
    fn save_should_fail_without_a_source_for_every_asset() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");

        let result = save(&path, &sample_project(), &HashMap::new(), &HashMap::new());

        assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
        assert!(!path.exists());
    }

    #[test]
    fn read_meta_should_summarise_a_saved_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let project = sample_project();
        save(&path, &project, &disk_sources(&dir), &HashMap::new()).unwrap();

        let meta = read_meta(&path).unwrap();

        assert_eq!(meta.name, project.name);
        assert_eq!(meta.frame_rate, project.timecode.frame_rate);
        assert_eq!(meta.track_count, project.tracks.len());
    }

    #[test]
    fn read_meta_should_ignore_fields_it_does_not_know_and_a_newer_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let newer = format!(
            r#"{{ "formatVersion": {}, "name": "Later", "somethingNew": true,
                 "timecode": {{ "frameRate": "fps25", "start": 0, "dropped": 1 }},
                 "tracks": [{{ "future": 1 }}, {{ "future": 2 }}] }}"#,
            FORMAT_VERSION + 1
        );
        zip_with(&path, &[(PROJECT_JSON, newer.as_bytes())]);

        let meta = read_meta(&path).unwrap();

        assert_eq!(meta.name, "Later");
        assert_eq!(meta.frame_rate, FrameRate::Fps25);
        assert_eq!(meta.track_count, 2);
    }

    #[test]
    fn read_meta_should_reject_an_archive_without_project_json() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        zip_with(&path, &[("audio/click.wav", AUDIO_BYTES)]);

        assert!(matches!(
            read_meta(&path),
            Err(ProjectError::MissingProjectJson)
        ));
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
    fn open_should_migrate_a_version_1_file_with_event_tracks_to_notes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r##"{ "formatVersion": 1, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
            "assets": [], "output": { "audio": null, "ltc": null, "mtc": null },
            "tracks": [{ "id": "t", "name": "Cues", "muted": false, "kind": { "type": "events", "triggers": [
                { "id": "e1", "at": 10, "label": "Go", "action": { "type": "marker" } },
                { "id": "e2", "at": 20, "label": "PC", "action": { "type": "midi", "bytes": [192, 1] } }
            ] } }] }"##;
        zip_with(&path, &[(PROJECT_JSON, old.as_bytes())]);

        let (project, _) = open(&path).unwrap();

        assert_eq!(project.format_version, FORMAT_VERSION);
        let crate::project::TrackKind::Notes { notes } = &project.tracks[0].kind else {
            panic!("the events track becomes a notes track");
        };
        assert_eq!(notes.len(), 1);
        assert_eq!((notes[0].at, notes[0].text.as_str()), (10, "Go"));
    }

    #[test]
    fn open_should_read_a_version_2_file_with_notes_and_single_outputs() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("titanium.fxtc");
        let written = r##"{ "formatVersion": 2, "name": "Titanium",
            "timecode": { "frameRate": "fps30", "start": 0 }, "assets": [],
            "tracks": [{ "id": "n", "name": "Events", "muted": false, "kind": { "type": "notes", "notes": [
                { "id": "a", "at": 60, "text": "Start INTRO" },
                { "id": "b", "at": 2115, "length": 404, "text": "Build up" }
            ] } }],
            "output": { "audio": { "deviceName": "", "channels": [1, 2] }, "ltc": null,
                        "mtc": { "portName": "loopMIDI Port" } } }"##;
        zip_with(&path, &[(PROJECT_JSON, written.as_bytes())]);

        let (project, _) = open(&path).unwrap();

        assert_eq!(project.format_version, FORMAT_VERSION);
        assert!(project.output.ltc.is_empty());
        assert_eq!(project.output.mtc.len(), 1);
        assert!(!project.meta.is_stamped());
        assert!(project.meta.validate().is_ok());
    }

    #[test]
    fn open_should_date_a_file_without_meta_by_the_file_itself() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r##"{ "formatVersion": 1, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
            "assets": [], "tracks": [], "output": {} }"##;
        zip_with(&path, &[(PROJECT_JSON, old.as_bytes())]);

        let (project, _) = open(&path).unwrap();

        assert_eq!(project.meta.created_at, file_date(&path));
        assert_eq!(project.meta.modified_at, project.meta.created_at);
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
        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();
        let (_, mut archive) = open(&path).unwrap();

        assert!(matches!(
            archive.asset_reader("project.json"),
            Err(ProjectError::InvalidAssetPath(_))
        ));
    }

    fn sample_asset() -> AudioAsset {
        sample_project().assets.remove(0)
    }

    fn previews(audio: &[u8]) -> HashMap<String, StoredPreview> {
        HashMap::from([("a1".to_owned(), stored(audio))])
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let mut names: Vec<_> = open_archive(path)
            .unwrap()
            .file_names()
            .map(|name| name.unwrap().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn save_should_store_the_preview_next_to_the_audio_and_read_preview_should_return_it() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let fresh = previews(AUDIO_BYTES);

        save(&path, &sample_project(), &disk_sources(&dir), &fresh).unwrap();

        assert_eq!(
            entry_names(&path),
            ["audio/click.wav", "peaks/click.wav.bin", "project.json"]
        );
        assert_eq!(
            read_preview(&path, &sample_asset()).unwrap(),
            Some(fresh["a1"].preview.clone())
        );
        assert!(open(&path).is_ok());
    }

    #[test]
    fn read_preview_should_be_none_when_the_archive_has_no_preview() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(read_preview(&path, &sample_asset()).unwrap(), None);
    }

    #[test]
    fn read_preview_should_ignore_a_preview_made_from_other_audio() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let before_replacing_the_audio = previews(b"RIFF....the old payload");

        save(
            &path,
            &sample_project(),
            &disk_sources(&dir),
            &before_replacing_the_audio,
        )
        .unwrap();

        assert_eq!(read_preview(&path, &sample_asset()).unwrap(), None);
    }

    #[test]
    fn read_preview_should_ignore_an_entry_this_build_cannot_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let json = serde_json::to_vec(&sample_project()).unwrap();
        zip_with(
            &path,
            &[
                (PROJECT_JSON, &json),
                ("audio/click.wav", AUDIO_BYTES),
                ("peaks/click.wav.bin", b"garbage"),
            ],
        );

        assert_eq!(read_preview(&path, &sample_asset()).unwrap(), None);
        assert!(open(&path).is_ok());
    }

    #[test]
    fn save_should_copy_the_preview_from_the_source_archive_when_there_is_no_new_one() {
        let dir = TempDir::new().unwrap();
        let original = dir.path().join("show.fxtc");
        let fresh = previews(AUDIO_BYTES);
        save(&original, &sample_project(), &disk_sources(&dir), &fresh).unwrap();

        let copy = dir.path().join("copy.fxtc");
        let sources = HashMap::from([("a1".to_owned(), AssetSource::Archive(original))]);
        save(&copy, &sample_project(), &sources, &HashMap::new()).unwrap();

        assert_eq!(
            read_preview(&copy, &sample_asset()).unwrap(),
            Some(fresh["a1"].preview.clone())
        );
    }

    #[test]
    fn save_should_not_invent_a_preview_for_an_archive_that_has_none() {
        let dir = TempDir::new().unwrap();
        let original = dir.path().join("show.fxtc");
        save(
            &original,
            &sample_project(),
            &disk_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();

        let copy = dir.path().join("copy.fxtc");
        let sources = HashMap::from([("a1".to_owned(), AssetSource::Archive(original))]);
        save(&copy, &sample_project(), &sources, &HashMap::new()).unwrap();

        assert_eq!(entry_names(&copy), ["audio/click.wav", "project.json"]);
    }
}
