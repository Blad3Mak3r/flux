use super::beat::PlayLane;
use super::play::Report;
use super::*;

fn idle(beats: f64) -> GridState {
    GridState {
        playing: Vec::new(),
        column: None,
        crossfader: 0.5,
        beats,
        beats_paused: false,
        link_peers: None,
    }
}

#[test]
fn play_lane_should_send_the_first_state() {
    assert!(PlayLane::default().next(idle(0.2)).is_some());
}

#[test]
fn play_lane_should_send_only_when_a_beat_or_anything_else_changed() {
    let mut lane = PlayLane::default();
    lane.next(idle(0.2));
    assert_eq!(lane.next(idle(0.9)), None, "the same beat");
    assert_eq!(lane.next(idle(1.1)), Some(idle(1.1)), "the next beat");
    let mut moved = idle(1.2);
    moved.crossfader = 0.7;
    assert_eq!(lane.next(moved.clone()), Some(moved));
}

fn secs(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

#[test]
fn beats_should_hold_while_paused_and_go_on_from_there() {
    let mut grid = Grid::default();
    assert_eq!(grid.count(secs(1.0), 120.0), 2.0);
    grid.paused = true;
    assert_eq!(grid.count(secs(5.0), 120.0), 2.0, "held");
    grid.paused = false;
    assert_eq!(
        grid.count(secs(5.0), 120.0),
        2.0,
        "from the time it resumed"
    );
    assert_eq!(grid.count(secs(6.0), 120.0), 4.0);
}

fn running(seconds: u64) -> Grid {
    Grid {
        started: Instant::now() - Duration::from_secs(seconds),
        ..Grid::default()
    }
}

#[test]
fn set_paused_should_hold_the_count_of_the_clock_that_runs() {
    let mut grid = running(1);
    grid.set_paused(true, 120.0);
    let held = grid.beats(120.0);
    assert!((2.0..2.1).contains(&held));
    assert_eq!(grid.beats(120.0), held);
}

#[test]
fn a_grid_in_a_link_session_should_keep_the_phase_of_its_bar() {
    let link = Arc::new(flux_link::Link::new(120.0));
    let mut grid = Grid::default();
    grid.set_link(Some(Arc::clone(&link)), 120.0);
    // Counts that would drift from the session's phase are put back on it.
    for seconds in [0.3, 0.9, 1.4] {
        let beats = grid.count(secs(seconds), 133.0);
        let off = (beats - link.beat()).rem_euclid(4.0);
        assert!(off < 0.05 || 4.0 - off < 0.05, "{seconds}: {beats} {off}");
    }
    // A hold is the operator's own, and a resync would move no peer.
    grid.set_paused(true, 120.0);
    let held = grid.beat.1;
    grid.resync(120.0);
    assert_eq!(grid.beat.1, held);
}

#[test]
fn resync_should_land_on_the_nearest_bar() {
    let mut grid = running(3);
    grid.resync(120.0);
    assert_eq!(
        grid.beat.1, 8.0,
        "six beats and a bit is nearer eight than four"
    );
}

#[test]
fn nudge_should_change_the_pace_only_while_held() {
    let mut grid = Grid {
        pace: 1.0 + NUDGE,
        ..Grid::default()
    };
    assert!((grid.count(secs(1.0), 120.0) - 2.2).abs() < 1e-9);
    grid.pace = 1.0;
    assert!((grid.count(secs(2.0), 120.0) - 4.2).abs() < 1e-9);
    grid.nudge(-1, 120.0);
    assert_eq!(grid.pace, 1.0 - NUDGE);
    grid.nudge(0, 120.0);
    assert_eq!(grid.pace, 1.0);
}
use flux_project::playhead::Motion;
use flux_project::vj::{Direction, Tempo};

fn head(motion: Motion) -> Playhead {
    Playhead::launch(motion, Tick::default())
}

fn motion() -> Motion {
    Motion {
        speed: 1.0,
        direction: Direction::Forward,
        tempo: Tempo::Free,
        region: Region::default(),
        playback: Playback::Loop,
    }
}

fn at(seconds: f64, beats: f64) -> Tick {
    Tick {
        time: Duration::from_secs_f64(seconds),
        beats,
    }
}

#[test]
fn the_report_should_say_where_the_playhead_is_in_the_clip_and_in_its_region() {
    let region = Region {
        start: 0.5,
        end: 1.0,
    };
    let moving = head(Motion { region, ..motion() });
    // A video of 10 s: the region lasts 5.
    let report = Report::of(&moving, at(1.0, 0.0), 120.0, Some(10.0));
    assert!((report.position.unwrap() - 0.6).abs() < 1e-9);
    assert!((report.progress.unwrap() - 0.2).abs() < 1e-9);
    assert!((report.remaining.unwrap() - 4.0).abs() < 1e-9);
    assert!((report.rate - 1.0).abs() < 1e-9);
    assert!((report.pass_seconds.unwrap() - 5.0).abs() < 1e-9);
}

#[test]
fn the_report_should_count_down_to_the_edge_it_moves_towards_at_the_pace_it_has() {
    let reverse = head(Motion {
        direction: Direction::Reverse,
        speed: 2.0,
        ..motion()
    });
    let report = Report::of(&reverse, at(1.0, 0.0), 120.0, Some(10.0));
    assert!((report.progress.unwrap() - 0.8).abs() < 1e-9);
    assert!(
        (report.remaining.unwrap() - 4.0).abs() < 1e-9,
        "to the start, twice as fast"
    );
    assert!((report.rate + 2.0).abs() < 1e-9);
    // A ping-pong on its way back goes to the start although it plays forwards.
    let bounce = head(Motion {
        playback: Playback::PingPong,
        ..motion()
    });
    let report = Report::of(&bounce, at(12.5, 0.0), 120.0, Some(10.0));
    assert!((report.progress.unwrap() - 0.75).abs() < 1e-9);
    assert!((report.remaining.unwrap() - 7.5).abs() < 1e-9);
}

#[test]
fn the_report_should_follow_the_beats_and_be_still_while_paused_or_ended() {
    let beats = head(Motion {
        tempo: Tempo::Beats { beats: 8 },
        speed: 4.0,
        ..motion()
    });
    let report = Report::of(&beats, at(1.0, 2.0), 120.0, Some(10.0));
    assert!((report.pass_seconds.unwrap() - 4.0).abs() < 1e-9);
    assert!((report.progress.unwrap() - 0.25).abs() < 1e-9);
    assert!((report.remaining.unwrap() - 3.0).abs() < 1e-9);
    assert!((report.rate - 2.5).abs() < 1e-9, "10 s of video in 4 s");
    let paused = head(Motion {
        direction: Direction::Pause,
        ..motion()
    });
    let report = Report::of(&paused, at(5.0, 0.0), 120.0, Some(10.0));
    assert_eq!(
        (report.remaining, report.pass_seconds, report.rate),
        (None, None, 0.0)
    );
    assert_eq!(report.position, Some(0.0));
    let once = head(Motion {
        playback: Playback::Once,
        ..motion()
    });
    let report = Report::of(&once, at(11.0, 0.0), 120.0, Some(10.0));
    assert_eq!(
        (report.position, report.progress, report.remaining),
        (None, None, None)
    );
    let unknown = Report::of(&once, at(1.0, 0.0), 120.0, None);
    assert_eq!(
        (unknown.position, unknown.pass_seconds, unknown.rate),
        (None, None, 1.0)
    );
}
