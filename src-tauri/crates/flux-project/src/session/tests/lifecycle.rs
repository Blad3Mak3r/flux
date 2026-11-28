use std::fs;
use std::sync::atomic::Ordering;

use tempfile::TempDir;

use super::super::*;
use super::imported_project;
use crate::archive::{self};
use flux_core::FrameRate;

#[test]
fn the_show_lock_should_refuse_edits_opening_and_closing_but_not_starting_the_output() {
    let mut session = Session::default();
    let project = session.new_project("Show", flux_core::FrameRate::Fps25);
    session.show_lock.store(true, Ordering::Release);

    assert!(session.ensure_editable().is_err());
    assert!(session.ensure_not_running().is_ok());
    assert!(session.add_timeline("Timeline").is_err());
    assert!(session.select_timeline(&project.timelines[0].id).is_err());
    assert!(session.open(std::path::Path::new("missing.flx")).is_err());
    let mut changed = project.clone();
    changed.name = "Other".into();
    assert!(session.sync(changed).is_err());
    assert_eq!(session.document().unwrap().project, project);

    session.show_lock.store(false, Ordering::Release);
    assert!(session.add_timeline("Timeline").is_ok());
}

/// Measures what one edit costs on a large show. Run in release:
/// `cargo test -p flux-project --release -- --ignored --nocapture large_project_edit_cost`
#[test]
#[ignore = "a measurement, not a check"]
fn large_project_edit_cost() {
    use serde_json::json;
    use std::time::Instant;
    // Items are counted per timeline; FLUX_SCALE=3 divides them to the totals of the whole project.
    let scale: i32 = std::env::var("FLUX_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let mut session = Session::default();
    let mut project = session.new_project("Large", flux_core::FrameRate::Fps25);
    project.assets.push(
        session
            .import_audio(std::path::Path::new("Cargo.toml"))
            .unwrap(),
    );
    let asset = project.assets[0].id.clone();
    let template = project.timelines[0].clone();
    project.timelines.clear();
    for n in 0..3 {
        let mut timeline = template.clone();
        timeline.id = format!("timeline-{n}");
        let mut tracks = Vec::new();
        for t in 0..30 {
            let id = format!("{n}-{t}");
            let kind = match t % 4 {
                0 => json!({"type": "audio", "clips": (0..25 / scale).map(|c| json!({
                    "id": format!("c{id}-{c}"), "asset": asset, "start": c * 900, "gainDb": 0.0,
                    "length": 800, "fadeIn": 10, "fadeOut": 10,
                    "envelope": [{"at": 0, "gainDb": 0.0}, {"at": 400, "gainDb": -6.0}]})).collect::<Vec<_>>()}),
                1 => json!({"type": "notes", "notes": (0..625 / scale).map(|c| json!({
                    "id": format!("n{id}-{c}"), "at": c * 100, "text": "A line the operator reads\nand a second line"})).collect::<Vec<_>>()}),
                2 => json!({"type": "midi", "events": (0..133 / scale).map(|c| json!({
                    "id": format!("m{id}-{c}"), "at": c * 50,
                    "message": {"type": "noteOn", "channel": 1, "note": 60, "velocity": 100}})).collect::<Vec<_>>()}),
                _ => json!({"type": "osc", "events": (0..133 / scale).map(|c| json!({
                    "id": format!("o{id}-{c}"), "at": c * 50, "address": "/cue/1/start",
                    "args": [{"type": "int", "value": 1}]})).collect::<Vec<_>>()}),
            };
            tracks.push(
                serde_json::from_value(
                    json!({"id": format!("t{id}"), "name": "Track", "muted": false, "kind": kind}),
                )
                .unwrap(),
            );
        }
        timeline.tracks = tracks;
        project.timelines.push(timeline);
    }
    let active = project.timelines[0].id.clone();
    session.update(project.clone());
    let json = serde_json::to_string(&project).unwrap();
    let rounds = 20u32;
    let time = |what: &str, mut run: Box<dyn FnMut()>| {
        let started = Instant::now();
        for _ in 0..rounds {
            run();
        }
        println!("{what}: {:?} per call", started.elapsed() / rounds);
    };
    println!("payload: {} bytes", json.len());
    time(
        "serialize",
        Box::new(|| drop(serde_json::to_string(&project).unwrap())),
    );
    time(
        "deserialize",
        Box::new(|| drop(serde_json::from_str::<crate::Project>(&json).unwrap())),
    );
    time(
        "change()",
        Box::new(|| {
            session
                .change(|project, current| {
                    *current = active.clone();
                    project.name = "Large".into();
                    Ok(())
                })
                .unwrap();
        }),
    );
}

#[test]
fn delayed_sync_should_not_resurrect_a_closed_or_replaced_document() {
    let mut session = Session::default();
    let old = session.new_project("Old", flux_core::FrameRate::Fps25);
    session.clear();
    assert!(session.sync(old.clone()).is_err());
    assert!(session.document().is_none());
    let current = session.new_project("Current", flux_core::FrameRate::Fps25);
    assert!(session.sync(old).is_err());
    assert_eq!(session.document().unwrap().project, current);
}

#[test]
fn imported_relative_sources_should_be_absolute_in_recovery() {
    let mut session = Session::default();
    let mut project = session.new_project("Show", flux_core::FrameRate::Fps25);
    project.assets.push(
        session
            .import_audio(std::path::Path::new("Cargo.toml"))
            .unwrap(),
    );
    session.update(project);
    let record = session.recovery().unwrap();
    record.validate().unwrap();
    assert!(record.sources.values().all(|source| match source {
        crate::AssetSource::Disk(path) | crate::AssetSource::Archive(path) => path.is_absolute(),
    }));
}

#[test]
fn autosave_options_should_validate_roundtrip_and_follow_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("show.fxtc");
    let mut session = Session::default();
    session.new_project("Show", flux_core::FrameRate::Fps25);
    for interval_seconds in [15, 30, 60] {
        let prefs = crate::Autosave {
            enabled: false,
            interval_seconds,
        };
        let state = session.set_autosave(prefs).unwrap();
        session.save(&state.project, &file).unwrap();
        session.new_project("Other", flux_core::FrameRate::Fps25);
        assert_eq!(
            session.document().unwrap().project.autosave,
            crate::Autosave::default()
        );
        assert_eq!(session.open(&file).unwrap().autosave, prefs);
    }
    let before = session.document();
    assert!(
        session
            .set_autosave(crate::Autosave {
                enabled: true,
                interval_seconds: 17
            })
            .is_err()
    );
    assert_eq!(session.document(), before);
}

