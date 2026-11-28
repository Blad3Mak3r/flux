//! The tempo map of a timeline: its tempo and its time signature as they change along it, and the
//! conversions between time, quarter notes, beats and bars that the ruler, the grid, the snap, the
//! metronome and the MIDI clock all share.
//!
//! The first point is the timeline's own [`Tempo`]; the others are its `tempo_changes`. A change
//! always starts a new bar and a new beat, so a segment counts its beats from where it starts. A
//! segment that ends in the middle of a beat has that beat cut short, and the next one begins at the
//! change (the file can only hold such a change by hand: the editing operations place them on bars).
//!
//! Everything is pure arithmetic on seconds from the timeline origin, so it needs no device.

use serde::Serialize;

use flux_core::FrameRate;

use crate::model::{MAX_BPM, MIN_BPM, Tempo, TempoChange, Timeline};

/// One stretch of the timeline with a fixed tempo and signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Seconds from the origin where it starts.
    pub start: f64,
    /// Quarter notes from the origin where it starts.
    pub start_quarters: f64,
    /// Beats from the origin where it starts; whole, a cut-short beat counts as one.
    pub start_beat: u64,
    /// The bar it starts, counted from 1.
    pub start_bar: u64,
    /// Quarter notes a minute.
    pub bpm: f64,
    pub beats_per_bar: u8,
    pub beat_unit: u8,
}

impl Segment {
    /// Quarter notes in a beat: 1 for a quarter, 0.5 for an eighth.
    pub fn quarters_per_beat(&self) -> f64 {
        4.0 / f64::from(self.beat_unit)
    }

    /// Seconds a beat lasts.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm * self.quarters_per_beat()
    }
}

/// Where a time falls on the bars: both counted from 1, and how far through the beat it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarBeat {
    pub bar: u64,
    pub beat: u64,
    /// From 0 up to, not including, 1.
    pub fraction: f64,
}

/// The tempo map of a timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    segments: Vec<Segment>,
    /// Whether the metronome plays while this timeline runs (the `click` of its tempo).
    pub click: bool,
}

/// A segment as the window draws it, in frames of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatSegment {
    /// Frames (real, not rounded) from the origin.
    pub start_frame: f64,
    pub beat_frames: f64,
    pub beats_per_bar: u8,
    pub beat_unit: u8,
    /// Quarter notes a minute.
    pub bpm: f64,
    /// The bar it starts, counted from 1.
    pub first_bar: u64,
}

fn clamp_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        120.0
    }
}

fn sane(beats_per_bar: u8, beat_unit: u8) -> (u8, u8) {
    (
        beats_per_bar.max(1),
        if beat_unit == 0 { 4 } else { beat_unit },
    )
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::single(Tempo::default())
    }
}

impl TempoMap {
    /// A map of one tempo for the whole timeline.
    pub fn single(tempo: Tempo) -> Self {
        Self::build(tempo, std::iter::empty())
    }

    /// The map of a timeline.
    pub fn of(timeline: &Timeline) -> Self {
        Self::new(
            timeline.tempo,
            &timeline.tempo_changes,
            timeline.timecode.frame_rate,
        )
    }

    /// The map of a first tempo and the changes after it, whose positions are frames at `rate`.
    pub fn new(first: Tempo, changes: &[TempoChange], rate: FrameRate) -> Self {
        let (num, den) = rate.fraction();
        let fps = num as f64 / den as f64;
        Self::build(
            first,
            changes.iter().map(|change| {
                (
                    change.at as f64 / fps,
                    change.bpm,
                    change.beats_per_bar,
                    change.beat_unit,
                )
            }),
        )
    }

