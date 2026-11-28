use std::fs;
use std::io::Read;
use std::path::PathBuf;

use tempfile::TempDir;

use super::super::*;
use super::{AUDIO_BYTES, imported_project};
use crate::archive::{self, AssetSource};
use crate::dropped::{MediaKind, Staged};
use crate::error::ProjectError;
use crate::model::TrackKind;
use crate::test_util::write_file;
use flux_core::FrameRate;

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
    let wav = write_file(dir.path(), "click.wav", AUDIO_BYTES);
    let mut session = Session::default();

    let first = session.import_audio(&wav).unwrap();
    let second = session.import_audio(&wav).unwrap();

    assert_ne!(first.id, second.id);
    assert_ne!(first.file, second.file);
}

#[test]
fn import_video_should_add_a_track_with_a_clip_and_save_the_file() {
    let dir = TempDir::new().unwrap();
    let mp4 = write_file(dir.path(), "intro.mp4", b"not decoded here");
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);

    let state = session.import_video(&mp4, 50).unwrap();

    let asset = &state.project.videos[0];
    assert_eq!(asset.name, "intro");
    assert!(asset.file.starts_with("video/") && asset.file.ends_with("_intro.mp4"));
    let track = state.project.timelines[0].tracks.last().unwrap();
    let TrackKind::Video { clips } = &track.kind else {
        panic!("not a video track: {track:?}");
    };
    assert_eq!(
        (clips[0].asset.as_str(), clips[0].start),
        (asset.id.as_str(), 50)
    );

    let path = dir.path().join("show.flx");
    session.save(&state.project, &path).unwrap();
    let (reopened, mut archive) = archive::open(&path).unwrap();
    // The video stays where it is: the archive holds only the link.
    assert!(archive.video_reader(&asset.file).is_err());
    let link = reopened.videos[0].link.as_ref().unwrap();
    assert_eq!(link.path, mp4.to_string_lossy());
    assert_eq!(link.relative.as_deref(), Some("intro.mp4"));
}

#[test]
fn replace_video_should_move_every_clip_to_the_new_file_and_undo_should_bring_it_back() {
    let dir = TempDir::new().unwrap();
    let mp4 = write_file(dir.path(), "intro.mp4", b"h264");
    let mov = write_file(dir.path(), "converted.mov", b"hap");
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);
    let before = session.import_video(&mp4, 10).unwrap();
    let old = before.project.videos[0].clone();

    let after = session.replace_video(&old.id, &mov, false).unwrap();

    let new = &after.project.videos[0];
    assert_eq!(after.project.videos.len(), 1);
    assert_ne!(new.id, old.id);
    assert_eq!(new.name, "intro");
    assert!(
        new.file.starts_with("video/") && new.file.ends_with("_intro.mov"),
        "{}",
        new.file
    );
    let TrackKind::Video { clips } = &after.project.timelines[0].tracks.last().unwrap().kind else {
        panic!("not a video track");
    };
    assert_eq!(
        (clips[0].asset.as_str(), clips[0].start),
        (new.id.as_str(), 10)
    );
    let cache = dir.path().join("cache");
    assert_eq!(session.video_path(new, &cache).unwrap(), mov);

    // Undo is the old project sent back: its file is still known.
    session.update(before.project.clone());
    assert_eq!(session.video_path(&old, &cache).unwrap(), mp4);
    let path = dir.path().join("show.flx");
    session.save(&after.project, &path).unwrap();
    let (_, mut archive) = archive::open(&path).unwrap();
    let mut bytes = Vec::new();
    archive
        .video_reader(&new.file)
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"hap");
    assert!(archive.video_reader(&old.file).is_err());
    // Undo past the save: the original is still read from where it was imported.
    session.update(before.project);
    assert_eq!(session.video_path(&old, &cache).unwrap(), mp4);
}

#[test]
fn drop_on_grid_should_fill_the_columns_from_the_target_and_leave_out_the_overflow() {
    let dir = TempDir::new().unwrap();
    let files: Vec<PathBuf> = (0..3)
        .map(|n| write_file(dir.path(), &format!("{n}.mp4"), b"v"))
        .collect();
    let mut session = Session::default();
    let grid = session
        .new_project_of("Show", FrameRate::Fps25, crate::ProjectMode::Vj)
        .vj;
    let columns = &grid.decks[0].columns;
    let last = columns.len() - 2;

    let (state, placed) = session
        .drop_on_grid(&files, &grid.layers[0].id, &columns[last].id)
        .unwrap();

    let cells: Vec<_> = state.project.vj.layers[0]
        .clips
        .iter()
        .map(|clip| clip.column.clone())
        .collect();
    assert_eq!(
        (placed, cells),
        (
            2,
            vec![
                Some(columns[last].id.clone()),
                Some(columns[last + 1].id.clone())
            ]
        )
    );
}