#[test]
fn signal_edit_should_preserve_document_on_invalid_level() {
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);
    session.edit_signal(crate::SignalOp::AddLtc).unwrap();
    let before = session.document().unwrap();
    assert!(
        session
            .edit_signal(crate::SignalOp::SetLtcLevel {
                slot: 1,
                text: "2".into()
            })
            .is_err()
    );
    assert_eq!(session.document().unwrap(), before);
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
fn reopening_after_closing_a_new_project_should_load_the_original_archive() {
    let dir = TempDir::new().unwrap();
    let (mut session, project) = imported_project(&dir);
    let path = dir.path().join("Show.fxtc");
    let saved = session.save(&project, &path).unwrap();
    let archive = fs::read(&path).unwrap();

    session.new_project("Untitled", FrameRate::Fps25);
    session.clear();

    // A fresh editor only reads the requested file when there is no session to restore.
    assert!(session.document().is_none());
    assert_eq!(session.open(&path).unwrap(), saved);
    assert_eq!(fs::read(&path).unwrap(), archive);
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
    assert_eq!(saved.meta.saved_with, crate::meta::APP_VERSION);
    assert_ne!(saved.meta.modified_at, "2000-01-01T00:00:00Z");
    assert_eq!(archive::open(&path).unwrap().0, saved);
}

#[test]
fn open_should_leave_an_old_archive_unchanged_until_explicit_save() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("old.fxtc");
    let old = r##"{ "formatVersion": 2, "name": "Old", "timecode": { "frameRate": "fps25", "start": 0 },
        "assets": [], "tracks": [], "output": { "ltc": null, "mtc": { "portName": "loopMIDI Port" } } }"##;
    let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    zip.start_file("project.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, old.as_bytes()).unwrap();
    zip.finish().unwrap();

    let before = fs::read(&path).unwrap();
    let mut session = Session::default();
    let opened = session.open(&path).unwrap();
    assert!(!opened.meta.is_stamped());
    assert_eq!(fs::read(&path).unwrap(), before);
    session.save(&opened, &path).unwrap();
    let again = Session::default().open(&path).unwrap();
    assert!(again.meta.is_stamped());
    assert_eq!(again.meta.id, opened.meta.id);
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