    /// Builds from the first tempo and the changes `(seconds, bpm, beats per bar, beat unit)`,
    /// which are put in order; one that is not after the previous is dropped.
    fn build(first: Tempo, changes: impl Iterator<Item = (f64, f64, u8, u8)>) -> Self {
        let (beats_per_bar, beat_unit) = sane(first.beats_per_bar, first.beat_unit);
        let mut segments = vec![Segment {
            start: 0.0,
            start_quarters: 0.0,
            start_beat: 0,
            start_bar: 1,
            bpm: clamp_bpm(first.bpm),
            beats_per_bar,
            beat_unit,
        }];
        for (at, bpm, beats_per_bar, beat_unit) in changes {
            let last = *segments.last().expect("a map has a first segment");
            if at.is_nan() || at <= last.start {
                continue;
            }
            let (beats_per_bar, beat_unit) = sane(beats_per_bar, beat_unit);
            let beats = ((at - last.start) / last.beat_seconds()).ceil();
            let bars = (beats / f64::from(last.beats_per_bar)).ceil();
            segments.push(Segment {
                start: at,
                start_quarters: last.start_quarters + (at - last.start) * last.bpm / 60.0,
                start_beat: last.start_beat + beats as u64,
                start_bar: last.start_bar + bars as u64,
                bpm: clamp_bpm(bpm),
                beats_per_bar,
                beat_unit,
            });
        }
        Self {
            segments,
            click: first.click,
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether the tempo or the signature ever changes.
    pub fn is_constant(&self) -> bool {
        self.segments.len() == 1
    }

    /// The segment that holds `seconds` (the first one for anything before the origin).
    fn at_time(&self, seconds: f64) -> &Segment {
        let index = self.segments.partition_point(|s| s.start <= seconds);
        &self.segments[index.saturating_sub(1)]
    }

    fn at_quarters(&self, quarters: f64) -> &Segment {
        let index = self
            .segments
            .partition_point(|s| s.start_quarters <= quarters);
        &self.segments[index.saturating_sub(1)]
    }

    /// The tempo, in quarter notes a minute, at a time.
    pub fn bpm_at(&self, seconds: f64) -> f64 {
        self.at_time(seconds).bpm
    }

    /// Quarter notes from the origin at a time.
    pub fn quarters_at(&self, seconds: f64) -> f64 {
        let segment = self.at_time(seconds.max(0.0));
        segment.start_quarters + (seconds.max(0.0) - segment.start) * segment.bpm / 60.0
    }

    /// The time of a position in quarter notes from the origin.
    pub fn seconds_at_quarters(&self, quarters: f64) -> f64 {
        let quarters = quarters.max(0.0);
        let segment = self.at_quarters(quarters);
        segment.start + (quarters - segment.start_quarters) * 60.0 / segment.bpm
    }

    /// Where a time is on the bars.
    pub fn bar_beat_at(&self, seconds: f64) -> BarBeat {
        let seconds = seconds.max(0.0);
        let segment = self.at_time(seconds);
        let beats = (seconds - segment.start) / segment.beat_seconds();
        let whole = beats.floor();
        BarBeat {
            bar: segment.start_bar + (whole as u64) / u64::from(segment.beats_per_bar),
            beat: (whole as u64) % u64::from(segment.beats_per_bar) + 1,
            fraction: beats - whole,
        }
    }

    /// The time `beat` beats after `bar` starts (`beat` counts from 0), or `None` when that bar is
    /// not in the map (it falls after a change that starts a later one).
    pub fn seconds_at_bar(&self, bar: u64, beat: f64) -> Option<f64> {
        let index = self
            .segments
            .partition_point(|s| s.start_bar <= bar)
            .checked_sub(1)?;
        let segment = &self.segments[index];
        let beats = (bar - segment.start_bar) as f64 * f64::from(segment.beats_per_bar) + beat;
        let seconds = segment.start + beats * segment.beat_seconds();
        match self.segments.get(index + 1) {
            Some(next) if seconds >= next.start => None,
            _ => Some(seconds),
        }
    }

    /// Where the beat `index` (from 0, over the whole map) starts, in seconds.
    pub fn seconds_at_beat(&self, index: u64) -> f64 {
        let at = self
            .segments
            .partition_point(|s| s.start_beat <= index)
            .saturating_sub(1);
        let segment = &self.segments[at];
        segment.start + (index - segment.start_beat) as f64 * segment.beat_seconds()
    }

    /// The beat a time belongs to: the last one that started at or before it.
    pub fn beat_index_at(&self, seconds: f64) -> u64 {
        let seconds = seconds.max(0.0);
        let segment = self.at_time(seconds);
        segment.start_beat + ((seconds - segment.start) / segment.beat_seconds()).floor() as u64
    }

    /// Whether the beat `index` is the first of a bar.
    pub fn is_downbeat(&self, index: u64) -> bool {
        let at = self
            .segments
            .partition_point(|s| s.start_beat <= index)
            .saturating_sub(1);
        let segment = &self.segments[at];
        (index - segment.start_beat).is_multiple_of(u64::from(segment.beats_per_bar))
    }

    /// The bar line or the beat closest to a time, in seconds. `bars_only` leaves the beats out.
    pub fn nearest_line(&self, seconds: f64, bars_only: bool) -> f64 {
        let seconds = seconds.max(0.0);
        let segment = self.at_time(seconds);
        let step = if bars_only {
            segment.beat_seconds() * f64::from(segment.beats_per_bar)
        } else {
            segment.beat_seconds()
        };
        let inside = segment.start + ((seconds - segment.start) / step).round() * step;
        let Some(next) = self.segments.iter().find(|s| s.start > segment.start) else {
            return inside;
        };
        if inside >= next.start || (next.start - seconds).abs() < (inside - seconds).abs() {
            next.start
        } else {
            inside
        }
    }

    /// The segments in frames of a timeline at `fps` real frames a second, for the window to draw.
    pub fn beat_segments(&self, fps: f64) -> Vec<BeatSegment> {
        self.segments
            .iter()
            .map(|s| BeatSegment {
                start_frame: s.start * fps,
                beat_frames: s.beat_seconds() * fps,
                beats_per_bar: s.beats_per_bar,
                beat_unit: s.beat_unit,
                bpm: s.bpm,
                first_bar: s.start_bar,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempo(bpm: f64, beats: u8, unit: u8) -> Tempo {
        Tempo {
            bpm,
            beats_per_bar: beats,
            beat_unit: unit,
            click: true,
        }
    }

    /// 4/4 at 120 for 4 seconds (2 bars), then 3/4 at 60 for 3 seconds (1 bar), then 6/8 at 90.
    fn map() -> TempoMap {
        TempoMap::build(
            tempo(120.0, 4, 4),
            [(4.0, 60.0, 3, 4), (7.0, 90.0, 6, 8)].into_iter(),
        )
    }

    #[test]
    fn a_single_tempo_should_count_beats_from_the_origin() {
        let map = TempoMap::single(tempo(120.0, 4, 4));
        assert!(map.is_constant());
        assert_eq!(map.quarters_at(3.0), 6.0);
        assert_eq!(map.seconds_at_quarters(6.0), 3.0);
        assert_eq!(
            map.bar_beat_at(2.25),
            BarBeat {
                bar: 2,
                beat: 1,
                fraction: 0.5
            }
        );
        assert_eq!(map.seconds_at_bar(3, 0.0), Some(4.0));
    }

    #[test]
    fn quarters_and_seconds_should_convert_both_ways_across_changes() {
        let map = map();
        // 8 quarters in the first 4 seconds, 3 in the next 3 (60 BPM), then 1.5 a second.
        assert_eq!(map.quarters_at(4.0), 8.0);
        assert_eq!(map.quarters_at(7.0), 11.0);
        assert_eq!(map.quarters_at(9.0), 14.0);
        for seconds in [0.0, 1.3, 4.0, 5.5, 7.0, 8.2, 20.0] {
            let back = map.seconds_at_quarters(map.quarters_at(seconds));
            assert!(
                (back - seconds).abs() < 1e-9,
                "{seconds} came back as {back}"
            );
        }
    }

    #[test]
    fn bars_and_beats_should_follow_the_signature_changes() {
        let map = map();
        let at = |s| map.bar_beat_at(s);
        assert_eq!((at(0.0).bar, at(0.0).beat), (1, 1));
        assert_eq!((at(3.9).bar, at(3.9).beat), (2, 4));
        // The 3/4 bar starts at 4 s and lasts 3 s: beats a second.
        assert_eq!((at(4.0).bar, at(4.0).beat), (3, 1));
        assert_eq!((at(6.5).bar, at(6.5).beat), (3, 3));
        // 6/8 at 90 BPM: an eighth lasts 60 / 90 / 2 = 1/3 s.
        assert_eq!((at(7.0).bar, at(7.0).beat), (4, 1));
        assert_eq!((at(8.0).bar, at(8.0).beat), (4, 4));
    }

    #[test]
    fn a_bar_should_go_back_to_its_time() {
        let map = map();
        assert_eq!(map.seconds_at_bar(2, 0.0), Some(2.0));
        assert_eq!(map.seconds_at_bar(3, 0.0), Some(4.0));
        assert_eq!(map.seconds_at_bar(3, 2.0), Some(6.0));
        assert_eq!(map.seconds_at_bar(4, 3.0), Some(8.0));
        // The 3/4 bar is the last of its segment: its fourth beat is already in bar 4.
        assert_eq!(map.seconds_at_bar(3, 3.0), None);
        for bar in 1..8 {
            let time = map.seconds_at_bar(bar, 0.0).unwrap();
            assert_eq!(map.bar_beat_at(time + 1e-9).bar, bar);
        }
    }

    #[test]
    fn beats_should_be_numbered_across_the_whole_map() {
        let map = map();
        // 8 beats in the first segment, 3 in the second, then thirds of a second.
        assert_eq!(map.seconds_at_beat(8), 4.0);
        assert_eq!(map.seconds_at_beat(11), 7.0);
        assert!((map.seconds_at_beat(12) - (7.0 + 1.0 / 3.0)).abs() < 1e-12);
        assert_eq!(map.beat_index_at(6.99), 10);
        assert_eq!(map.beat_index_at(7.0), 11);
        assert!(map.is_downbeat(0) && map.is_downbeat(4) && !map.is_downbeat(5));
        assert!(map.is_downbeat(8) && !map.is_downbeat(9) && map.is_downbeat(11));
        assert!(map.is_downbeat(17) && !map.is_downbeat(16));
    }

    #[test]
    fn a_change_inside_a_beat_should_cut_it_short_and_start_a_new_bar() {
        // 120 BPM: a beat is half a second; the change at 1.2 s is in the third beat.
        let map = TempoMap::build(tempo(120.0, 4, 4), [(1.2, 60.0, 4, 4)].into_iter());
        let second = map.segments()[1];
        assert_eq!(second.start_beat, 3);
        assert_eq!(second.start_bar, 2);
        assert_eq!(map.seconds_at_beat(3), 1.2);
    }

    #[test]
    fn the_nearest_line_should_be_a_beat_or_a_bar_of_the_segment_it_is_in() {
        let map = map();
        assert_eq!(map.nearest_line(0.74, false), 0.5);
        assert_eq!(map.nearest_line(0.74, true), 0.0);
        assert_eq!(map.nearest_line(1.1, true), 2.0);
        assert_eq!(map.nearest_line(5.2, false), 5.0);
        // Closer to the start of the next segment than to the last beat of this one.
        assert_eq!(map.nearest_line(3.95, false), 4.0);
        assert_eq!(map.nearest_line(6.9, true), 7.0);
    }

    #[test]
    fn changes_out_of_order_should_be_left_out() {
        let map = TempoMap::build(
            tempo(120.0, 4, 4),
            [(2.0, 90.0, 4, 4), (2.0, 60.0, 4, 4), (1.0, 60.0, 4, 4)].into_iter(),
        );
        assert_eq!(map.segments().len(), 2);
    }

    #[test]
    fn the_segments_for_the_window_should_be_in_frames() {
        let segments = map().beat_segments(25.0);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].beat_frames, 12.5);
        assert_eq!(segments[1].start_frame, 100.0);
        assert_eq!(segments[1].first_bar, 3);
        assert_eq!(segments[2].beats_per_bar, 6);
        assert!((segments[2].beat_frames - 25.0 / 3.0).abs() < 1e-12);
    }
}
