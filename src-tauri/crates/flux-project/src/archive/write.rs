//! Writing a project file.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::{self, File};
use std::io::{self, BufReader, Write};
use std::path::Path;

use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;
use crate::error::{ProjectError, Result};
use crate::model::Project;
use crate::preview::{StoredPreview, preview_path};

/// Writes `project` and its assets to `path`. `sources` maps each asset id to its bytes.
///
/// `previews` are the waveforms generated in this session; an asset without one keeps the preview
/// of the archive it is copied from, if any.
///
/// A linked asset writes no media, only its preview: the fresh one, else the one in the archive at
/// `preview_from` (the file the project was opened or saved as).
///
/// The archive is written to a temporary file next to `path` and renamed over it, so a failed
/// save leaves any existing file untouched.
pub fn save(
    path: &Path,
    project: &Project,
    sources: &HashMap<String, AssetSource>,
    previews: &HashMap<String, StoredPreview>,
) -> Result<()> {
    save_from(path, project, sources, previews, None)
}

/// [`save`] for a project with linked assets, whose previews come from `preview_from`.
pub fn save_from(
    path: &Path,
    project: &Project,
    sources: &HashMap<String, AssetSource>,
    previews: &HashMap<String, StoredPreview>,
    preview_from: Option<&Path>,
) -> Result<()> {
    project.validate()?;

    let tmp = tmp_path(path);
    let written = write_archive(&tmp, project, sources, previews, preview_from)
        .and_then(|()| Ok(fs::rename(&tmp, path)?));
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
    preview_from: Option<&Path>,
) -> Result<()> {
    let mut zip = ZipWriter::new(File::create(path)?);

    zip.start_file(
        PROJECT_JSON,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )?;
    if project.mode == crate::ProjectMode::Vj {
        serde_json::to_writer_pretty(&mut zip, &crate::vj_file::VjFile::of(project))?;
    } else {
        serde_json::to_writer_pretty(&mut zip, project)?;
    }

    // Audio is large and usually compressed already, so it is stored as is; previews are mostly noise.
    let stored = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .large_file(true);
    let mut open_sources: HashMap<&Path, FileArchive> = HashMap::new();
    for asset in project.assets.iter().filter(|a| a.link.is_some()) {
        let preview_file = preview_path(&asset.file);
        if let Some(fresh) = previews.get(&asset.id) {
            zip.start_file(&preview_file, stored)?;
            zip.write_all(&fresh.encode())?;
        } else if let Some(from) = preview_from {
            // The old file may be gone or damaged: a preview is only a cache.
            if let Ok(mut old) = open_archive(from)
                && let Ok(entry) = old.by_name(&preview_file)
            {
                zip.raw_copy_file(entry)?;
            }
        }
    }
    for asset in project.assets.iter().filter(|a| a.link.is_none()) {
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

    for video in project.videos.iter().filter(|v| v.link.is_none()) {
        let source = sources
            .get(&video.id)
            .ok_or_else(|| ProjectError::MissingAsset(video.file.clone()))?;
        match source {
            AssetSource::Disk(file) => {
                zip.start_file(&video.file, stored)?;
                io::copy(&mut BufReader::new(File::open(file)?), &mut zip)?;
            }
            AssetSource::Archive(archive_path) => {
                let archive = match open_sources.entry(archive_path.as_path()) {
                    Entry::Occupied(entry) => entry.into_mut(),
                    Entry::Vacant(entry) => entry.insert(open_archive(archive_path)?),
                };
                zip.raw_copy_file(by_name(archive, &video.file)?)?;
            }
        }
    }

    // Close the source handles before the caller renames over a possible source archive.
    drop(open_sources);
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {

    use std::collections::HashMap;
    use tempfile::TempDir;

    use super::*;
    use crate::archive::support::*;
    use crate::archive::*;
    use crate::model::FORMAT_VERSION;
    use crate::model::tests::{sample_project, video_project};
    use flux_core::FrameRate;

    #[test]
    fn save_then_open_should_round_trip_project_and_assets() {
        let dir = TempDir::new().unwrap();
        let project = sample_project();

        for name in ["show.flx", "show.fxtc"] {
            let path = dir.path().join(name);
            save(&path, &project, &disk_sources(&dir), &HashMap::new()).unwrap();
            let (opened, mut archive) = open(&path).unwrap();

            assert_eq!(opened, project);
            assert_eq!(read_asset(&mut archive, "audio/click.wav"), AUDIO_BYTES);
        }
    }

    #[test]
    fn save_then_open_should_bundle_the_video_and_keep_it_when_resaving() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.flx");
        let project = video_project();
        save(&path, &project, &video_sources(&dir), &HashMap::new()).unwrap();

        let (opened, mut archive) = open(&path).unwrap();
        assert_eq!(opened, project);
        assert_eq!(opened.format_version, FORMAT_VERSION);
        let mut bytes = Vec::new();
        archive
            .video_reader("video/intro.mp4")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, VIDEO_BYTES);
        drop(archive);

        let from_archive = HashMap::from([
            ("a1".to_owned(), AssetSource::Archive(path.clone())),
            ("v1".to_owned(), AssetSource::Archive(path.clone())),
        ]);
        let copy = dir.path().join("copy.flx");
        save(&copy, &project, &from_archive, &HashMap::new()).unwrap();
        assert_eq!(
            entry_names(&copy),
            ["audio/click.wav", "project.json", "video/intro.mp4"]
        );
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
    fn a_vj_show_should_be_saved_in_its_own_shape_and_open_again() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vj.flx");
        let mut project = Project::new("Gira", FrameRate::Fps25);
        project.mode = crate::ProjectMode::Vj;
        let mut n = 0;
        project.vj = crate::vj::Vj::starting(|| {
            n += 1;
            format!("g{n}")
        });
        project.timelines[0].tempo.bpm = 124.0;
        save(&path, &project, &HashMap::new(), &HashMap::new()).unwrap();

        let mut archive = open_archive(&path).unwrap();
        let json: serde_json::Value =
            serde_json::from_reader(archive.by_name(PROJECT_JSON).unwrap()).unwrap();
        assert!(json.get("timelines").is_none() && json.get("assets").is_none());
        assert_eq!(json["tempo"]["bpm"], 124.0);
        assert_eq!(json["layers"].as_array().unwrap().len(), 3);

        let (opened, _) = open(&path).unwrap();
        assert_eq!(opened.mode, crate::ProjectMode::Vj);
        assert_eq!(opened.vj, project.vj);
        assert_eq!(opened.timelines[0].tempo.bpm, 124.0);
    }

    #[test]
    fn read_meta_should_tell_a_vj_show_and_count_its_grid() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vj.flx");
        let mut project = Project::new("Gira", FrameRate::Fps25);
        project.mode = crate::ProjectMode::Vj;
        let mut n = 0;
        project.vj = crate::vj::Vj::starting(|| {
            n += 1;
            format!("g{n}")
        });
        save(&path, &project, &HashMap::new(), &HashMap::new()).unwrap();

        let meta = read_meta(&path).unwrap();

        assert_eq!(meta.mode, crate::ProjectMode::Vj);
        assert_eq!(
            (meta.layer_count, meta.deck_count, meta.clip_count),
            (crate::vj::DEFAULT_LAYERS, 1, 0)
        );
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
