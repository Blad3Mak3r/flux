//! Reads the sound of the `tone` fixtures: one second of a 440 Hz sine, AAC at 44.1 kHz in MP4
//! and Opus at 48 kHz in WebM, beside a grey picture.

use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn audio_wav_should_give_the_sound_the_mixer_can_play() {
    for name in ["tone.mp4", "tone.webm"] {
        let wav = flux_video::audio_wav(&fixture(name)).unwrap().unwrap();
        let decoded = flux_audio::decode(wav, Some("wav")).unwrap();
        let seconds = decoded.duration_seconds();
        assert!((0.95..1.1).contains(&seconds), "{name}: {seconds} s");
        let loudest = decoded
            .samples
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        assert!(loudest > 1000, "{name}: peak {loudest}");
    }
}

#[test]
fn audio_wav_should_say_when_a_video_has_no_sound() {
    assert!(
        flux_video::audio_wav(&fixture("gray25.mp4"))
            .unwrap()
            .is_none()
    );
}
