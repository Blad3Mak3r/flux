//! Decodes the fixtures: 50 frames of 64x36, each a flat grey a little lighter than the one before,
//! encoded losslessly with a keyframe every 12 frames. Software decoding only, no GPU or display.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use flux_video::{Rate, VideoError, VideoFile, VideoPlayer};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The red value of the frame's first pixel; the fixtures' frames differ in it.
fn level(file: &mut VideoFile, index: u64) -> u8 {
    let frame = file.frame(index).unwrap();
    assert_eq!(frame.index, index);
    assert_eq!(frame.rgba.len(), 64 * 36 * 4);
    frame.rgba[0]
}

fn levels_in_order(name: &str) -> Vec<u8> {
    let mut file = VideoFile::open(&fixture(name)).unwrap();
    (0..50).map(|i| level(&mut file, i)).collect()
}

#[test]
fn open_should_read_the_size_rate_and_frame_count() {
    for (name, rate) in [
        ("gray25.mp4", Rate::new(25, 1)),
        ("gray2997.mp4", Rate::new(30_000, 1_001)),
    ] {
        let file = VideoFile::open(&fixture(name)).unwrap();
        let info = file.info();
        assert_eq!((info.width, info.height), (64, 36), "{name}");
        assert_eq!(info.rate, rate, "{name}");
        assert_eq!(info.frames, 50, "{name}");
        assert_eq!(info.codec, "h264", "{name}");
        assert!(!info.has_audio, "{name}");
    }
}

#[test]
fn every_frame_should_be_a_different_picture() {
    for name in ["gray25.mp4", "gray2997.mp4"] {
        let levels = levels_in_order(name);
        assert!(levels.windows(2).all(|w| w[0] < w[1]), "{name}: {levels:?}");
    }
}

#[test]
fn a_seek_in_any_order_should_give_the_same_frame_as_playing_through() {
    for name in ["gray25.mp4", "gray2997.mp4"] {
        let played = levels_in_order(name);
        let mut file = VideoFile::open(&fixture(name)).unwrap();
        // Back and forth, across keyframes (every 12), onto them and just before them.
        for index in [37, 3, 49, 0, 24, 23, 12, 11, 36, 48, 1, 25, 25, 26] {
            assert_eq!(
                level(&mut file, index),
                played[index as usize],
                "{name} frame {index}"
            );
        }
    }
}

#[test]
fn a_frame_past_the_end_should_be_an_error() {
    let mut file = VideoFile::open(&fixture("gray25.mp4")).unwrap();
    assert!(matches!(
        file.frame(50),
        Err(VideoError::PastEnd {
            index: 50,
            frames: 50
        })
    ));
    // The file still works after it.
    assert_eq!(file.frame(10).unwrap().index, 10);
}

#[test]
fn open_should_fail_for_a_file_that_is_not_a_video() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert!(VideoFile::open(&path).is_err());
}

#[test]
fn the_player_should_end_on_the_newest_request() {
    let played = levels_in_order("gray25.mp4");
    let player = VideoPlayer::open(fixture("gray25.mp4")).unwrap();
    for index in [5, 40, 12, 30] {
        player.request(index);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let frame = loop {
        if let Some(frame) = player.latest().filter(|f| f.index == 30) {
            break frame;
        }
        assert!(Instant::now() < deadline, "frame 30 never arrived");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(frame.rgba[0], played[30]);
}
