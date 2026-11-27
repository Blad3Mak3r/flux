//! Writes a few synthetic frames and a second of sound through the recorder and reads the file
//! back as Flux reads any video. Skipped where FFmpeg was built without HAP.

use std::sync::atomic::Ordering;

use flux_video::{RecordConfig, Recorder, VideoFile, hap_available};

/// A frame of one grey level.
fn grey(width: u32, height: u32, level: u8) -> Vec<u8> {
    [level, level, level, 255].repeat(width as usize * height as usize)
}

#[test]
fn a_recording_should_keep_its_rate_fill_gaps_and_carry_the_sound() {
    if !hap_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("take.mov");
    // 10 x 6 is not a multiple of 4: the file is 12 x 8.
    let recorder = Recorder::start(RecordConfig {
        path: path.clone(),
        size: (10, 6),
        rate: (25, 1),
        audio: Some((48_000, 2)),
    })
    .unwrap();
    recorder.push_frame(0, grey(10, 6, 40));
    recorder.push_frame(1, grey(10, 6, 80));
    // Ticks 2 and 3 never arrive.
    recorder.push_frame(4, grey(10, 6, 200));
    recorder.push_audio(vec![1000; 48_000 * 2]);
    let counters = std::sync::Arc::clone(recorder.counters());
    recorder.finish(6).unwrap();
    // 0, 1, the two missed, 4 and the tick the end asks for.
    assert_eq!(counters.written.load(Ordering::Relaxed), 6);
    assert_eq!(counters.dropped.load(Ordering::Relaxed), 3);

    let mut video = VideoFile::open(&path).unwrap();
    let info = video.info().clone();
    assert_eq!(info.codec, "hap");
    assert_eq!((info.width, info.height, info.frames), (12, 8, 6));
    assert_eq!((info.rate.num, info.rate.den), (25, 1));
    assert!(info.has_audio);
    for (frame, expected) in [(0, 40), (1, 80), (2, 80), (3, 80), (4, 200), (5, 200)] {
        let got = video.frame(frame).unwrap().rgba[(2 * 12 + 2) * 4];
        assert!(
            got.abs_diff(expected) <= 8,
            "frame {frame}: {got} vs {expected}"
        );
    }
}

#[test]
fn a_frame_of_the_wrong_size_should_be_left_to_the_repeat() {
    if !hap_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("take.mov");
    let recorder = Recorder::start(RecordConfig {
        path: path.clone(),
        size: (8, 8),
        rate: (30, 1),
        audio: None,
    })
    .unwrap();
    recorder.push_frame(0, grey(8, 8, 100));
    recorder.push_frame(1, vec![0; 5]);
    recorder.finish(2).unwrap();
    let video = VideoFile::open(&path).unwrap();
    assert_eq!(video.info().frames, 2);
    assert!(!video.info().has_audio);
}
