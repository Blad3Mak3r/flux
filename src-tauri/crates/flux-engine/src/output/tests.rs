use super::*;
use crate::ltc::{LtcConfig, LtcStream};

const RATE: FrameRate = FrameRate::Fps25;

fn clock() -> Clock {
    Clock::new(48_000, RATE)
}

fn config() -> LtcConfig {
    LtcConfig {
        sample_rate: 48_000,
        level: 0.5,
        edge_samples: 0,
    }
}

fn tap(channel: usize, gain: f32) -> LtcTap {
    LtcTap { channel, gain }
}

/// A renderer for `channels` interface channels with LTC on 0-based channel `ltc_channel`.
fn renderer(channels: usize, ltc_channel: usize) -> (Renderer, Sender<Message>, Arc<SharedClock>) {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(SharedClock::new());
    let ltc = LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0),
        channels: vec![tap(ltc_channel, 1.0)],
    };
    let renderer = Renderer::new(
        Transport::new(clock(), 0),
        rx,
        Arc::new(ClipSet::default()),
        Vec::new(),
        Some(ltc),
        channels,
        Arc::clone(&shared),
    );
    (renderer, tx, shared)
}

fn channel_of(buffer: &[f32], channels: usize, channel: usize) -> Vec<f32> {
    buffer
        .chunks_exact(channels)
        .map(|frame| frame[channel])
        .collect()
}

fn spoken_clips() -> Arc<ClipSet> {
    spoken_clips_with(vec![16_384; 96_000])
}

fn routed_config() -> OutputConfig {
    use flux_project::routing::{ResolvedAudioBus, TrackRoute};
    let mut config = OutputConfig::default();
    for (id, device, channels) in [
        ("one", "Main", vec![1, 2]),
        ("two", "Main", vec![1, 2]),
        ("three", "Main", vec![1, 2]),
    ] {
        config.audio_routes.push(ResolvedAudioBus {
            id: id.into(),
            route: Some(flux_project::AudioOutput {
                device_name: device.into(),
                channels,
            }),
        });
        config.track_routes.push(TrackRoute {
            track_id: id.into(),
            output: Some(id.into()),
        });
    }
    config
}
fn routed_clips() -> Arc<ClipSet> {
    use crate::mixer::ClipPlayer;
    let clips = [24_576, 24_576, -24_576, 24_576]
        .into_iter()
        .enumerate()
        .map(|(track, sample)| {
            ClipPlayer::new(
                Arc::new(flux_audio::Decoded {
                    samples: vec![sample; 4096],
                    source_rate: ENGINE_RATE,
                }),
                0,
                1.0,
                track,
            )
        })
        .collect();
    Arc::new(ClipSet::new(
        clips,
        ["one", "two", "three", "unassigned"]
            .map(|id| (id.into(), 1.0))
            .into(),
    ))
}
#[test]
fn named_routes_sum_once_before_clamping_and_do_not_play_new_unassigned_tracks() {
    let config = routed_config();
    let (mut renderer, tx, _) = renderer(3, 2);
    renderer.clips = routed_clips();
    renderer.audio_lanes = crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 24];
    renderer.render(&mut buffer, 0);
    assert_eq!(channel_of(&buffer, 3, 0), vec![0.75; 8]);
    assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
    assert!(channel_of(&buffer, 3, 2).iter().all(|s| s.abs() > 0.0));
}
#[test]
fn named_routes_keep_mono_channels_separate_and_faders_work() {
    let mut config = routed_config();
    config.audio_routes[0].route.as_mut().unwrap().channels = vec![1];
    config.audio_routes[1].route.as_mut().unwrap().channels = vec![2];
    config.audio_routes[2].route = None;
    let clips = routed_clips();
    clips.levels().set_gain_db("one", -60.0);
    let (mut renderer, tx, _) = renderer(3, 2);
    renderer.clips = clips;
    renderer.audio_lanes = crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 24];
    renderer.render(&mut buffer, 0);
    assert_eq!(buffer[21], 0.0);
    assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
    assert!(
        renderer
            .clips
            .levels()
            .take_peaks()
            .iter()
            .any(|p| p.id == "two" && p.peak == 0.75)
    );
}
#[test]
fn ltc_collision_silences_the_whole_bus_and_missing_routes_never_fall_back() {
    let mut config = routed_config();
    config.audio_routes[1].route.as_mut().unwrap().device_name = "Gone".into();
    config.audio_routes[2].route = None;
    config.ltc.push(LtcOutput {
        device_name: "Main".into(),
        channel: 2,
        level: None,
        offset_ms: None,
    });
    assert!(
        crate::audio_routing::lanes(&config, "Main", 3, |a, b| a == b)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn named_output_devices_share_ltc_click_and_aliases_in_one_stream() {
    let mut config = routed_config();
    config.audio_routes[0].route.as_mut().unwrap().device_name = "Secondary".into();
    config.audio_routes[1].route.as_mut().unwrap().device_name = "Alias".into();
    config.audio = Some(flux_project::AudioOutput {
        device_name: "Main".into(),
        channels: vec![1, 2],
    });
    config.ltc.push(LtcOutput {
        device_name: "Secondary".into(),
        channel: 8,
        level: None,
        offset_ms: None,
    });
    let timeline = flux_project::Project::new("Show", RATE).timelines.remove(0);
    let plan = Plan::build(&config, &timeline, None, |a, b| {
        a == b || (["Secondary", "Alias"].contains(&a) && ["Secondary", "Alias"].contains(&b))
    });
    assert_eq!(plan.followers.len(), 1);
    assert_eq!(plan.followers[0].group.slots.len(), 1);
    assert_eq!(plan.master.device_hint, "Main");
}
#[test]
fn audio_route_health_reports_loss_without_a_bitmask_limit() {
    use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
    let flag = Arc::new(AtomicBool::new(false));
    let states: Vec<_> = (0..48)
        .map(|id| RouteHealth {
            state: AudioRouteState {
                id: id.to_string(),
                device: "D".into(),
                channels: vec![1],
                status: AudioRouteStatus::Ready,
            },
            lost: Some(Arc::clone(&flag)),
        })
        .collect();
    flag.store(true, Ordering::Relaxed);
    assert!(
        states
            .iter()
            .all(|s| s.state().status == AudioRouteStatus::Lost)
    );
}

#[test]
fn clip_edits_keep_active_routes_until_stationary_reconfiguration() {
    let mut active = routed_config();
    active.audio_routes.truncate(1);
    active.track_routes.truncate(1);
    active.audio_routes[0].route.as_mut().unwrap().channels = vec![1];
    let mut desired = active.clone();
    desired.audio_routes[0].route.as_mut().unwrap().channels = vec![2];
    let (mut renderer, tx, _) = renderer(3, 2);
    renderer.clips = routed_clips();
    renderer.audio_lanes = crate::audio_routing::lanes(&active, "Main", 3, |a, b| a == b);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 24];
    renderer.render(&mut buffer, 0);
    tx.send(Message::Clips(routed_clips())).unwrap();
    renderer.render(&mut buffer, 0);
    assert_eq!(channel_of(&buffer, 3, 0), vec![0.75; 8]);
    assert_eq!(channel_of(&buffer, 3, 1), vec![0.0; 8]);
    tx.send(Message::Transport(Command::Pause)).unwrap();
    renderer.render(&mut buffer, 0);
    let parked = renderer.transport.position_samples();
    assert!(buffer.iter().all(|s| *s == 0.0));
    renderer.audio_lanes = crate::audio_routing::lanes(&desired, "Main", 3, |a, b| a == b);
    renderer.transport.apply(Command::Restore {
        samples: parked,
        paused: true,
    });
    renderer.render(&mut buffer, 0);
    assert_eq!(renderer.transport.state(), TransportState::Paused);
    assert_eq!(renderer.transport.position_samples(), parked);
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut buffer, 0);
    assert_eq!(channel_of(&buffer, 3, 0), vec![0.0; 8]);
    assert_eq!(channel_of(&buffer, 3, 1), vec![0.75; 8]);
}

#[test]
fn unassigned_bus_health_is_not_mistaken_for_the_default_device() {
    use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
    let shared = SharedClock::new();
    shared.audio_health.write().unwrap().push(RouteHealth {
        state: AudioRouteState {
            id: "operator".into(),
            device: String::new(),
            channels: vec![],
            status: AudioRouteStatus::Missing,
        },
        lost: None,
    });
    mark_audio_routes(&shared, "", None, 2, &shared.lost);
    assert_eq!(
        shared.audio_health.read().unwrap()[0].state().status,
        AudioRouteStatus::Missing
    );
    // A reopened device must re-evaluate conflicts, not promote every available bus to Ready.
    let mut shared = SharedClock::new();
    shared.audio_config = routed_config();
    shared.audio_config.ltc.push(LtcOutput {
        device_name: "Main".into(),
        channel: 2,
        level: None,
        offset_ms: None,
    });
    shared.audio_health.write().unwrap().push(RouteHealth {
        state: AudioRouteState {
            id: "one".into(),
            device: "Main".into(),
            channels: vec![1, 2],
            status: AudioRouteStatus::Missing,
        },
        lost: None,
    });
    mark_audio_routes(&shared, "Main", None, 2, &shared.lost);
    assert_eq!(
        shared.audio_health.read().unwrap()[0].state().status,
        AudioRouteStatus::Conflict
    );
}

