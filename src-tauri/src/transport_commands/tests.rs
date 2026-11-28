use super::running::report;
use super::*;
use flux_engine::cues::Countdown;

fn at(frame: u64) -> Position {
    Position {
        state: TransportState::Playing,
        frame,
        position_samples: frame * 1_920,
        device_lost: false,
        ltc_lost: 0,
        mtc_lost: 0,
        metronome_lost: false,
        operator_lost: false,
        clock_unpaced: false,
    }
}

fn peak(level: f32) -> Vec<TrackPeak> {
    vec![TrackPeak {
        id: "t1".into(),
        peak: level,
    }]
}

#[test]
fn position_lane_should_send_the_first_position() {
    assert_eq!(PositionLane::default().next(at(1)), Some(at(1)));
}

#[test]
fn position_lane_should_not_repeat_a_position_that_did_not_move() {
    let mut lane = PositionLane::default();
    lane.next(at(1));
    assert_eq!(lane.next(at(1)), None);
    assert_eq!(lane.next(at(2)), Some(at(2)));
}

#[test]
fn reattached_position_lane_should_send_an_unchanged_position_to_the_new_page() {
    let (subscriptions, pending) = mpsc::channel();
    let (old_tx, old_rx) = mpsc::channel();
    let (new_tx, new_rx) = mpsc::channel();
    let channel = |tx: Sender<u64>| {
        Channel::<Position>::new(move |body| {
            let json = body.deserialize::<serde_json::Value>().unwrap();
            tx.send(json["frame"].as_u64().unwrap()).unwrap();
            Ok(())
        })
    };
    let mut step = report(pending, channel(old_tx), |lane: &mut PositionLane| {
        lane.next(at(17))
    });
    step();
    assert_eq!(old_rx.try_recv(), Ok(17));
    step();
    assert!(old_rx.try_recv().is_err());

    subscriptions.send(channel(new_tx)).unwrap();
    step();
    assert_eq!(new_rx.try_recv(), Ok(17));
    assert!(old_rx.try_recv().is_err());
}

fn state(next: &str) -> LiveState {
    LiveState {
        statuses: Vec::new(),
        current: None,
        next: Some(Countdown {
            cue_id: next.into(),
            remaining: 1,
            text: String::new(),
        }),
        running: Vec::new(),
        notes: Vec::new(),
    }
}

#[test]
fn live_lane_should_send_the_first_state() {
    assert_eq!(LiveLane::default().next(state("a")), Some(state("a")));
}

#[test]
fn live_lane_should_not_repeat_a_state_that_did_not_change() {
    let mut lane = LiveLane::default();
    lane.next(state("a"));
    assert_eq!(lane.next(state("a")), None);
    assert_eq!(lane.next(state("b")), Some(state("b")));
}

#[test]
fn levels_lane_should_send_sound() {
    let mut lane = LevelsLane::default();
    assert_eq!(lane.next(peak(0.5)), Some(peak(0.5)));
    assert_eq!(lane.next(peak(0.25)), Some(peak(0.25)));
}

#[test]
fn end_lane_should_say_so_once_per_end() {
    let mut lane = EndLane::new(0);

    assert!(!lane.ended(0));
    assert!(lane.ended(1));
    assert!(!lane.ended(1));
    assert!(lane.ended(2));
}

#[test]
fn end_lane_should_ignore_the_ends_before_the_output_opened() {
    let mut lane = EndLane::new(3);

    assert!(!lane.ended(3));
}

#[test]
fn levels_lane_should_send_silence_once_and_then_nothing() {
    let mut lane = LevelsLane::default();
    lane.next(peak(0.5));
    assert_eq!(lane.next(peak(0.0)), Some(peak(0.0)));
    assert_eq!(lane.next(peak(0.0)), None);
}

#[test]
fn levels_lane_should_stay_quiet_from_the_start_while_the_tracks_are_silent() {
    assert_eq!(LevelsLane::default().next(peak(0.0)), None);
}

#[test]
fn device_lane_should_not_look_for_a_device_that_is_not_lost() {
    let mut lane = DeviceLane::default();

    assert!(!lane.back(false, Instant::now(), || panic!("looked")));
}

#[test]
fn device_lane_should_look_once_a_second_and_announce_once() {
    let mut lane = DeviceLane::default();
    let start = Instant::now();

    assert!(!lane.back(true, start, || false));
    assert!(
        !lane.back(true, start + Duration::from_millis(500), || panic!(
            "too soon"
        ))
    );
    assert!(lane.back(true, start + DEVICE_CHECK_INTERVAL, || true));
    assert!(
        !lane.back(true, start + 3 * DEVICE_CHECK_INTERVAL, || panic!(
            "announced"
        ))
    );
}

#[test]
fn lanes_should_serialise_to_the_shape_the_ui_reads() {
    let position = serde_json::to_value(at(3)).unwrap();
    assert_eq!(position["frame"], 3);
    assert_eq!(position["positionSamples"], 3 * 1_920);
    assert_eq!(position["ltcLost"], 0);
    assert_eq!(position["mtcLost"], 0);
    assert_eq!(position["metronomeLost"], false);
    let levels = serde_json::to_value(peak(0.5)).unwrap();
    assert_eq!(levels[0]["id"], "t1");
}
