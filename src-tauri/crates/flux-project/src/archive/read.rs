//! Reading a project file: opening it, its summary and its previews.

use std::fs::{self};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use zip::result::ZipError;

use super::*;
use crate::error::{ProjectError, Result};
use crate::meta;
use crate::migrate;
use crate::model::{AudioAsset, FORMAT_VERSION, Project};
use crate::preview::{AudioStamp, Preview, StoredPreview, preview_path};
use flux_core::FrameRate;

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

    let project: Project = if crate::vj_file::is_vj_file(&value) {
        serde_json::from_value::<crate::vj_file::VjFile>(value)?.into_project()
    } else {
        migrate::upgrade(&mut value, &file_date(path));
        serde_json::from_value(value)?
    };
    project.validate()?;
    // Linked media is not in the archive, and missing media never stops a project from opening.
    for file in project
        .assets
        .iter()
        .filter(|a| a.link.is_none())
        .map(|a| &a.file)
        .chain(
            project
                .videos
                .iter()
                .filter(|v| v.link.is_none())
                .map(|v| &v.file),
        )
    {
        by_name(&mut archive, file)?;
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
    /// A cue show with timelines or a VJ show with a grid.
    pub mode: crate::ProjectMode,
    /// For a VJ show, its grid: how many layers, decks and clips.
    pub layer_count: usize,
    pub deck_count: usize,
    pub clip_count: usize,
}

/// Reads the summary of a `.fxtc` without validating the project or looking at its audio. It shows
/// the frame rate of the first timeline and the tracks of all of them.
///
/// Unknown fields are ignored, so a file from a newer version still lists as long as these fields
/// keep their shape. Files from before version 4 have their one timeline at the top level.
pub fn read_meta(path: &Path) -> Result<ProjectMeta> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        name: String,
        #[serde(default)]
        timelines: Vec<RawTimeline>,
        timecode: Option<RawTimecode>,
        #[serde(default)]
        tracks: Vec<serde::de::IgnoredAny>,
        #[serde(default)]
        mode: crate::ProjectMode,
        #[serde(default)]
        vj: RawGrid,
        /// A VJ show in its own shape has its grid at the top level.
        #[serde(flatten)]
        grid: RawGrid,
    }
    #[derive(Deserialize, Default)]
    struct RawGrid {
        #[serde(default)]
        layers: Vec<RawLayer>,
        #[serde(default)]
        decks: Vec<serde::de::IgnoredAny>,
    }
    #[derive(Deserialize)]
    struct RawLayer {
        #[serde(default)]
        clips: Vec<serde::de::IgnoredAny>,
    }
    #[derive(Deserialize)]
    struct RawTimeline {
        timecode: RawTimecode,
        #[serde(default)]
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
    let (frame_rate, track_count) = match (raw.timelines.first(), raw.timecode) {
        (Some(first), _) => (
            first.timecode.frame_rate,
            raw.timelines
                .iter()
                .map(|timeline| timeline.tracks.len())
                .sum(),
        ),
        (None, Some(timecode)) => (timecode.frame_rate, raw.tracks.len()),
        (None, None) => return Err(ProjectError::Invalid("project.json has no timeline".into())),
    };
    let grid = if raw.vj.layers.is_empty() {
        &raw.grid
    } else {
        &raw.vj
    };
    Ok(ProjectMeta {
        name: raw.name,
        frame_rate,
        track_count,
        mode: raw.mode,
        layer_count: grid.layers.len(),
        deck_count: grid.decks.len(),
        clip_count: grid.layers.iter().map(|layer| layer.clips.len()).sum(),
    })
}

/// The preview stored for `asset`, if there is one made from the audio the archive holds now (or,
/// for linked audio that is not in the archive, from the audio with the `linked` stamp).
///
/// A preview is only a cache, so one that is missing, unreadable or made from other audio is `None`.
pub fn read_preview(path: &Path, asset: &AudioAsset) -> Result<Option<Preview>> {
    read_preview_of(path, asset, None)
}

/// [`read_preview`] with the stamp of the audio when it lives outside the archive.
pub fn read_preview_of(
    path: &Path,
    asset: &AudioAsset,
    linked: Option<AudioStamp>,
) -> Result<Option<Preview>> {
    let mut archive = open_archive(path)?;
    let stamp = match linked {
        Some(stamp) => stamp,
        None => {
            let audio = by_name(&mut archive, &asset.file)?;
            AudioStamp {
                size: audio.size(),
                crc32: audio.crc32(),
            }
        }
    };

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

#[cfg(test)]
mod tests {

    use std::collections::HashMap;
    use tempfile::TempDir;

    use super::*;
    use crate::archive::support::*;
    use crate::archive::*;
    use crate::model::tests::{sample_project, video_project};

    #[test]
    fn open_should_reject_an_archive_missing_a_declared_video() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.flx");
        let json = serde_json::to_vec(&video_project()).unwrap();
        zip_with(
            &path,
            &[("project.json", &json), ("audio/click.wav", AUDIO_BYTES)],
        );
        assert!(matches!(
            open(&path),
            Err(ProjectError::MissingAsset(file)) if file == "video/intro.mp4"
        ));
    }

    #[test]
    fn video_reader_should_refuse_paths_outside_the_video_folder() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.flx");
        save(
            &path,
            &video_project(),
            &video_sources(&dir),
            &HashMap::new(),
        )
        .unwrap();
        let (_, mut archive) = open(&path).unwrap();
        assert!(archive.video_reader("audio/click.wav").is_err());
    }

    #[test]
    fn read_meta_should_summarise_a_saved_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let project = sample_project();
        save(&path, &project, &disk_sources(&dir), &HashMap::new()).unwrap();

        let meta = read_meta(&path).unwrap();

        assert_eq!(meta.name, project.name);
        assert_eq!(meta.frame_rate, project.timelines[0].timecode.frame_rate);
        assert_eq!(meta.track_count, project.timelines[0].tracks.len());
    }

    #[test]
    fn read_meta_should_ignore_fields_it_does_not_know_and_a_newer_version() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("show.fxtc");
        let newer = format!(
            r#"{{ "formatVersion": {}, "name": "Later", "somethingNew": true,
                 "timelines": [
                     {{ "timecode": {{ "frameRate": "fps25", "start": 0, "dropped": 1 }},
                        "tracks": [{{ "future": 1 }}] }},
                     {{ "timecode": {{ "frameRate": "fps30", "start": 0 }},
                        "tracks": [{{ "future": 2 }}] }}
                 ] }}"#,
            FORMAT_VERSION + 1
        );
        zip_with(&path, &[(PROJECT_JSON, newer.as_bytes())]);

        let meta = read_meta(&path).unwrap();

        assert_eq!(meta.name, "Later");
        assert_eq!(meta.frame_rate, FrameRate::Fps25);
        assert_eq!(meta.track_count, 2);
    }

    #[test]
    fn read_meta_should_read_a_file_from_before_timelines() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("old.fxtc");
        let old = r#"{ "formatVersion": 3, "name": "Old",
            "timecode": { "frameRate": "fps30", "start": 0 }, "tracks": [{ "x": 1 }] }"#;
        zip_with(&path, &[(PROJECT_JSON, old.as_bytes())]);

        let meta = read_meta(&path).unwrap();

        assert_eq!((meta.frame_rate, meta.track_count), (FrameRate::Fps30, 1));
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
        let crate::TrackKind::Notes { notes } = &project.timelines[0].tracks[0].kind else {
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
}