fn spoken_clips_with(samples: Vec<i16>) -> Arc<ClipSet> {
    use flux_project::{Alignment, AudioAsset, GeneratedVoice, Track, TrackKind, VoiceOrder};
    let mut project = flux_project::Project::new("Show", FrameRate::Fps25);
    project.assets.push(AudioAsset {
        id: "a".into(),
        name: "Voice".into(),
        file: "audio/voice.mp3".into(),
        link: None,
        bpm: None,
    });
    project.timelines[0].tracks.push(Track {
        audio_output: Default::default(),
        id: "voice".into(),
        name: "Operator".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind: TrackKind::Operator {
            voice: None,
            orders: vec![VoiceOrder {
                mode: Default::default(),
                synthesis_text: None,
                trim_start: 0,
                trim_end: 0,
                id: "o".into(),
                at: 0,
                dialogue: "Go".into(),
                alignment: Alignment::Start,
                generated: Some(GeneratedVoice {
                    origin: flux_project::operator::RecordingOrigin::Generated,
                    synthesis_text: None,
                    asset: "a".into(),
                    dialogue: "Go".into(),
                    voice: "v".into(),
                    model: flux_project::operator::VOICE_MODEL.into(),
                    samples: 48_000,
                }),
            }],
        },
    });
    Arc::new(
        super::super::mixer::build(&project, &project.timelines[0], |_| {
            Ok(Arc::new(flux_audio::Decoded {
                samples: samples.clone(),
                source_rate: ENGINE_RATE,
            }))
        })
        .set,
    )
}

#[test]
fn operator_renders_only_on_its_channels_and_obeys_transport() {
    let shared = Arc::new(SharedClock::new());
    let (tx, rx) = mpsc::channel();
    let clock = Clock::new(ENGINE_RATE, FrameRate::Fps25);
    let mut renderer = Renderer::new(
        Transport::new(clock, 0),
        rx,
        spoken_clips(),
        vec![0, 1],
        vec![],
        3,
        shared,
    );
    renderer.operator_channels = vec![2];
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 6];
    renderer.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
    tx.send(Message::Transport(Command::Pause)).unwrap();
    renderer.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0; 6]);
    tx.send(Message::Transport(Command::Seek(1))).unwrap();
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
}

#[test]
fn operator_follower_tracks_seek_and_silences_when_master_is_lost() {
    let shared = Arc::new(SharedClock::new());
    shared.update_operator(spoken_clips());
    let clock = Clock::new(ENGINE_RATE, FrameRate::Fps25);
    let mut follower = LtcFollower::new(vec![], 3, clock, Arc::clone(&shared));
    follower.operator_channels = vec![2];
    shared.publish(Snapshot {
        show: None,
        state: TransportState::Playing,
        epoch: 1,
        position: 100,
        stamp: 0,
    });
    let mut buffer = [0.0f32; 6];
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0, 0.0, 0.5, 0.0, 0.0, 0.5]);
    shared.publish(Snapshot {
        show: None,
        state: TransportState::Playing,
        epoch: 2,
        position: 0,
        stamp: 0,
    });
    follower.render(&mut buffer, 0);
    assert_eq!(follower.voice_cursor.next, Some(2.0));
    shared.lost.store(true, Ordering::Relaxed);
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0; 6]);
}

#[test]
fn operator_callback_keeps_playing_while_control_plane_is_locked() {
    let shared = Arc::new(SharedClock::new());
    shared.update_operator(spoken_clips());
    let mut follower = LtcFollower::new(vec![], 1, clock(), Arc::clone(&shared));
    follower.operator_channels = vec![0];
    shared.publish(Snapshot {
        show: None,
        state: TransportState::Playing,
        epoch: 1,
        position: 0,
        stamp: 0,
    });
    let locked = shared.operator_set.lock().unwrap();
    let mut buffer = [0.0f32; 256];
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, [0.5; 256]);
    drop(locked);
    shared.update_operator(Arc::new(ClipSet::default()));
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, [0.0; 256]);
}

#[test]
fn follower_corrects_device_drift_without_jumping_spoken_samples() {
    let mut cursor = VoiceCursor::default();
    let clips = ClipSet::default();
    let mut left = [0.0; MIX_CHUNK];
    let mut right = [0.0; MIX_CHUNK];
    let mut master = 0.0;
    // One hour at 100 ppm clock disagreement, in fixed callback blocks.
    for _ in 0..675_000 {
        let before = cursor.next;
        cursor.follow(master as u64, false);
        if let Some(before) = before {
            assert_eq!(cursor.next, Some(before));
        }
        assert!((cursor.next.unwrap() - master).abs() < 12.0);
        cursor.mix(&clips, &mut left, &mut right);
        master += MIX_CHUNK as f64 * 1.0001;
    }
    cursor.follow(0, true);
    assert_eq!(cursor.next, Some(0.0), "a seek resets immediately");
    assert_eq!(cursor.step, 1.0);
}

#[test]
fn follower_interpolates_stereo_continuously_across_callback_boundaries() {
    let clips = spoken_clips_with(
        (0..48_000)
            .flat_map(|i| {
                let v = (i % 1000) as i16;
                [v, -v]
            })
            .collect(),
    );
    let mut cursor = VoiceCursor {
        next: Some(3.25),
        step: 1.001,
    };
    let mut left = [0.0; MIX_CHUNK];
    let mut right = [0.0; MIX_CHUNK];
    for block in 0..2 {
        cursor.mix(&clips, &mut left, &mut right);
        for i in 0..MIX_CHUNK {
            let expected = (3.25 + (block * MIX_CHUNK + i) as f64 * 1.001) as f32 / 32768.0;
            assert!((left[i] - expected).abs() < 1e-7);
            assert!((right[i] + expected).abs() < 1e-7);
        }
    }
}

#[test]
fn consuming_play_does_not_clear_a_later_queued_play() {
    let (mut renderer, tx, shared) = renderer(1, 0);
    shared.pending_plays.store(2, Ordering::Release);
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut [0.0f32; 1], 0);
    assert_eq!(shared.pending_plays.load(Ordering::Acquire), 1);
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut [0.0f32; 1], 0);
    assert_eq!(shared.pending_plays.load(Ordering::Acquire), 0);
}

#[test]
fn operator_route_uses_one_stream_per_device_and_keeps_master_requirements() {
    let project = flux_project::Project::new("Show", FrameRate::Fps25);
    let mut output = OutputConfig {
        audio_routes: Vec::new(),
        track_routes: Vec::new(),
        audio: Some(flux_project::AudioOutput {
            device_name: "Main".into(),
            channels: vec![1, 2],
        }),
        operator: Some(flux_project::AudioOutput {
            device_name: "Main".into(),
            channels: vec![5],
        }),
        ..Default::default()
    };
    let plan = Plan::build(&output, &project.timelines[0], None, |a, b| a == b);
    assert_eq!(plan.master.needed, 5);
    assert!(plan.followers.is_empty());
    output.operator.as_mut().unwrap().device_name = "Booth".into();
    output.metronome = Some(flux_project::MetronomeOutput {
        device_name: "Booth".into(),
        channels: vec![2],
        gain_db: 0.0,
    });
    let plan = Plan::build(&output, &project.timelines[0], None, |a, b| a == b);
    assert_eq!(plan.master.needed, 2);
    assert_eq!(plan.followers.len(), 1);
    assert!(plan.followers[0].click.is_some());
    assert!(plan.own_click.is_none());
}

#[test]
fn unassigned_ltc_should_not_open_a_device_or_shift_later_output_indices() {
    let outputs = [
        LtcOutput {
            device_name: flux_project::UNASSIGNED_DEVICE.into(),
            channel: 1,
            level: None,
            offset_ms: None,
        },
        LtcOutput {
            device_name: "Sync".into(),
            channel: 8,
            level: None,
            offset_ms: None,
        },
    ];
    let plan = plan_ltc(&outputs, "Main", |a, b| a == b);
    assert!(plan.shared.is_empty());
    assert_eq!(plan.own.len(), 1);
    assert_eq!(plan.own[0].slots[0].outputs, vec![1]);
}

#[test]
fn render_should_be_silent_until_played() {
    let (mut renderer, _tx, shared) = renderer(2, 1);
    let mut buffer = [1.0f32; 256];
    renderer.render(&mut buffer, 10);
    assert!(buffer.iter().all(|&s| s == 0.0));
    let snapshot = shared.load();
    assert_eq!(snapshot.state, TransportState::Stopped);
    assert_eq!(snapshot.position, 0);
}

#[test]
fn render_should_put_ltc_on_its_channel_only() {
    let (mut renderer, tx, _shared) = renderer(4, 2);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 4 * 4_000];
    renderer.render(&mut buffer, 10);

    assert!(channel_of(&buffer, 4, 2).iter().any(|&s| s != 0.0));
    for other in [0, 1, 3] {
        assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
    }
}

#[test]
fn render_should_advance_the_position_by_the_frames_rendered() {
    let (mut renderer, tx, shared) = renderer(2, 0);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 480];
    renderer.render(&mut buffer, 10);
    renderer.render(&mut buffer, 20);
    let snapshot = shared.load();
    assert_eq!(snapshot.position, 960);
    assert_eq!(snapshot.state, TransportState::Playing);
    assert_eq!(snapshot.stamp, 20);
}

#[test]
fn render_should_go_silent_and_hold_the_position_when_paused() {
    let (mut renderer, tx, shared) = renderer(1, 0);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 4_000];
    renderer.render(&mut buffer, 10);
    tx.send(Message::Transport(Command::Pause)).unwrap();
    renderer.render(&mut buffer, 20);
    assert!(buffer.iter().all(|&s| s == 0.0));
    assert_eq!(shared.load().position, 4_000);
    assert_eq!(shared.load().state, TransportState::Paused);
}

