//! Decodes the fixtures: 50 frames of 64x36, each a flat grey a little lighter than the one before,
//! encoded losslessly with a keyframe every 12 frames, as H.264 in MP4 and VP9 in WebM. No display
//! is needed; a GPU decoder is used only where the computer has one. `still.png` and `still.jpg` are
//! one flat grey picture of the same size.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use flux_video::{Rate, VideoError, VideoFile, VideoPlayer};

const FIXTURES: [&str; 3] = ["gray25.mp4", "gray2997.mp4", "gray25.webm"];

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
    for (name, rate, codec) in [
        ("gray25.mp4", Rate::new(25, 1), "h264"),
        ("gray2997.mp4", Rate::new(30_000, 1_001), "h264"),
        ("gray25.webm", Rate::new(25, 1), "vp9"),
    ] {
        let file = VideoFile::open(&fixture(name)).unwrap();
        let info = file.info();
        assert_eq!((info.width, info.height), (64, 36), "{name}");
        assert_eq!(info.rate, rate, "{name}");
        assert_eq!(info.frames, 50, "{name}");
        assert_eq!(info.codec, codec, "{name}");
        assert!(!info.has_audio, "{name}");
    }
}

#[test]
fn every_frame_should_be_a_different_picture() {
    for name in FIXTURES {
        let levels = levels_in_order(name);
        assert!(levels.windows(2).all(|w| w[0] < w[1]), "{name}: {levels:?}");
    }
}

#[test]
fn a_seek_in_any_order_should_give_the_same_frame_as_playing_through() {
    for name in FIXTURES {
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
fn an_accelerated_file_should_give_the_same_frames_wherever_it_decodes() {
    for name in FIXTURES {
        let played = levels_in_order(name);
        let mut file = VideoFile::open_accelerated(&fixture(name)).unwrap();
        for index in [0, 1, 2, 30, 13, 49, 12] {
            let level = level(&mut file, index);
            // A GPU converts its colours on its own way: a step or two apart at most.
            assert!(
                level.abs_diff(played[index as usize]) <= 2,
                "{name} frame {index} on {:?}: {level} vs {}",
                file.decoding(),
                played[index as usize]
            );
        }
    }
}

#[test]
fn a_webm_with_an_alpha_track_should_keep_its_transparency() {
    // 10 frames of 64x36: the left half opaque red, the right half transparent (VP9, lossless).
    let mut file = VideoFile::open_accelerated(&fixture("alpha25.webm")).unwrap();
    let info = file.info().clone();
    assert!(info.alpha);
    assert_eq!((info.codec.as_str(), info.frames), ("vp9", 10));
    let frame = file.frame(4).unwrap();
    assert!(frame.alpha);
    let pixel = |x: usize| &frame.rgba[x * 4..x * 4 + 4];
    assert!(pixel(5)[0] > 250 && pixel(5)[3] == 255, "{:?}", pixel(5));
    assert_eq!(pixel(50)[3], 0);
    // Without an alpha track nothing changes.
    let opaque = VideoFile::open(&fixture("gray25.webm")).unwrap();
    assert!(!opaque.info().alpha);
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

#[test]
fn thumbnails_should_give_one_picture_a_second_side_by_side() {
    for name in FIXTURES {
        let thumbs = flux_video::thumbnails(&fixture(name)).unwrap();
        // 64x36 at the thumbnail height, 50 frames at about 25 or 30 per second.
        assert_eq!((thumbs.width, thumbs.height), (96, 54), "{name}");
        assert_eq!(thumbs.count, 2, "{name}");
        assert!((0.99..1.01).contains(&thumbs.seconds), "{name}");
        assert_eq!(&thumbs.jpeg[..2], &[0xFF, 0xD8], "{name}");
    }
}

#[test]
fn poster_should_give_only_the_first_frame() {
    for name in FIXTURES {
        let poster = flux_video::poster(&fixture(name)).unwrap();
        // 64x36 at the poster height.
        assert_eq!((poster.width, poster.height), (320, 180), "{name}");
        assert_eq!(poster.count, 1, "{name}");
        assert_eq!(&poster.jpeg[..2], &[0xFF, 0xD8], "{name}");
    }
}

#[test]
fn a_still_image_should_open_as_one_frame() {
    // A flat grey 64x36 picture, as PNG and as JPEG.
    for name in ["still.png", "still.jpg"] {
        let mut file = VideoFile::open(&fixture(name)).unwrap();
        assert_eq!(file.info().frames, 1, "{name}");
        let grey = level(&mut file, 0);
        assert!(grey.abs_diff(0x80) <= 2, "{name}: {grey}");
        assert!(
            matches!(file.frame(1), Err(VideoError::PastEnd { .. })),
            "{name}"
        );
        let poster = flux_video::poster(&fixture(name)).unwrap();
        assert_eq!(poster.count, 1, "{name}");
    }
}
