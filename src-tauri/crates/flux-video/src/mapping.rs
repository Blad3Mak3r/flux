use flux_core::FrameRate;
use flux_project::VideoClip;

/// A frame rate as a fraction, as a video stream carries it (e.g. 30000/1001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub num: u32,
    pub den: u32,
}

impl Rate {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }
}

/// Which frame of its video `clip` shows when the timeline is at `position` (frames from the
/// timeline origin, at `timeline`'s rate), or `None` when the clip shows nothing there: before its
/// start, after its length, or past the last of the video's `video_frames`.
///
/// The frame on screen at an instant is the last one that started at or before it, so the division
/// rounds down. The arithmetic is on integers: equal rates map frame for frame, 29.97 included.
pub fn source_frame(
    clip: &VideoClip,
    position: u64,
    timeline: FrameRate,
    video: Rate,
    video_frames: u64,
) -> Option<u64> {
    let into = position.checked_sub(clip.start)?;
    if clip.length.is_some_and(|length| into >= length) || video.num == 0 || video.den == 0 {
        return None;
    }
    let (tl_num, tl_den) = timeline.fraction();
    let at = u128::from(clip.offset + into);
    let index = at * u128::from(tl_den) * u128::from(video.num)
        / (u128::from(tl_num) * u128::from(video.den));
    u64::try_from(index)
        .ok()
        .filter(|&index| index < video_frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [(FrameRate, Rate); 4] = [
        (FrameRate::Fps24, Rate::new(24, 1)),
        (FrameRate::Fps25, Rate::new(25, 1)),
        (FrameRate::Fps2997Drop, Rate::new(30_000, 1_001)),
        (FrameRate::Fps30, Rate::new(30, 1)),
    ];

    fn clip(start: u64, offset: u64, length: Option<u64>) -> VideoClip {
        VideoClip {
            id: "c".into(),
            asset: "v".into(),
            start,
            name: None,
            offset,
            length,
        }
    }

    #[test]
    fn a_video_at_the_timeline_rate_should_map_frame_for_frame_at_every_rate() {
        for (timeline, video) in RATES {
            let clip = clip(100, 0, None);
            for frame in [0, 1, 29, 30, 1_799, 1_800, 17_982, 107_891] {
                assert_eq!(
                    source_frame(&clip, 100 + frame, timeline, video, u64::MAX),
                    Some(frame),
                    "{timeline:?} frame {frame}"
                );
            }
        }
    }

    #[test]
    fn the_clip_should_show_nothing_before_its_start_or_after_its_length() {
        let clip = clip(100, 0, Some(10));
        let rate = Rate::new(25, 1);
        assert_eq!(source_frame(&clip, 99, FrameRate::Fps25, rate, 50), None);
        assert_eq!(
            source_frame(&clip, 109, FrameRate::Fps25, rate, 50),
            Some(9)
        );
        assert_eq!(source_frame(&clip, 110, FrameRate::Fps25, rate, 50), None);
    }

    #[test]
    fn the_trim_in_should_skip_frames_of_the_video() {
        let clip = clip(100, 12, None);
        let rate = Rate::new(25, 1);
        assert_eq!(
            source_frame(&clip, 100, FrameRate::Fps25, rate, 50),
            Some(12)
        );
        assert_eq!(
            source_frame(&clip, 137, FrameRate::Fps25, rate, 50),
            Some(49)
        );
    }

    #[test]
    fn the_clip_should_end_with_the_last_frame_of_the_video() {
        let clip = clip(0, 0, None);
        let rate = Rate::new(25, 1);
        assert_eq!(
            source_frame(&clip, 49, FrameRate::Fps25, rate, 50),
            Some(49)
        );
        assert_eq!(source_frame(&clip, 50, FrameRate::Fps25, rate, 50), None);
    }

    #[test]
    fn a_video_at_another_rate_should_show_the_frame_that_started_last() {
        let clip = clip(0, 0, None);
        // A 25 fps video on a 30 fps timeline: timeline frame 6 is 0.2 s, video frame 5.
        let video = Rate::new(25, 1);
        let shown: Vec<_> = (0..7)
            .map(|f| source_frame(&clip, f, FrameRate::Fps30, video, 100).unwrap())
            .collect();
        assert_eq!(shown, [0, 0, 1, 2, 3, 4, 5]);
        // A 29.97 video on a 30 fps timeline lags by one frame every 1001.
        let video = Rate::new(30_000, 1_001);
        assert_eq!(
            source_frame(&clip, 1_000, FrameRate::Fps30, video, u64::MAX),
            Some(999)
        );
        assert_eq!(
            source_frame(&clip, 1_001, FrameRate::Fps30, video, u64::MAX),
            Some(1_000)
        );
    }

    #[test]
    fn a_seek_should_land_on_the_same_frame_as_playing_up_to_it() {
        // No state: the frame depends on the position alone, so a seek and playback agree.
        let clip = clip(1_000, 7, Some(5_000));
        for (timeline, video) in RATES {
            let played: Vec<_> = (1_000..6_000)
                .map(|p| source_frame(&clip, p, timeline, video, u64::MAX))
                .collect();
            for p in [5_999, 1_000, 3_333, 1_001, 4_000] {
                assert_eq!(
                    source_frame(&clip, p, timeline, video, u64::MAX),
                    played[(p - 1_000) as usize]
                );
            }
        }
    }

    #[test]
    fn a_rate_of_zero_should_show_nothing() {
        let clip = clip(0, 0, None);
        assert_eq!(
            source_frame(&clip, 0, FrameRate::Fps25, Rate::new(0, 1), 50),
            None
        );
    }
}