#[test]
fn render_should_resync_ltc_after_a_seek() {
    let (mut renderer, tx, shared) = renderer(1, 0);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 1_000];
    renderer.render(&mut buffer, 10);
    let epoch = shared.load().epoch;

    // Frame 10 starts at sample 19 200: playing from there must begin LTC at once.
    tx.send(Message::Transport(Command::Seek(10))).unwrap();
    renderer.render(&mut buffer, 20);
    let snapshot = shared.load();
    assert_ne!(snapshot.epoch, epoch);
    assert_eq!(snapshot.position, 19_200 + 1_000);
    assert_ne!(buffer[0], 0.0, "LTC should start on the frame boundary");
}

#[test]
fn render_should_convert_to_the_device_sample_format() {
    let (mut renderer, tx, _shared) = renderer(1, 0);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0i16; 4_000];
    renderer.render(&mut buffer, 10);
    let peak = buffer.iter().map(|s| s.unsigned_abs()).max().unwrap();
    // Level 0.5 of full scale.
    assert!((16_000..=16_500).contains(&peak), "peak {peak}");
}

#[test]
fn render_should_drain_every_pending_command() {
    let (mut renderer, tx, shared) = renderer(1, 0);
    tx.send(Message::Transport(Command::Play)).unwrap();
    tx.send(Message::Transport(Command::Seek(10))).unwrap();
    tx.send(Message::Transport(Command::Pause)).unwrap();
    let mut buffer = [0.0f32; 100];
    renderer.render(&mut buffer, 10);
    assert_eq!(shared.load().state, TransportState::Paused);
    assert_eq!(shared.load().position, 19_200);
}

#[test]
fn estimated_position_should_follow_the_wall_clock_while_playing() {
    let snapshot = Snapshot {
        show: None,
        state: TransportState::Playing,
        epoch: 1,
        position: 48_000,
        stamp: 1_000_000_000,
    };
    // 10 ms later is 480 samples at 48 kHz.
    assert_eq!(snapshot.estimated_position(1_010_000_000, clock()), 48_480);
    assert_eq!(snapshot.estimated_position(1_000_000_000, clock()), 48_000);
    // A clock reading from before the stamp never goes backwards.
    assert_eq!(snapshot.estimated_position(500, clock()), 48_000);
}

#[test]
fn estimated_position_should_not_extrapolate_past_a_stalled_stream() {
    let snapshot = Snapshot {
        show: None,
        state: TransportState::Playing,
        epoch: 1,
        position: 0,
        stamp: 0,
    };
    // After 10 seconds without an update it only trusts the last 100 ms: 4 800 samples.
    assert_eq!(snapshot.estimated_position(10_000_000_000, clock()), 4_800);
}

#[test]
fn estimated_position_should_stand_still_when_not_playing() {
    for state in [TransportState::Stopped, TransportState::Paused] {
        let snapshot = Snapshot {
            show: None,
            state,
            epoch: 1,
            position: 777,
            stamp: 0,
        };
        assert_eq!(snapshot.estimated_position(5_000_000_000, clock()), 777);
    }
}

fn range(channels: u16, format: SampleFormat) -> ConfigRange {
    ConfigRange {
        channels,
        min_rate: 44_100,
        max_rate: 96_000,
        format,
    }
}

#[test]
fn pick_config_should_choose_the_fewest_channels_that_cover_the_need() {
    let ranges = [
        range(8, SampleFormat::F32),
        range(2, SampleFormat::F32),
        range(4, SampleFormat::F32),
    ];
    assert_eq!(
        pick_config(&ranges, 3, 48_000),
        Some((4, SampleFormat::F32))
    );
    assert_eq!(
        pick_config(&ranges, 1, 48_000),
        Some((2, SampleFormat::F32))
    );
}

#[test]
fn pick_config_should_prefer_float_then_wider_integers() {
    let ranges = [
        range(2, SampleFormat::I16),
        range(2, SampleFormat::I32),
        range(2, SampleFormat::F32),
    ];
    assert_eq!(
        pick_config(&ranges, 2, 48_000),
        Some((2, SampleFormat::F32))
    );
    assert_eq!(
        pick_config(&ranges[..2], 2, 48_000),
        Some((2, SampleFormat::I32))
    );
}

#[test]
fn pick_config_should_play_24_bit_between_32_and_16_bit_integers() {
    let ranges = [range(2, SampleFormat::I16), range(2, SampleFormat::I24)];
    assert_eq!(
        pick_config(&ranges, 2, 48_000),
        Some((2, SampleFormat::I24))
    );
    let ranges = [range(2, SampleFormat::I24), range(2, SampleFormat::I32)];
    assert_eq!(
        pick_config(&ranges, 2, 48_000),
        Some((2, SampleFormat::I32))
    );
}

#[test]
fn pick_config_should_skip_formats_it_cannot_write() {
    let ranges = [range(2, SampleFormat::U8), range(2, SampleFormat::F64)];
    assert_eq!(pick_config(&ranges, 2, 48_000), None);
}

#[test]
fn pick_config_should_require_the_sample_rate() {
    let mut only_44k = range(2, SampleFormat::F32);
    only_44k.max_rate = 44_100;
    assert_eq!(pick_config(&[only_44k], 2, 48_000), None);
    assert_eq!(pick_config(&[], 1, 48_000), None);
}

#[test]
fn pick_config_should_fail_when_the_device_has_too_few_channels() {
    assert_eq!(pick_config(&[range(2, SampleFormat::F32)], 8, 48_000), None);
}

use crate::mixer::ClipPlayer;
use flux_audio::Decoded;

/// A clip of constant stereo samples, as 16-bit values, that starts at the timeline origin.
fn clip(left: i16, right: i16, frames: usize) -> ClipSet {
    ClipSet::new(
        vec![ClipPlayer::new(
            Arc::new(Decoded {
                samples: (0..frames).flat_map(|_| [left, right]).collect(),
                source_rate: ENGINE_RATE,
            }),
            0,
            1.0,
            0,
        )],
        vec![("t1".into(), 1.0)],
    )
}

/// A renderer playing `clips` on the given 0-based channels, with LTC on `ltc` if any.
fn programme_renderer(
    channels: usize,
    programme: Vec<usize>,
    ltc: Option<usize>,
    clips: ClipSet,
) -> (Renderer, Sender<Message>, Arc<SharedClock>) {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(SharedClock::new());
    let ltc = ltc.map(|channel| LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0),
        channels: vec![tap(channel, 1.0)],
    });
    let renderer = Renderer::new(
        Transport::new(clock(), 0),
        rx,
        Arc::new(clips),
        programme,
        ltc,
        channels,
        Arc::clone(&shared),
    );
    (renderer, tx, shared)
}

fn show_entry(
    id: &str,
    level: i16,
    end: u64,
    transition: flux_project::Transition,
) -> crate::show::PreparedEntry {
    let mut timeline = Timeline::new(id, FrameRate::Fps25);
    timeline.id = id.into();
    let project = flux_project::Project::new("Test", FrameRate::Fps25);
    crate::show::PreparedEntry {
        cues: crate::cues::CueList::build(&project, &timeline, |_| None),
        timeline,
        entry: flux_project::SetlistEntry {
            id: id.into(),
            timeline_id: id.into(),
            name: String::new(),
            notes: String::new(),
            omitted: false,
            transition,
        },
        clips: Arc::new(clip(level, level, 10_000)),
        end,
    }
}

fn show_plan(entries: Vec<crate::show::PreparedEntry>) -> Arc<crate::show::ShowPlan> {
    Arc::new(crate::show::ShowPlan {
        overview: flux_project::show::SetlistOverview {
            id: "s".into(),
            name: "Show".into(),
            entries: vec![],
        },
        mix: Default::default(),
        execution_lock: Arc::new(AtomicBool::new(false)),
        setlist_id: "test".into(),
        entries,
    })
}

#[test]
fn show_switches_timelines_and_frame_rates_inside_a_buffer_without_a_gap() {
    let (mut renderer, tx, shared) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    let first = show_entry("one", 8192, 101, flux_project::Transition::Continue);
    let mut next = show_entry("two", 16384, 5000, flux_project::Transition::Wait);
    next.timeline.timecode.frame_rate = FrameRate::Fps30;
    next.timeline.timecode.start = 108_000;
    shared.show_plan.set(show_plan(vec![first, next])).unwrap();
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    let mut buffer = [0.0f32; 600];
    renderer.render(&mut buffer, 0);
    let left = channel_of(&buffer, 2, 0);
    assert_eq!(&left[..101], &[0.25; 101]);
    assert_eq!(&left[101..], &[0.5; 199]);
    assert_eq!(shared.load().show.unwrap().entry, 1);
    assert_eq!(renderer.transport.position_samples(), 199);
    assert_eq!(renderer.transport.start(), 108_000);
    assert_eq!(
        renderer.transport.clock(),
        Clock::new(48_000, FrameRate::Fps30)
    );
}

