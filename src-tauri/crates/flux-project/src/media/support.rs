//! Test helpers shared by the tests of the media modules.

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;
use crate::test_util::write_file;
use crate::{FrameRate, Session};

/// A saved show in `dir/show/show.flx` with a linked song and a linked video in `dir/media`.
pub fn linked_show(dir: &TempDir) -> (Session, Project, PathBuf) {
    let song = write_file(&dir.path().join("media"), "song.wav", b"song");
    let clip = write_file(&dir.path().join("media"), "Clip.mp4", b"clip");
    let mut session = Session::default();
    let mut project = session.new_project("Show", FrameRate::Fps25);
    project.assets.push(session.import_audio(&song).unwrap());
    session.update(project.clone());
    let state = session.import_video(&clip, 0).unwrap();
    let flx = dir.path().join("show").join("show.flx");
    fs::create_dir_all(flx.parent().unwrap()).unwrap();
    let saved = session.save(&state.project, &flx).unwrap();
    (session, saved, flx)
}

pub fn entries_of(flx: &Path) -> Vec<String> {
    let zip = zip::ZipArchive::new(fs::File::open(flx).unwrap()).unwrap();
    zip.file_names().flatten().map(|n| n.into_owned()).collect()
}
