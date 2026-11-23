//! The deck driven by positions, as the engine clock would drive it, with the 25 fps fixture.

use std::path::Path;
use std::time::{Duration, Instant};

use flux_core::FrameRate;
use flux_project::{Timeline, Track, TrackKind, VideoClip};
use flux_video::{Cue, Shown, VideoDeck, VideoFile};

fn deck() -> VideoDeck {
    VideoDeck::new(|asset| {
        (asset == "v1")
            .then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gray25.mp4"))
    })
}

fn timeline(asset: &str) -> Timeline {
    let mut timeline = Timeline::new("Show", FrameRate::Fps25);
    timeline.tracks.push(Track {
        audio_output: Default::default(),
        id: "t".into(),
        name: "Screen".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind: TrackKind::Video {
            clips: vec![VideoClip {
                id: "c".into(),
                asset: asset.into(),
                start: 100,
                name: None,
                offset: 4,
                length: None,
                fit: Default::default(),
                fade_in: 0,
                fade_out: 0,
            }],
        },
    });
    timeline
}

/// Calls the deck at `cue` until its picture is the frame it asks for, as a render loop would.
fn settle(deck: &mut VideoDeck, timeline: &Timeline, cue: Cue) -> Vec<Shown> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = deck.show(timeline, cue);
        let ready = shown
            .iter()
            .all(|s| s.frame.as_ref().is_some_and(|f| f.index == s.layer.frame));
        if ready {
            return shown;
        }
        assert!(
            Instant::now() < deadline,
            "the deck never caught up at {cue:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn level(index: u64) -> u8 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gray25.mp4");
    VideoFile::open(&path).unwrap().frame(index).unwrap().rgba[0]
}

#[test]
fn the_picture_should_follow_the_position_through_play_seek_and_stop() {
    let timeline = timeline("v1");
    let mut deck = deck();

    // Before the clip: nothing, but its file is opened ahead.
    assert!(deck.show(&timeline, Cue::At(60)).is_empty());

    // Playing through the start of the clip, frame by frame.
    for position in 100..110 {
        let shown = settle(&mut deck, &timeline, Cue::At(position));
        assert_eq!(shown.len(), 1);
        let expected = position - 100 + 4;
        assert_eq!(shown[0].layer.frame, expected);
        assert_eq!(shown[0].frame.as_ref().unwrap().rgba[0], level(expected));
    }

    // A seek back and one forward land on their exact frames.
    for (position, expected) in [(130, 34), (101, 5), (145, 49)] {
        let shown = settle(&mut deck, &timeline, Cue::At(position));
        assert_eq!(shown[0].frame.as_ref().unwrap().index, expected);
    }

    // Past the end of the video, and stopped: black.
    assert!(deck.show(&timeline, Cue::At(146)).is_empty());
    assert!(deck.show(&timeline, Cue::Black).is_empty());
}

#[test]
fn a_missing_file_should_be_reported_and_show_nothing() {
    let timeline = timeline("gone");
    let mut deck = deck();
    assert!(deck.show(&timeline, Cue::At(100)).is_empty());
    assert_eq!(
        deck.failed().map(|(clip, _)| clip).collect::<Vec<_>>(),
        ["c"]
    );
}