#[test]
fn show_journal_keeps_each_loop_interval_and_devamp_finishes_the_turn() {
    let (mut renderer, tx, shared) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    let mut entry = show_entry("vamp", 8192, 9000, flux_project::Transition::Wait);
    entry.timeline.controls.push(flux_project::TimelineControl {
        id: "loop".into(),
        name: "Vamp".into(),
        at: 0,
        kind: flux_project::ControlKind::Loop {
            end: 1,
            turns: None,
        },
    });
    shared.show_plan.set(show_plan(vec![entry])).unwrap();
    let (journal, intervals) = mpsc::sync_channel(32);
    shared.show_spans.set(journal).unwrap();
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    renderer.render(&mut vec![0.0f32; 4000 * 2], 0);
    let spans: Vec<_> = intervals.try_iter().collect();
    assert_eq!(
        spans
            .iter()
            .map(|s| (s.from, s.to, s.turn))
            .collect::<Vec<_>>(),
        vec![(0, 1920, 1), (0, 1920, 2), (0, 160, 3)]
    );
    tx.send(Message::Show(crate::show::ShowCommand::Devamp))
        .unwrap();
    renderer.render(&mut vec![0.0f32; 2000 * 2], 0);
    assert_eq!(renderer.transport.position_samples(), 2160);
    assert!(!shared.load().show.unwrap().devamp_pending);
}

#[test]
fn show_waits_at_an_exact_sample_and_duplicate_go_does_not_skip_entries() {
    let (mut renderer, tx, shared) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    let mut entry = show_entry("wait", 8192, 9000, flux_project::Transition::Wait);
    entry.timeline.controls.push(flux_project::TimelineControl {
        id: "wait".into(),
        name: "Hold".into(),
        at: 1,
        kind: flux_project::ControlKind::Wait,
    });
    shared.show_plan.set(show_plan(vec![entry])).unwrap();
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    let mut buffer = vec![0.0f32; 2000 * 2];
    renderer.render(&mut buffer, 0);
    assert!(buffer[..1920 * 2].iter().all(|s| *s == 0.25));
    assert!(buffer[1920 * 2..].iter().all(|s| *s == 0.0));
    assert_eq!(renderer.transport.position_samples(), 1920);
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    renderer.render(&mut buffer[..200], 0);
    assert_eq!(renderer.transport.position_samples(), 2020);
}

#[test]
fn show_journal_includes_terminal_events_once_without_adding_silence() {
    let (mut renderer, tx, shared) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    let first = show_entry("one", 8192, 100, flux_project::Transition::Continue);
    let second = show_entry("two", 16384, 100, flux_project::Transition::Wait);
    shared
        .show_plan
        .set(show_plan(vec![first, second]))
        .unwrap();
    let (journal, intervals) = mpsc::sync_channel(32);
    shared.show_spans.set(journal).unwrap();
    tx.send(Message::Show(crate::show::ShowCommand::Go))
        .unwrap();
    let mut buffer = [0.0f32; 600];
    renderer.render(&mut buffer, 0);
    let spans: Vec<_> = intervals.try_iter().collect();
    assert_eq!(
        spans
            .iter()
            .filter(|s| s.include_end)
            .map(|s| (s.entry, s.to))
            .collect::<Vec<_>>(),
        vec![(0, 100), (1, 100)]
    );
    assert!(spans.iter().filter(|s| s.release).all(|s| !s.include_end));
    assert_eq!(
        &channel_of(&buffer, 2, 0)[..200],
        &[vec![0.25; 100], vec![0.5; 100]].concat()
    );
}

#[test]
fn render_should_play_the_programme_on_its_channels_beside_ltc() {
    let (mut renderer, tx, _) =
        programme_renderer(4, vec![0, 1], Some(3), clip(16_384, -16_384, 1_000));
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 4 * 400];

    renderer.render(&mut buffer, 10);

    assert!(channel_of(&buffer, 4, 0).iter().all(|&s| s == 0.5));
    assert!(channel_of(&buffer, 4, 1).iter().all(|&s| s == -0.5));
    assert!(channel_of(&buffer, 4, 2).iter().all(|&s| s == 0.0));
    assert!(channel_of(&buffer, 4, 3).iter().any(|&s| s != 0.0));
}

#[test]
fn render_should_play_music_with_no_ltc_at_all() {
    let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, clip(8_192, 8_192, 100));
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 100];

    renderer.render(&mut buffer, 10);

    assert!(buffer.iter().all(|&s| s == 0.25));
}

#[test]
fn render_should_send_a_single_programme_channel_the_mono_sum() {
    let (mut renderer, tx, _) = programme_renderer(3, vec![2], None, clip(16_384, 0, 10));
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 3 * 10];

    renderer.render(&mut buffer, 10);

    assert!(channel_of(&buffer, 3, 2).iter().all(|&s| s == 0.25));
    assert!(channel_of(&buffer, 3, 0).iter().all(|&s| s == 0.0));
}

#[test]
fn render_should_not_play_the_programme_while_paused_or_stopped() {
    let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, clip(16_384, 16_384, 100));
    let mut buffer = [1.0f32; 2 * 100];
    renderer.render(&mut buffer, 10);
    assert!(buffer.iter().all(|&s| s == 0.0));

    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut buffer, 20);
    tx.send(Message::Transport(Command::Pause)).unwrap();
    renderer.render(&mut buffer, 30);
    assert!(buffer.iter().all(|&s| s == 0.0));
}

#[test]
fn render_should_clamp_a_programme_that_is_too_loud() {
    let loud = ClipSet::new(
        vec![
            ClipPlayer::new(
                Arc::new(Decoded {
                    samples: vec![i16::MAX; 20],
                    source_rate: ENGINE_RATE,
                }),
                0,
                1.0,
                0,
            );
            3
        ],
        vec![("t1".into(), 1.0)],
    );
    let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, loud);
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 10];

    renderer.render(&mut buffer, 10);

    assert!(buffer.iter().all(|&s| s == 1.0));
}

#[test]
fn render_should_switch_to_new_clips_sent_while_playing() {
    let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 10];
    renderer.render(&mut buffer, 10);
    assert!(buffer.iter().all(|&s| s == 0.0));

    // The transport is at sample 10 now; the clip starts at 0 and is long enough to cover it.
    tx.send(Message::Clips(Arc::new(clip(16_384, 16_384, 100))))
        .unwrap();
    renderer.render(&mut buffer, 20);

    assert!(buffer.iter().all(|&s| s == 0.5));
}

#[test]
fn render_should_stop_the_transport_at_the_end_of_the_show() {
    let (mut renderer, tx, shared) = programme_renderer(
        1,
        vec![0],
        Some(0),
        clip(8_192, 8_192, 100).with_end(Some(100)),
    );
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 60];
    renderer.render(&mut buffer, 10);
    assert_eq!(shared.load().state, TransportState::Playing);

    renderer.render(&mut buffer, 20);
    let snapshot = shared.load();
    assert_eq!(snapshot.state, TransportState::Stopped);
    assert_eq!(snapshot.position, 0);

    // Stopped for good: nothing sounds and the position stays at the start.
    renderer.render(&mut buffer, 30);
    assert!(buffer.iter().all(|&s| s == 0.0));
    assert_eq!(shared.load().position, 0);
}

#[test]
fn render_should_follow_an_end_sent_while_playing() {
    let (mut renderer, tx, shared) =
        programme_renderer(1, vec![0], None, ClipSet::default().with_end(Some(100)));
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 60];
    renderer.render(&mut buffer, 10);

    // The clips were edited: the show now ends later, so the first end passes unnoticed.
    tx.send(Message::Clips(Arc::new(
        ClipSet::default().with_end(Some(1_000)),
    )))
    .unwrap();
    renderer.render(&mut buffer, 20);
    assert_eq!(shared.load().state, TransportState::Playing);
    assert_eq!(shared.load().position, 120);
}

#[test]
fn render_should_not_stop_a_show_that_has_no_end() {
    let (mut renderer, tx, shared) = programme_renderer(1, vec![0], Some(0), ClipSet::default());
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 60];
    for tick in 0..100 {
        renderer.render(&mut buffer, tick);
    }
    assert_eq!(shared.load().state, TransportState::Playing);
}

#[test]
fn render_should_mix_a_buffer_longer_than_one_chunk() {
    let (mut renderer, tx, _) = programme_renderer(2, vec![0, 1], None, clip(8_192, 8_192, 1_000));
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 700];

    renderer.render(&mut buffer, 10);

    assert!(buffer.iter().all(|&s| s == 0.25));
}

#[test]
fn pace_watch_should_stay_quiet_for_a_device_that_keeps_time() {
    let mut watch = PaceWatch::default();
    let mut verdict = None;
    // 480 frames every 10 ms is exactly real time at 48 kHz.
    for tick in 0..=100u64 {
        verdict = watch.observe(tick * 10_000_000, 480).or(verdict);
    }
    assert_eq!(verdict, Some(false));
}

#[test]
fn pace_watch_should_flag_a_device_that_runs_far_faster_than_real_time() {
    let mut watch = PaceWatch::default();
    let mut verdict = None;
    // 300 times too fast, as ALSA's null device does.
    for tick in 0..=100u64 {
        verdict = watch.observe(tick * 10_000_000, 480 * 300).or(verdict);
    }
    assert_eq!(verdict, Some(true));
}

#[test]
fn pace_watch_should_say_nothing_before_a_second_has_passed() {
    let mut watch = PaceWatch::default();
    assert_eq!(watch.observe(0, 480), None);
    assert_eq!(watch.observe(500_000_000, 480 * 300), None);
}

#[test]
fn stream_failed_should_count_an_xrun_without_losing_the_device() {
    let clock = SharedClock::new();

    let flag = AtomicBool::new(false);
    clock.stream_failed(cpal::ErrorKind::Xrun, None);
    clock.stream_failed(cpal::ErrorKind::Xrun, Some(&flag));

    assert_eq!(clock.faults().xruns, 2);
    assert_eq!(clock.faults().stream_errors, 0);
    assert!(!clock.is_lost() && !flag.load(Ordering::Relaxed));
}

