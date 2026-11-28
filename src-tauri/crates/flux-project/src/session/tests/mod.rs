mod assets;
mod imports;
mod lifecycle;
mod timelines;
mod vj;

use std::io::Read;
use std::path::Path;

use tempfile::TempDir;

use super::*;
use crate::archive::{self};
use crate::model::{AudioClip, Track, TrackKind};
use crate::test_util::write_file;
use flux_core::FrameRate;

pub(super) const AUDIO_BYTES: &[u8] = b"RIFF....fake wav payload";

/// A session with one imported audio file used by a clip in a new project.
pub(super) fn imported_project(dir: &TempDir) -> (Session, Project) {
    let wav = write_file(dir.path(), "click.wav", AUDIO_BYTES);

    let mut session = Session::default();
    let mut project = session.new_project("Show", FrameRate::Fps25);
    let asset = session.import_embedded(&wav).unwrap();
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

pub(super) fn read_asset(path: &Path, file: &str) -> Vec<u8> {
    let (_, mut archive) = archive::open(path).unwrap();
    let mut bytes = Vec::new();
    archive
        .asset_reader(file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