#[test]
fn drop_on_timeline_should_add_the_staged_assets_and_clips() {
    let dir = TempDir::new().unwrap();
    let wav = write_file(dir.path(), "kick.wav", b"riff");
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);

    let staged = session.stage_drop(&wav, MediaKind::Audio).unwrap();
    let state = session
        .drop_on_timeline(
            vec![Staged {
                media: staged,
                frames: 25,
            }],
            None,
            50,
        )
        .unwrap();

    let timeline = &state.project.timelines[0];
    let TrackKind::Audio { clips } = &timeline.tracks[0].kind else {
        panic!("audio track");
    };
    assert_eq!((state.project.assets.len(), clips[0].start), (1, 50));
    assert!(state.project.assets[0].link.is_some());
    assert!(session.sources.is_empty());
}

#[test]
fn import_vj_video_should_fill_the_cell_save_the_file_and_follow_a_replace() {
    let dir = TempDir::new().unwrap();
    let mp4 = write_file(dir.path(), "loop.mp4", b"h264");
    let mov = write_file(dir.path(), "loop-hap.mov", b"hap");
    let mut session = Session::default();
    let grid = session
        .new_project_of("Show", FrameRate::Fps25, crate::ProjectMode::Vj)
        .vj;
    let (layer, column) = (&grid.layers[1].id, &grid.decks[0].columns[2].id);

    let state = session.import_vj_video(&mp4, layer, Some(column)).unwrap();

    let asset = state.project.videos[0].clone();
    let clip = &state.project.vj.layers[1].clips[0];
    assert_eq!(
        (&clip.asset, clip.column.as_ref()),
        (&asset.id, Some(column))
    );
    let path = dir.path().join("show.flx");
    session.save(&state.project, &path).unwrap();
    let (saved, _) = archive::open(&path).unwrap();
    assert_eq!(saved.videos[0].id, asset.id);
    assert_eq!(
        saved.videos[0].link.as_ref().unwrap().path,
        asset.link.as_ref().unwrap().path
    );

    let after = session.replace_video(&asset.id, &mov, false).unwrap();
    assert_eq!(
        after.project.vj.layers[1].clips[0].asset,
        after.project.videos[0].id
    );
}

#[test]
fn replace_video_should_refuse_an_unknown_video() {
    let dir = TempDir::new().unwrap();
    let mov = write_file(dir.path(), "converted.mov", b"hap");
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);

    let result = session.replace_video("nope", &mov, false);

    assert!(matches!(result, Err(ProjectError::MissingAsset(_))));
}

#[test]
fn video_path_should_copy_a_saved_video_out_of_the_archive_once() {
    let dir = TempDir::new().unwrap();
    let mp4 = write_file(dir.path(), "intro.mp4", b"frames");
    let mut session = Session::default();
    session.new_project("Show", FrameRate::Fps25);
    let mut state = session.import_video(&mp4, 0).unwrap();
    // A legacy project holds its video inside the archive.
    state.project.videos[0].link = None;
    let asset = state.project.videos[0].clone();
    session
        .sources
        .insert(asset.id.clone(), AssetSource::Disk(mp4.clone()));
    let cache = dir.path().join("cache");
    assert_eq!(session.video_path(&asset, &cache).unwrap(), mp4);

    let path = dir.path().join("show.flx");
    session.save(&state.project, &path).unwrap();
    let mut reopened = Session::default();
    reopened.open(&path).unwrap();
    let copied = reopened.video_path(&asset, &cache).unwrap();

    assert!(copied.starts_with(&cache));
    assert_eq!(fs::read(&copied).unwrap(), b"frames");
    assert_eq!(reopened.video_path(&asset, &cache).unwrap(), copied);
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
fn save_should_drop_a_video_no_clip_shows() {
    let dir = TempDir::new().unwrap();
    let (mut session, mut project) = imported_project(&dir);
    project.videos.push(crate::VideoAsset {
        id: "v1".into(),
        name: "Intro".into(),
        file: "video/intro.mp4".into(),
        link: None,
    });

    let saved = session
        .save(&project, &dir.path().join("show.flx"))
        .unwrap();

    assert!(saved.videos.is_empty());
}