#[test]
fn stream_failed_should_lose_the_device_that_failed() {
    let clock = SharedClock::new();

    let flag = AtomicBool::new(false);

    clock.stream_failed(cpal::ErrorKind::DeviceNotAvailable, Some(&flag));

    assert_eq!(clock.faults().stream_errors, 1);
    assert!(flag.load(Ordering::Relaxed));
    assert!(!clock.is_lost());
}

fn lamps() -> (Lamps, Arc<AtomicBool>, Arc<AtomicBool>, Arc<AtomicBool>) {
    let [ltc, mtc, click] = std::array::from_fn(|_| Arc::new(AtomicBool::new(false)));
    let lamps = Lamps {
        programme_ltc: 0b001,
        programme_click: false,
        // Two LTC outputs share the second stream; the click has a stream of its own.
        ltc: vec![Lamp::new([1, 3], &ltc)],
        mtc: vec![Lamp::new([0, 2], &mtc)],
        click: Some(Arc::clone(&click)),
    };
    (lamps, ltc, mtc, click)
}

#[test]
fn health_should_report_nothing_lost_while_everything_runs() {
    let health = Health::default();
    health.replace(lamps().0);

    assert_eq!(health.ltc_lost(false), 0);
    assert_eq!(health.mtc_lost(), 0);
    assert!(!health.click_lost(false));
}

#[test]
fn health_should_mark_only_the_outputs_of_the_stream_that_failed() {
    let health = Health::default();
    let (all, ltc, mtc, click) = lamps();
    health.replace(all);

    ltc.store(true, Ordering::Relaxed);
    assert_eq!((health.ltc_lost(false), health.mtc_lost()), (0b1010, 0));

    mtc.store(true, Ordering::Relaxed);
    assert_eq!(health.mtc_lost(), 0b101);
    assert!(!health.click_lost(false));

    click.store(true, Ordering::Relaxed);
    assert!(health.click_lost(false));
}

#[test]
fn health_should_lose_the_outputs_on_the_programme_stream_with_it() {
    let health = Health::default();
    health.replace(Lamps {
        programme_ltc: 0b01,
        programme_click: true,
        ..lamps().0
    });

    assert_eq!(health.ltc_lost(true), 0b01);
    assert_eq!(health.ltc_lost(false), 0);
    let health = Health::default();
    health.replace(Lamps {
        programme_click: true,
        ..Lamps::default()
    });
    assert!(health.click_lost(true));
}

#[test]
fn health_should_forget_the_old_flags_when_replaced() {
    let health = Health::default();
    let (all, ltc, ..) = lamps();
    health.replace(all);
    ltc.store(true, Ordering::Relaxed);

    health.replace(Lamps::default());

    assert_eq!(health.ltc_lost(false), 0);
}

#[test]
fn lamp_should_give_no_bit_to_the_outputs_past_the_32nd() {
    let lost = Arc::new(AtomicBool::new(true));

    assert_eq!(Lamp::new([31, 32, 100], &lost).outputs, 1 << 31);
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn watchdog_should_stay_quiet_while_the_stream_keeps_calling_back() {
    let mut dog = Watchdog::new(ms(0));

    for step in 1..=100 {
        assert!(!dog.stalled(step, ms(step * 500)));
    }
}

#[test]
fn watchdog_should_flag_a_stream_that_stops_calling_back() {
    let mut dog = Watchdog::new(ms(0));
    assert!(!dog.stalled(10, ms(100)));

    assert!(!dog.stalled(10, ms(1_100)));
    assert!(dog.stalled(10, ms(1_101)));
}

#[test]
fn watchdog_should_give_a_stream_that_has_not_started_time_to_do_so() {
    let mut dog = Watchdog::new(ms(0));

    assert!(!dog.stalled(0, ms(2_900)));
    assert!(dog.stalled(0, ms(3_100)));
}

#[test]
fn supervisor_should_close_a_lost_follower_and_reopen_it_a_second_later() {
    let mut supervisor = Supervisor::new(ms(0), true);
    assert_eq!(supervisor.step(ms(100), 5, false), Step::Idle);

    assert_eq!(supervisor.step(ms(200), 6, true), Step::Lose);
    assert_eq!(supervisor.step(ms(900), 6, true), Step::Idle);
    assert_eq!(supervisor.step(ms(1_200), 6, true), Step::Reopen);

    // Still not there: it waits another second.
    supervisor.reopen_failed(ms(1_200));
    assert_eq!(supervisor.step(ms(2_000), 6, true), Step::Idle);
    assert_eq!(supervisor.step(ms(2_300), 6, true), Step::Reopen);

    supervisor.reopened(ms(2_300));
    assert_eq!(supervisor.step(ms(2_400), 7, false), Step::Idle);
}

#[test]
fn supervisor_should_lose_a_stream_that_went_silent_without_an_error() {
    let mut supervisor = Supervisor::new(ms(0), true);
    supervisor.step(ms(100), 5, false);

    assert_eq!(supervisor.step(ms(1_200), 5, false), Step::Lose);
}

#[test]
fn supervisor_should_report_the_programme_stream_once_and_never_reopen_it() {
    let mut supervisor = Supervisor::new(ms(0), false);

    assert_eq!(supervisor.step(ms(100), 0, true), Step::Lose);
    assert_eq!(supervisor.step(ms(5_000), 0, true), Step::Idle);
}

/// A MIDI port that takes messages until it is told to go away, and comes back when told to.
struct FakePort {
    present: Arc<AtomicBool>,
    sent: Sent,
    reconnects: u32,
}

impl MidiLink for FakePort {
    fn send(&mut self, message: &[u8]) -> bool {
        let present = self.present.load(Ordering::Relaxed);
        if present {
            self.sent.lock().unwrap().push(message.to_vec());
        }
        present
    }

    fn reconnect(&mut self) -> bool {
        self.reconnects += 1;
        self.present.load(Ordering::Relaxed)
    }
}

type Sent = Arc<Mutex<Vec<Vec<u8>>>>;

fn fake_port() -> (MtcPort<FakePort>, Arc<AtomicBool>, Sent) {
    let present = Arc::new(AtomicBool::new(true));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let link = FakePort {
        present: Arc::clone(&present),
        sent: Arc::clone(&sent),
        reconnects: 0,
    };
    let port = MtcPort::new(
        link,
        Arc::new(AtomicBool::new(false)),
        Arc::new(SharedClock::new()),
    );
    (port, present, sent)
}

const SECOND: u64 = 1_000_000_000;

#[test]
fn mtc_port_should_send_while_the_port_is_there() {
    let (mut port, _, sent) = fake_port();

    port.send(&[1, 2], 0);

    assert_eq!(*sent.lock().unwrap(), [vec![1, 2]]);
    assert!(!port.is_lost());
}

#[test]
fn mtc_port_should_be_lost_when_the_port_refuses_a_message_and_send_nothing_more() {
    let (mut port, present, sent) = fake_port();
    present.store(false, Ordering::Relaxed);

    port.send(&[1], 0);
    port.send(&[2], 1);

    assert!(port.is_lost());
    // Only the first refusal is counted: a lost port is not tried per message.
    assert_eq!(port.shared.faults().mtc_errors, 1);
    assert!(sent.lock().unwrap().is_empty());
}

#[test]
fn mtc_port_should_try_again_once_a_second_until_the_port_is_back() {
    let (mut port, present, sent) = fake_port();
    present.store(false, Ordering::Relaxed);
    port.send(&[1], 0);

    assert!(!port.retry(SECOND / 2));
    assert_eq!(port.link.reconnects, 0);
    assert!(!port.retry(SECOND));
    assert!(!port.retry(SECOND + 1));
    assert_eq!(port.link.reconnects, 1);

    present.store(true, Ordering::Relaxed);
    assert!(port.retry(2 * SECOND));
    assert!(!port.is_lost());
    port.send(&[3], 2 * SECOND);
    assert_eq!(*sent.lock().unwrap(), [vec![3]]);
}

#[test]
fn mtc_port_should_have_nothing_to_retry_while_it_works() {
    let (mut port, _, _) = fake_port();

    assert!(!port.retry(10 * SECOND));
    assert_eq!(port.link.reconnects, 0);
}

#[test]
fn render_should_publish_that_the_clock_is_unpaced() {
    let (mut renderer, tx, shared) = programme_renderer(2, vec![0, 1], None, ClipSet::default());
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 4_800];
    assert!(!shared.is_unpaced());

    // 100 ms of audio rendered every millisecond.
    for tick in 0..=1_000u64 {
        renderer.render(&mut buffer, tick * 1_000_000);
    }

    assert!(shared.is_unpaced());
}

/// One LTC lane running `offset` samples ahead of the transport, on `taps`.
fn lane(offset: i64, taps: Vec<LtcTap>) -> Vec<LtcChannel> {
    if taps.is_empty() {
        return Vec::new();
    }
    vec![LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0).with_offset(offset),
        channels: taps,
    }]
}

/// A follower writing mono LTC on channel 0, and the clock it follows.
fn follower() -> (LtcFollower, Arc<SharedClock>) {
    let shared = Arc::new(SharedClock::new());
    let follower = LtcFollower::new(lane(0, vec![tap(0, 1.0)]), 1, clock(), Arc::clone(&shared));
    (follower, shared)
}

