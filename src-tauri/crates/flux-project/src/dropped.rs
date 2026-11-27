//! Files dragged in from the file explorer: what each one is, and where it goes on a timeline.

use std::path::Path;

use super::error::{ProjectError, Result};
use super::model::{AudioAsset, AudioClip, Timeline, Track, TrackKind, VideoAsset, VideoClip};

const AUDIO: [&str; 6] = ["wav", "aif", "aiff", "flac", "mp3", "ogg"];
const VIDEO: [&str; 4] = ["mp4", "m4v", "mov", "webm"];
const IMAGE: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];

/// What a dropped file is, told by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
    Image,
}

/// The kind of file `path` names, or `None` when Flux does not take it.
pub fn media_kind(path: &Path) -> Option<MediaKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let is = |list: &[&str]| list.contains(&extension.as_str());
    if is(&AUDIO) {
        Some(MediaKind::Audio)
    } else if is(&VIDEO) {
        Some(MediaKind::Video)
    } else if is(&IMAGE) {
        Some(MediaKind::Image)
    } else {
        None
    }
}

/// A dropped file registered with the session, not yet in the project.
#[derive(Debug, Clone)]
pub enum Dropped {
    Audio(AudioAsset),
    Video(VideoAsset),
}

/// A dropped file and the frames it lasts on the timeline it goes to.
#[derive(Debug, Clone)]
pub struct Staged {
    pub media: Dropped,
    pub frames: u64,
}

/// Puts the files on `timeline` one after another from frame `at`: audio on the audio track
/// `track` and video on the video track `track`, or on a new track of the kind when `track` is not
/// one (or is none). The assets are for the caller to add to the project.
pub(crate) fn place(
    timeline: &mut Timeline,
    files: Vec<Staged>,
    track: Option<&str>,
    at: u64,
    mut new_id: impl FnMut() -> String,
) -> Result<()> {
    let named = |audio: bool| {
        timeline.tracks.iter().position(|t| {
            track == Some(t.id.as_str())
                && matches!(
                    (&t.kind, audio),
                    (TrackKind::Audio { .. }, true) | (TrackKind::Video { .. }, false)
                )
        })
    };
    let (mut audio, mut video) = (named(true), named(false));
    let mut cursor = at;
    for Staged { media, frames } in files {
        let (name, index, empty) = match &media {
            Dropped::Audio(asset) => (
                &asset.name,
                &mut audio,
                TrackKind::Audio { clips: Vec::new() },
            ),
            Dropped::Video(asset) => (
                &asset.name,
                &mut video,
                TrackKind::Video { clips: Vec::new() },
            ),
        };
        let index = *index.get_or_insert_with(|| {
            timeline.tracks.push(Track {
                audio_output: Default::default(),
                id: new_id(),
                name: name.clone(),
                muted: false,
                gain_db: None,
                color: None,
                kind: empty,
            });
            timeline.tracks.len() - 1
        });
        match (&mut timeline.tracks[index].kind, media) {
            (TrackKind::Audio { clips }, Dropped::Audio(asset)) => {
                clips.push(AudioClip::new(new_id(), asset.id, cursor));
            }
            (TrackKind::Video { clips }, Dropped::Video(asset)) => clips.push(VideoClip {
                id: new_id(),
                asset: asset.id,
                start: cursor,
                name: None,
                offset: 0,
                length: None,
                fit: Default::default(),
                fade_in: 0,
                fade_out: 0,
            }),
            _ => {
                return Err(ProjectError::Invalid(
                    "the track does not fit the file".into(),
                ));
            }
        }
        cursor += frames;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameRate, Session};

    fn audio(id: &str, frames: u64) -> Staged {
        Staged {
            media: Dropped::Audio(AudioAsset {
                id: id.into(),
                name: id.into(),
                file: format!("audio/{id}.wav"),
                bpm: None,
            }),
            frames,
        }
    }

    fn video(id: &str, frames: u64) -> Staged {
        Staged {
            media: Dropped::Video(VideoAsset {
                id: id.into(),
                name: id.into(),
                file: format!("video/{id}.mp4"),
            }),
            frames,
        }
    }

    fn timeline() -> Timeline {
        Session::default()
            .new_project("Show", FrameRate::Fps25)
            .timelines
            .remove(0)
    }

    fn counter() -> impl FnMut() -> String {
        let mut next = 0;
        move || {
            next += 1;
            format!("id{next}")
        }
    }

    #[test]
    fn media_kind_should_tell_files_by_extension_ignoring_case() {
        let kind = |name: &str| media_kind(Path::new(name));
        assert_eq!(
            [
                kind("a.WAV"),
                kind("b.mp4"),
                kind("c.png"),
                kind("d.txt"),
                kind("e")
            ],
            [
                Some(MediaKind::Audio),
                Some(MediaKind::Video),
                Some(MediaKind::Image),
                None,
                None
            ]
        );
    }

    #[test]
    fn place_should_chain_files_on_a_new_track_when_none_is_named() {
        let mut timeline = timeline();
        place(
            &mut timeline,
            vec![audio("a", 100), audio("b", 50)],
            None,
            10,
            counter(),
        )
        .unwrap();

        let [
            Track {
                kind: TrackKind::Audio { clips },
                ..
            },
        ] = &timeline.tracks[..]
        else {
            panic!("one audio track expected");
        };
        assert_eq!(
            clips
                .iter()
                .map(|c| (c.asset.as_str(), c.start))
                .collect::<Vec<_>>(),
            [("a", 10), ("b", 110)]
        );
    }

    #[test]
    fn place_should_use_the_named_track_of_the_right_kind_and_make_one_for_the_other() {
        let mut timeline = timeline();
        place(&mut timeline, vec![audio("a", 10)], None, 0, counter()).unwrap();
        let named = timeline.tracks[0].id.clone();

        place(
            &mut timeline,
            vec![audio("b", 10), video("v", 20)],
            Some(&named),
            100,
            counter(),
        )
        .unwrap();

        assert_eq!(timeline.tracks.len(), 2);
        let TrackKind::Audio { clips } = &timeline.tracks[0].kind else {
            panic!("audio track");
        };
        assert_eq!(clips.iter().map(|c| c.start).collect::<Vec<_>>(), [0, 100]);
        let TrackKind::Video { clips } = &timeline.tracks[1].kind else {
            panic!("video track");
        };
        assert_eq!(clips[0].start, 110);
    }

    #[test]
    fn place_should_make_a_new_track_when_the_named_one_is_of_another_kind() {
        let mut timeline = timeline();
        place(&mut timeline, vec![video("v", 10)], None, 0, counter()).unwrap();
        let named = timeline.tracks[0].id.clone();

        place(
            &mut timeline,
            vec![audio("a", 10)],
            Some(&named),
            5,
            counter(),
        )
        .unwrap();

        assert_eq!(timeline.tracks.len(), 2);
    }
}
