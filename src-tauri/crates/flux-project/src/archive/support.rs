//! Test helpers shared by the tests of the archive modules.

use std::collections::HashMap;
use std::path::Path;

use tempfile::TempDir;

use super::*;
use crate::model::AudioAsset;
use crate::model::tests::sample_project;
use crate::preview::StoredPreview;
use crate::preview::tests::stored;
use crate::test_util::write_file;
use std::fs::File;
use std::io::Write;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// Writes the sample audio to disk and returns the sources `save` needs for the sample project.
pub fn disk_sources(dir: &TempDir) -> HashMap<String, AssetSource> {
    let wav = write_file(dir.path(), "click.wav", AUDIO_BYTES);
    HashMap::from([("a1".to_owned(), AssetSource::Disk(wav))])
}

pub fn read_asset(archive: &mut ProjectArchive, file: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    archive
        .asset_reader(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}

pub fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

pub const AUDIO_BYTES: &[u8] = b"RIFF....fake wav payload";

pub const VIDEO_BYTES: &[u8] = b"....ftypisom fake mp4 payload";

pub fn video_sources(dir: &TempDir) -> HashMap<String, AssetSource> {
    let mp4 = write_file(dir.path(), "intro.mp4", VIDEO_BYTES);
    let mut sources = disk_sources(dir);
    sources.insert("v1".to_owned(), AssetSource::Disk(mp4));
    sources
}

pub fn sample_asset() -> AudioAsset {
    sample_project().assets.remove(0)
}

pub fn previews(audio: &[u8]) -> HashMap<String, StoredPreview> {
    HashMap::from([("a1".to_owned(), stored(audio))])
}

pub fn entry_names(path: &Path) -> Vec<String> {
    let mut names: Vec<_> = open_archive(path)
        .unwrap()
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    names
}