fn master_at(shared: &SharedClock, state: TransportState, epoch: u64, position: u64) {
    shared.publish(Snapshot {
        show: None,
        state,
        epoch,
        position,
        stamp: 0,
    });
}

/// `len` samples of LTC started at transport position `position`.
fn ltc_from(position: u64, len: usize) -> Vec<f32> {
    let mut stream = LtcStream::new(config(), RATE, 0, 0);
    stream.resync(position);
    let mut out = vec![0.0; len];
    stream.fill(&mut out);
    out
}

#[test]
fn follower_should_be_silent_while_the_master_is_not_playing() {
    let (mut follower, shared) = follower();
    let mut buffer = [1.0f32; 4_000];
    follower.render(&mut buffer, 0);
    assert!(buffer.iter().all(|&s| s == 0.0));

    master_at(&shared, TransportState::Paused, 1, 9_000);
    follower.render(&mut buffer, 0);
    assert!(buffer.iter().all(|&s| s == 0.0));
}

#[test]
fn follower_should_play_the_masters_position_on_its_channel_only() {
    let shared = Arc::new(SharedClock::new());
    let mut follower =
        LtcFollower::new(lane(0, vec![tap(2, 1.0)]), 4, clock(), Arc::clone(&shared));
    master_at(&shared, TransportState::Playing, 1, 5_000);
    let mut buffer = [0.0f32; 4 * 4_000];
    follower.render(&mut buffer, 0);

    assert_eq!(channel_of(&buffer, 4, 2), ltc_from(5_000, 4_000));
    for other in [0, 1, 3] {
        assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
    }
}

#[test]
fn follower_should_keep_its_frames_whole_while_in_step_with_the_master() {
    let (mut follower, shared) = follower();
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut first = [0.0f32; 1_000];
    follower.render(&mut first, 0);

    // The master is 1 000 samples further, and the follower is where it should be.
    master_at(&shared, TransportState::Playing, 1, 1_000);
    let mut second = [0.0f32; 1_000];
    follower.render(&mut second, 0);

    let expected = ltc_from(0, 2_000);
    assert_eq!(first, expected[..1_000]);
    assert_eq!(second, expected[1_000..]);
}

#[test]
fn follower_should_ignore_a_difference_of_less_than_a_frame() {
    let (mut follower, shared) = follower();
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut first = [0.0f32; 1_000];
    follower.render(&mut first, 0);

    // A frame is 1 920 samples at 25 fps: the master is 900 ahead of where the follower is.
    master_at(&shared, TransportState::Playing, 1, 1_900);
    let mut second = [0.0f32; 1_000];
    follower.render(&mut second, 0);

    assert_eq!(second, ltc_from(0, 2_000)[1_000..]);
}

#[test]
fn follower_should_start_over_when_the_clocks_drift_a_frame_apart() {
    let (mut follower, shared) = follower();
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = [0.0f32; 1_000];
    follower.render(&mut buffer, 0);

    // The follower is at 1 000; the master reports 1 920 samples (one frame) beyond that.
    master_at(&shared, TransportState::Playing, 1, 2_920);
    follower.render(&mut buffer, 0);

    assert_eq!(buffer, ltc_from(2_920, 1_000)[..]);
}

#[test]
fn follower_should_start_over_after_a_seek() {
    let (mut follower, shared) = follower();
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = [0.0f32; 1_000];
    follower.render(&mut buffer, 0);

    // A seek lands where the follower would have been, but it is a new start all the same.
    master_at(&shared, TransportState::Playing, 2, 1_000);
    follower.render(&mut buffer, 0);

    assert_eq!(buffer, ltc_from(1_000, 1_000)[..]);
}

#[test]
fn follower_should_start_over_when_playing_resumes_after_a_pause() {
    let (mut follower, shared) = follower();
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = [0.0f32; 1_000];
    follower.render(&mut buffer, 0);
    master_at(&shared, TransportState::Paused, 1, 1_000);
    follower.render(&mut buffer, 0);

    master_at(&shared, TransportState::Playing, 1, 1_000);
    follower.render(&mut buffer, 0);

    assert_eq!(buffer, ltc_from(1_000, 1_000)[..]);
}

/// 1 ms is 48 samples at the engine rate.
const AHEAD: i64 = 480;

#[test]
fn follower_should_play_the_position_plus_its_offset() {
    let shared = Arc::new(SharedClock::new());
    let mut follower = LtcFollower::new(
        lane(AHEAD, vec![tap(0, 1.0)]),
        1,
        clock(),
        Arc::clone(&shared),
    );
    let mut buffer = [0.0f32; 4_000];
    master_at(&shared, TransportState::Playing, 1, 5_000);
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, ltc_from(5_000 + AHEAD as u64, 4_000)[..]);

    // A seek starts over from the new position, still shifted.
    master_at(&shared, TransportState::Playing, 2, 30_000);
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, ltc_from(30_000 + AHEAD as u64, 4_000)[..]);

    // So does resuming after a pause.
    master_at(&shared, TransportState::Paused, 2, 31_000);
    follower.render(&mut buffer, 0);
    master_at(&shared, TransportState::Playing, 2, 31_000);
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, ltc_from(31_000 + AHEAD as u64, 4_000)[..]);
}

#[test]
fn follower_should_play_a_negative_offset_behind_the_position() {
    let shared = Arc::new(SharedClock::new());
    let mut follower = LtcFollower::new(
        lane(-AHEAD, vec![tap(0, 1.0)]),
        1,
        clock(),
        Arc::clone(&shared),
    );
    let mut buffer = [0.0f32; 4_000];
    master_at(&shared, TransportState::Playing, 1, 5_000);
    follower.render(&mut buffer, 0);
    assert_eq!(buffer, ltc_from(5_000 - AHEAD as u64, 4_000)[..]);
}

#[test]
fn render_should_shift_ltc_by_the_offset_across_a_seek_and_a_stop() {
    let (tx, rx) = mpsc::channel();
    let ltc = LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0).with_offset(AHEAD),
        channels: vec![tap(0, 1.0)],
    };
    let mut renderer = Renderer::new(
        Transport::new(clock(), 0),
        rx,
        Arc::new(ClipSet::default()),
        Vec::new(),
        Some(ltc),
        1,
        Arc::new(SharedClock::new()),
    );
    let mut buffer = [0.0f32; 1_000];
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut buffer, 10);
    assert_eq!(buffer, ltc_from(AHEAD as u64, 1_000)[..]);

    // Frame 10 starts at sample 19 200.
    tx.send(Message::Transport(Command::Seek(10))).unwrap();
    renderer.render(&mut buffer, 20);
    assert_eq!(buffer, ltc_from(19_200 + AHEAD as u64, 1_000)[..]);

    // Stop goes back to the origin, and playing again starts the shifted signal over.
    tx.send(Message::Transport(Command::Stop)).unwrap();
    renderer.render(&mut buffer, 30);
    tx.send(Message::Transport(Command::Play)).unwrap();
    renderer.render(&mut buffer, 40);
    assert_eq!(buffer, ltc_from(AHEAD as u64, 1_000)[..]);
}

#[test]
fn ltc_lanes_should_share_a_generator_between_outputs_with_the_same_offset() {
    let send = |channel, offset_ms| LtcSend {
        channel,
        level: 1.0,
        offset_ms,
    };
    let lanes = ltc_lanes(
        &[send(1, 0), send(2, 5), send(3, 0)],
        config(),
        RATE,
        0,
        clock(),
    );
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0].channels.len(), 2);
    assert_eq!(lanes[1].channels.len(), 1);
    assert!(ltc_lanes(&[], config(), RATE, 0, clock()).is_empty());
}

#[test]
fn plan_ltc_should_take_the_offset_of_the_first_output_on_a_slot() {
    let mut early = ltc_out("Main", 3);
    early.offset_ms = Some(20);
    let mut late = ltc_out("Main", 3);
    late.offset_ms = Some(-20);
    let plan = plan_ltc(&[early, late, ltc_out("Main", 4)], "Main", loosely);
    let offsets: Vec<_> = sends_of(&plan.shared).iter().map(|s| s.offset_ms).collect();
    assert_eq!(offsets, vec![20, 0]);
}

#[test]
fn prune_should_free_only_the_sets_nothing_else_holds() {
    let playing = Arc::new(ClipSet::default());
    let old = Arc::new(ClipSet::default());
    // The callback holds `playing` through a clone; `old` was swapped out and released.
    let callback_copy = Arc::clone(&playing);
    let mut sets = vec![Arc::clone(&old), Arc::clone(&playing)];
    drop(old);

    prune_clip_sets(&mut sets);

    assert_eq!(sets.len(), 1);
    assert!(Arc::ptr_eq(&sets[0], &callback_copy));
}

#[test]
fn mtc_wait_should_park_until_the_spin_window_and_spin_inside_it() {
    let ms = Duration::from_millis;
    assert_eq!(mtc_wait(ms(8), true), MtcWait::Park(MTC_TICK));
    assert_eq!(mtc_wait(MTC_TICK, true), MtcWait::Park(MTC_TICK - MTC_SPIN));
    assert_eq!(mtc_wait(MTC_SPIN, true), MtcWait::Spin(MTC_SPIN));
}

#[test]
fn mtc_wait_should_never_spin_on_a_stale_position() {
    assert_eq!(mtc_wait(Duration::ZERO, false), MtcWait::Park(MTC_TICK));
}

