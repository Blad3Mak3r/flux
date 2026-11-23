//! Converts the fixtures to HAP and reads the copies back as Flux reads any video. Skipped where
//! FFmpeg was built without HAP.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use flux_video::{VideoError, VideoFile, convert_to_hap, hap_available};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn convert(name: &str) -> (tempfile::TempDir, PathBuf, Vec<(u64, u64)>) {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("copy.mov");
    let mut seen = Vec::new();
    convert_to_hap(
        &fixture(name),
        &dst,
        &mut |done, total| seen.push((done, total)),
        &AtomicBool::new(false),
    )
    .unwrap();
    (dir, dst, seen)
}

#[test]
fn a_hap_copy_should_hold_the_same_frames_at_the_same_rate() {
    if !hap_available() {
        return;
    }
    for name in ["gray25.mp4", "gray2997.mp4", "gray25.webm"] {
        let (_dir, dst, seen) = convert(name);
        assert_eq!(seen.last(), Some(&(50, 50)), "{name}");
        let mut source = VideoFile::open(&fixture(name)).unwrap();
        let mut copy = VideoFile::open(&dst).unwrap();
        let (a, b) = (source.info().clone(), copy.info().clone());
        assert_eq!(b.codec, "hap", "{name}");
        assert_eq!(
            (b.width, b.height, b.rate, b.frames),
            (a.width, a.height, a.rate, a.frames),
            "{name}"
        );
        assert!(!b.alpha && !b.has_audio, "{name}");
        // Out of order, as a seek: HAP's textures move a level by a step or two at most.
        for index in [49, 0, 24, 13, 1] {
            let (x, y) = (
                source.frame(index).unwrap().rgba[0],
                copy.frame(index).unwrap().rgba[0],
            );
            assert!(x.abs_diff(y) <= 3, "{name} frame {index}: {x} vs {y}");
        }
    }
}

#[test]
fn a_transparent_video_should_become_hap_alpha() {
    if !hap_available() {
        return;
    }
    let (_dir, dst, _) = convert("alpha25.webm");
    let mut copy = VideoFile::open(&dst).unwrap();
    assert!(copy.info().alpha);
    let frame = copy.frame(4).unwrap();
    assert!(frame.alpha);
    let pixel = |x: usize| &frame.rgba[x * 4..x * 4 + 4];
    assert!(pixel(5)[0] > 240 && pixel(5)[3] == 255, "{:?}", pixel(5));
    assert_eq!(pixel(50)[3], 0);
}

#[test]
fn a_hap_copy_should_keep_the_sound() {
    if !hap_available() {
        return;
    }
    for name in ["tone.mp4", "tone.webm"] {
        let (_dir, dst, _) = convert(name);
        assert!(VideoFile::open(&dst).unwrap().info().has_audio, "{name}");
        let wav = flux_video::audio_wav(&dst).unwrap().unwrap();
        let original = flux_video::audio_wav(&fixture(name)).unwrap().unwrap();
        // The same samples: PCM keeps what the mixer heard from the source.
        assert_eq!(wav.len(), original.len(), "{name}");
        assert_eq!(wav, original, "{name}");
    }
}

#[test]
fn a_cancelled_conversion_should_leave_nothing_behind() {
    if !hap_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("copy.mov");
    let result = convert_to_hap(
        &fixture("gray25.mp4"),
        &dst,
        &mut |_, _| {},
        &AtomicBool::new(true),
    );
    assert!(matches!(result, Err(VideoError::Cancelled)));
    assert!(!dst.exists());
}
