use std::fs;

use tempfile::TempDir;

use super::super::*;
use super::{AUDIO_BYTES, imported_project, read_asset};
use crate::error::ProjectError;
use crate::preview::{AudioStamp, Preview};
use flux_core::FrameRate;

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
fn save_should_copy_legacy_project_and_audio_to_flx_without_changing_the_original() {
    let dir = TempDir::new().unwrap();
    let (mut session, project) = imported_project(&dir);
    let original = dir.path().join("show.fxtc");
    session.save(&project, &original).unwrap();
    let original_bytes = fs::read(&original).unwrap();

    let mut reopened = Session::default();
    let opened = reopened.open(&original).unwrap();
    let copy = dir.path().join("copy.flx");
    let saved = reopened.save(&opened, &copy).unwrap();

    assert_eq!(read_asset(&copy, &opened.assets[0].file), AUDIO_BYTES);
    assert_eq!(Session::default().open(&copy).unwrap(), saved);
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
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