#[test]
fn follower_should_follow_the_wall_clock_between_the_masters_updates() {
    let (mut follower, shared) = follower();
    // Published at 0 ns; 10 ms later the master has played 480 more samples.
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = [0.0f32; 1_000];
    follower.render(&mut buffer, 10_000_000);

    assert_eq!(buffer, ltc_from(480, 1_000)[..]);
}

#[test]
fn render_should_put_one_ltc_signal_on_every_channel_it_is_given() {
    let (tx, rx) = mpsc::channel();
    let shared = Arc::new(SharedClock::new());
    let ltc = LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0),
        channels: vec![tap(1, 1.0), tap(3, 1.0)],
    };
    let mut renderer = Renderer::new(
        Transport::new(clock(), 0),
        rx,
        Arc::new(ClipSet::default()),
        Vec::new(),
        Some(ltc),
        4,
        shared,
    );
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 4 * 400];
    renderer.render(&mut buffer, 10);

    let first = channel_of(&buffer, 4, 1);
    assert!(first.iter().any(|&s| s != 0.0));
    assert_eq!(first, channel_of(&buffer, 4, 3));
    for other in [0, 2] {
        assert!(channel_of(&buffer, 4, other).iter().all(|&s| s == 0.0));
    }
}

#[test]
fn render_should_scale_each_ltc_channel_by_its_level() {
    let (tx, rx) = mpsc::channel();
    let ltc = LtcChannel {
        stream: LtcStream::new(config(), RATE, 0, 0),
        channels: vec![tap(0, 1.0), tap(1, 0.25)],
    };
    let mut renderer = Renderer::new(
        Transport::new(clock(), 0),
        rx,
        Arc::new(ClipSet::default()),
        Vec::new(),
        Some(ltc),
        2,
        Arc::new(SharedClock::new()),
    );
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = [0.0f32; 2 * 400];
    renderer.render(&mut buffer, 10);

    let peak = |channel| {
        channel_of(&buffer, 2, channel)
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    };
    assert_eq!(peak(0), config().level);
    assert_eq!(peak(1), config().level * 0.25);
}

#[test]
fn plan_ltc_should_take_the_level_of_the_first_output_on_a_slot() {
    let mut quiet = ltc_out("Main", 3);
    quiet.level = Some(0.25);
    let mut loud = ltc_out("Main", 3);
    loud.level = Some(1.0);
    let plan = plan_ltc(&[quiet, loud, ltc_out("Main", 4)], "Main", loosely);
    let levels: Vec<_> = sends_of(&plan.shared)
        .iter()
        .map(|s| (s.channel, s.level))
        .collect();
    assert_eq!(levels, vec![(3, 0.25), (4, DEFAULT_LTC_LEVEL)]);
}

#[test]
fn follower_should_put_one_ltc_signal_on_every_channel_it_is_given() {
    let shared = Arc::new(SharedClock::new());
    let mut follower = LtcFollower::new(
        lane(0, vec![tap(0, 1.0), tap(2, 1.0)]),
        3,
        clock(),
        Arc::clone(&shared),
    );
    master_at(&shared, TransportState::Playing, 1, 5_000);
    let mut buffer = [0.0f32; 3 * 4_000];
    follower.render(&mut buffer, 0);

    assert_eq!(channel_of(&buffer, 3, 0), ltc_from(5_000, 4_000));
    assert_eq!(channel_of(&buffer, 3, 2), ltc_from(5_000, 4_000));
    assert!(channel_of(&buffer, 3, 1).iter().all(|&s| s == 0.0));
}

fn ltc_out(device: &str, channel: u16) -> LtcOutput {
    LtcOutput {
        device_name: device.into(),
        channel,
        level: None,
        offset_ms: None,
    }
}

/// Hints that differ only in case name the same device.
fn loosely(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn slot(channel: u16, outputs: &[usize]) -> LtcSlot {
    LtcSlot {
        channel,
        level: DEFAULT_LTC_LEVEL,
        offset_ms: 0,
        outputs: outputs.to_vec(),
    }
}

use flux_project::{AudioOutput, MetronomeOutput};

#[derive(Clone)]
struct RoutedProject {
    output: OutputConfig,
    timelines: Vec<Timeline>,
}

fn project_with(edit: impl FnOnce(&mut OutputConfig)) -> RoutedProject {
    let mut output = OutputConfig::default();
    edit(&mut output);
    RoutedProject {
        output,
        timelines: flux_project::Project::new("p", FrameRate::Fps25).timelines,
    }
}

fn plan_of(project: &RoutedProject) -> Plan {
    Plan::build(&project.output, &project.timelines[0], None, loosely)
}

fn main_device(name: &str) -> Option<AudioOutput> {
    Some(AudioOutput {
        device_name: name.into(),
        channels: vec![1, 2],
    })
}

fn mtc_out(port: &str) -> MtcOutput {
    MtcOutput {
        port_name: port.into(),
        port_index: 0,
        offset_ms: None,
    }
}

fn click_on(device: &str) -> Option<MetronomeOutput> {
    Some(MetronomeOutput {
        device_name: device.into(),
        channels: vec![1, 2],
        gain_db: 0.0,
    })
}

#[test]
fn unassigned_signals_stay_off_without_opening_branches() {
    let project = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out(flux_project::UNASSIGNED_DEVICE, 1)];
        o.mtc = vec![mtc_out("")];
    });
    let plan = plan_of(&project);
    let mut branches = Branches::default();
    let mut warnings = Vec::new();
    let opened = branches.apply(
        &plan,
        &BranchCtx::new(None, &project.timelines[0]),
        &Arc::new(SharedClock::new()),
        "Main",
        &mut warnings,
    );
    assert_eq!(opened.ltc.len(), 1);
    assert!(!opened.ltc[0].ok && !opened.mtc[0].ok);
    assert!(branches.followers.is_empty() && branches.mtc.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn pair_should_match_each_wanted_item_with_a_running_one_once() {
    let (paired, unused) = pair(&[1, 2, 2, 4], &[2, 5, 1, 2], |want, running| {
        want == running
    });

    assert_eq!(paired, [Some(2), Some(0), Some(3), None]);
    assert_eq!(unused, [1]);
}

#[test]
fn pair_should_leave_everything_unused_when_nothing_is_wanted() {
    let (paired, unused) = pair::<u8, u8>(&[], &[1, 2], |a, b| a == b);

    assert!(paired.is_empty());
    assert_eq!(unused, [0, 1]);
}

#[test]
fn plan_should_not_touch_the_programme_for_an_ltc_or_mtc_output_on_another_device() {
    let base = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Main", 3)];
    });
    let more = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Main", 3), ltc_out("Booth", 1)];
        o.mtc = vec![mtc_out("Port")];
    });

    let (before, after) = (plan_of(&base), plan_of(&more));

    assert_eq!(before.master, after.master);
    assert_eq!(before.click, after.click);
    assert_eq!(after.followers.len(), 1);
    assert_eq!(after.mtc.len(), 1);
}

#[test]
fn plan_should_not_mind_where_in_the_list_an_other_device_output_sits() {
    let first = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Main", 3), ltc_out("Booth", 1)];
    });
    let moved = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Stage", 1), ltc_out("Main", 3), ltc_out("Booth", 1)];
    });

    assert_eq!(plan_of(&first).master, plan_of(&moved).master);
}

#[test]
fn plan_should_ask_for_the_programme_to_reopen_when_ltc_on_its_device_changes() {
    let base = project_with(|o| o.audio = main_device("Main"));
    let with_ltc = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Main", 3)];
    });

    assert_ne!(plan_of(&base).master, plan_of(&with_ltc).master);
}

#[test]
fn plan_should_ask_for_the_programme_to_reopen_when_its_device_channels_or_clock_change() {
    let base = project_with(|o| o.audio = main_device("Main"));
    let device = project_with(|o| o.audio = main_device("Other"));
    let channels = project_with(|o| {
        o.audio = Some(AudioOutput {
            device_name: "Main".into(),
            channels: vec![3, 4],
        })
    });
    let mut rate = base.clone();
    rate.timelines[0].timecode.frame_rate = FrameRate::Fps30;
    let mut start = base.clone();
    start.timelines[0].timecode.start = 90_000;

    let base = plan_of(&base).master;
    for changed in [&device, &channels, &rate, &start] {
        assert_ne!(base, plan_of(changed).master);
    }
}

#[test]
fn plan_should_ask_for_a_new_output_when_the_metronome_changes() {
    let off = project_with(|o| o.audio = main_device("Main"));
    let on = project_with(|o| {
        o.audio = main_device("Main");
        o.metronome = click_on("Booth");
    });
    let louder = project_with(|o| {
        o.audio = main_device("Main");
        o.metronome = Some(MetronomeOutput {
            gain_db: -6.0,
            ..click_on("Booth").unwrap()
        });
    });

    assert_ne!(plan_of(&off).click, plan_of(&on).click);
    assert_ne!(plan_of(&on).click, plan_of(&louder).click);
    assert_eq!(plan_of(&on).click, plan_of(&on).click);
}

#[test]
fn plan_should_follow_the_metronome_to_the_stream_it_plays_on() {
    let project = project_with(|o| {
        o.audio = main_device("Main");
        o.ltc = vec![ltc_out("Booth", 1)];
        o.metronome = click_on("booth");
    });

    let plan = plan_of(&project);

    assert_eq!(
        plan.click.map(|click| click.place),
        Some(ClickPlaceKey::Group("Booth".into()))
    );
    assert!(plan.followers[0].click.is_some());
    assert!(plan.own_click.is_none() && plan.programme_click.is_none());
}

#[test]
fn plan_should_keep_the_running_metronome_for_the_streams_it_opens() {
    let project = project_with(|o| {
        o.audio = main_device("Main");
        o.metronome = click_on("Booth");
    });
    let running = plan_of(&project).metronome.unwrap();

    let plan = Plan::build(
        &project.output,
        &project.timelines[0],
        Some(Arc::clone(&running)),
        loosely,
    );

    assert!(Arc::ptr_eq(&plan.metronome.unwrap(), &running));
    assert!(Arc::ptr_eq(&plan.own_click.unwrap().1.shared, &running));
}

#[test]
fn plan_ltc_should_split_outputs_between_the_programme_stream_and_other_devices() {
    let outputs = [
        ltc_out("Main", 3),
        ltc_out("Booth", 1),
        ltc_out("MAIN", 4),
        ltc_out("booth", 2),
        ltc_out("Stage", 1),
    ];
    let plan = plan_ltc(&outputs, "main", loosely);
    assert_eq!(plan.shared, vec![slot(3, &[0]), slot(4, &[2])]);
    assert_eq!(
        plan.own,
        vec![
            LtcGroup {
                device: "Booth".into(),
                slots: vec![slot(1, &[1]), slot(2, &[3])],
            },
            LtcGroup {
                device: "Stage".into(),
                slots: vec![slot(1, &[4])],
            },
        ]
    );
}

#[test]
fn plan_ltc_should_play_a_repeated_device_and_channel_once_for_all_its_outputs() {
    let outputs = [
        ltc_out("Main", 3),
        ltc_out("Main", 3),
        ltc_out("Booth", 1),
        ltc_out("Booth", 1),
    ];
    let plan = plan_ltc(&outputs, "Main", loosely);
    assert_eq!(plan.shared, vec![slot(3, &[0, 1])]);
    assert_eq!(plan.own.len(), 1);
    assert_eq!(plan.own[0].slots, vec![slot(1, &[2, 3])]);
    assert_eq!(sends_of(&plan.shared).len(), 1);
}

#[test]
fn plan_ltc_should_be_empty_without_outputs() {
    let plan = plan_ltc(&[], "Main", loosely);
    assert!(plan.shared.is_empty() && plan.own.is_empty());
}

#[test]
fn output_info_should_serialise_the_outputs_as_lists() {
    let info = OutputInfo {
        operator: None,
        midi_clock: None,
        device: "Main".into(),
        sample_rate: 48_000,
        channels: 4,
        ltc: vec![LtcInfo {
            device: "Main".into(),
            channel: 3,
            ok: true,
        }],
        mtc: vec![MtcInfo {
            port: "IAC".into(),
            ok: false,
        }],
        cues: vec![],
        metronome: Some(MetronomeInfo {
            device: "Band".into(),
            ok: true,
        }),
        warnings: Vec::new(),
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["ltc"][0]["channel"], 3);
    assert_eq!(json["ltc"][0]["ok"], true);
    assert_eq!(json["mtc"][0]["port"], "IAC");
    assert_eq!(json["mtc"][0]["ok"], false);
    assert_eq!(json["metronome"]["device"], "Band");
}

fn click_out(bpm: f64, click: bool, channels: Vec<usize>) -> ClickOut {
    ClickOut {
        shared: Arc::new(SharedMetronome::new(
            Tempo {
                bpm,
                beats_per_bar: 4,
                click,
            },
            0.0,
        )),
        channels,
    }
}

/// A programme renderer that also plays a metronome, over `clips`.
fn clicking_renderer(
    channels: usize,
    programme: Vec<usize>,
    click: ClickOut,
    clips: ClipSet,
) -> (Renderer, Sender<Message>) {
    let (renderer, tx, _) = programme_renderer(channels, programme, None, clips);
    (renderer.with_click(Some(click)), tx)
}

#[test]
fn render_should_click_on_its_own_channel_beside_the_programme() {
    let (mut renderer, tx) = clicking_renderer(
        4,
        vec![0, 1],
        click_out(120.0, true, vec![2]),
        clip(16_384, -16_384, 24_000),
    );
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = vec![0.0f32; 4 * 4_000];

    renderer.render(&mut buffer, 10);

    assert!(channel_of(&buffer, 4, 0).iter().all(|&s| s == 0.5));
    assert!(channel_of(&buffer, 4, 2).iter().any(|&s| s.abs() > 0.1));
    assert!(channel_of(&buffer, 4, 3).iter().all(|&s| s == 0.0));
}

#[test]
fn render_should_add_the_click_to_a_programme_channel_it_shares() {
    let (mut renderer, tx) = clicking_renderer(
        2,
        vec![0, 1],
        click_out(120.0, true, vec![0]),
        clip(8_192, 8_192, 24_000),
    );
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = vec![0.0f32; 2 * 2_000];

    renderer.render(&mut buffer, 10);

    let shared = channel_of(&buffer, 2, 0);
    assert!(
        shared.iter().any(|&s| (s - 0.25).abs() > 0.05),
        "no click on it"
    );
    assert!(channel_of(&buffer, 2, 1).iter().all(|&s| s == 0.25));
}

#[test]
fn render_should_not_click_while_the_click_is_off_or_the_show_is_not_playing() {
    let click = click_out(120.0, false, vec![1]);
    let shared = Arc::clone(&click.shared);
    let (mut renderer, tx) = clicking_renderer(2, vec![0], click, ClipSet::default());
    tx.send(Message::Transport(Command::Play)).unwrap();
    let mut buffer = vec![0.0f32; 2 * 2_000];
    renderer.render(&mut buffer, 10);
    assert!(channel_of(&buffer, 2, 1).iter().all(|&s| s == 0.0));

    // Switching it on while the show runs makes it sound without rebuilding anything.
    shared.set(Tempo {
        bpm: 120.0,
        beats_per_bar: 4,
        click: true,
    });
    tx.send(Message::Transport(Command::Seek(0))).unwrap();
    renderer.render(&mut buffer, 20);
    assert!(channel_of(&buffer, 2, 1).iter().any(|&s| s.abs() > 0.1));

    tx.send(Message::Transport(Command::Pause)).unwrap();
    renderer.render(&mut buffer, 30);
    assert!(buffer.iter().all(|&s| s == 0.0));
}

/// A follower on a 2-channel device with LTC on channel 0 (or none) and the click on channel 1.
fn clicking_follower(ltc: bool) -> (LtcFollower, Arc<SharedClock>) {
    let shared = Arc::new(SharedClock::new());
    let follower = LtcFollower::new(
        lane(0, if ltc { vec![tap(0, 1.0)] } else { Vec::new() }),
        2,
        clock(),
        Arc::clone(&shared),
    )
    .with_click(Some(click_out(120.0, true, vec![1])));
    (follower, shared)
}

#[test]
fn follower_should_click_in_step_with_the_masters_position() {
    let (mut follower, shared) = clicking_follower(true);
    // The master is exactly on beat 1 of 120 BPM (24 000 samples).
    master_at(&shared, TransportState::Playing, 1, 24_000);
    let mut buffer = vec![0.0f32; 2 * 2_000];

    follower.render(&mut buffer, 0);

    let click = channel_of(&buffer, 2, 1);
    assert!(
        click[..200].iter().any(|&s| s.abs() > 0.1),
        "no click on the beat"
    );
    assert!(
        channel_of(&buffer, 2, 0).iter().any(|&s| s != 0.0),
        "no LTC beside it"
    );
}

#[test]
fn follower_with_no_ltc_should_play_only_the_click() {
    let (mut follower, shared) = clicking_follower(false);
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = vec![0.0f32; 2 * 2_000];

    follower.render(&mut buffer, 0);

    assert!(channel_of(&buffer, 2, 0).iter().all(|&s| s == 0.0));
    assert!(channel_of(&buffer, 2, 1).iter().any(|&s| s.abs() > 0.1));
}

#[test]
fn follower_should_keep_its_own_count_across_buffers_while_in_step() {
    let (mut follower, shared) = clicking_follower(false);
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut first = vec![0.0f32; 2 * 4_000];
    follower.render(&mut first, 0);
    // The master has not published again, but 4 000 samples of time (83.3 ms) have gone by: the
    // follower is where it expects to be, so it carries on instead of starting over.
    let mut second = vec![0.0f32; 2 * 4_000];
    follower.render(&mut second, 83_333_333);

    let expected = SharedMetronome::new(
        Tempo {
            bpm: 120.0,
            beats_per_bar: 4,
            click: true,
        },
        0.0,
    )
    .grid(ENGINE_RATE)
    .unwrap();
    let mut whole = vec![0.0f32; 8_000];
    expected.add(0, &mut whole);
    let mut heard = channel_of(&first, 2, 1);
    heard.extend(channel_of(&second, 2, 1));
    assert_eq!(heard, whole);
}

#[test]
fn follower_should_be_silent_when_the_master_stops() {
    let (mut follower, shared) = clicking_follower(false);
    master_at(&shared, TransportState::Playing, 1, 0);
    let mut buffer = vec![0.0f32; 2 * 2_000];
    follower.render(&mut buffer, 0);
    master_at(&shared, TransportState::Stopped, 2, 0);
    follower.render(&mut buffer, 0);
    assert!(buffer.iter().all(|&s| s == 0.0));
}

#[test]
fn click_spec_should_leave_out_the_channels_a_device_does_not_have() {
    let spec = ClickSpec {
        shared: click_out(120.0, true, Vec::new()).shared,
        channels: vec![1, 3, 9],
    };
    assert_eq!(spec.out(4).channels, vec![0, 2]);
}
