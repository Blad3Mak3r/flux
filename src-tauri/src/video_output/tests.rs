use super::menu::display_at;
use super::*;

#[test]
fn a_picture_is_shared_only_while_young_and_of_the_same_revision() {
    assert!(picture_is_fresh(FRESH / 2, 3, 3));
    assert!(!picture_is_fresh(FRESH * 2, 3, 3));
    assert!(!picture_is_fresh(FRESH / 2, 3, 4));
}

fn display(id: &str) -> Display {
    Display {
        id: id.into(),
        width: 1920,
        height: 1080,
        main: false,
    }
}

#[test]
fn display_at_should_count_from_one_in_the_order_of_the_system() {
    let list = [display("a"), display("b")];
    assert_eq!(display_at(&list, 1).map(|d| d.id.as_str()), Some("a"));
    assert_eq!(display_at(&list, 2).map(|d| d.id.as_str()), Some("b"));
    assert!(display_at(&list, 0).is_none());
    assert!(display_at(&list, 3).is_none());
}

#[test]
fn controls_should_start_off_and_flip_one_flag_at_a_time() {
    let controls = Controls::default();
    assert!(!controls.disabled.load(Ordering::Relaxed));
    assert!(!controls.card.load(Ordering::Relaxed));
    assert!(!controls.disabled.fetch_xor(true, Ordering::Relaxed));
    assert!(controls.disabled.load(Ordering::Relaxed));
    assert!(!controls.card.load(Ordering::Relaxed));
}

#[test]
fn card_caption_should_name_the_screen_and_the_window_size() {
    assert_eq!(card_caption("Main", 1920, 1080), "Main  1920 x 1080");
}
use flux_project::playhead::Tick;
use flux_project::vj::{Blend, Clip, Layer, Playback, Side, Trigger, Vj};
use flux_project::{VideoFit, vj::Direction};
use flux_video::{GridPlay, Rate, Shown};

fn clip(id: &str) -> Clip {
    Clip {
        id: id.into(),
        column: Some("c1".into()),
        slot: None,
        asset: "v1".into(),
        generator: None,
        name: None,
        playback: Playback::Loop,
        trigger: Trigger::Normal,
        speed: 1.0,
        duration: flux_project::vj::DEFAULT_DURATION,
        direction: Direction::default(),
        tempo: Default::default(),
        region: Default::default(),
        cues: Default::default(),
        fit: VideoFit::Fill,
        scale: 0.5,
        position: [0.1, 0.0],
        effects: Vec::new(),
        modulation: Default::default(),
        dashboard: Vec::new(),
    }
}

fn layer(id: &str, clips: Vec<Clip>) -> Layer {
    Layer {
        id: id.into(),
        name: None,
        global: false,
        master: 0.4,
        opacity: 0.3,
        volume: 1.0,
        blend: Blend::Add,
        transition: 2.0,
        transition_effect: Default::default(),
        transition_random: false,
        autopilot: Default::default(),
        crossfade: Some(Side::A),
        bypass: true,
        solo: false,
        collapsed: false,
        group: None,
        effects: Vec::new(),
        modulation: Default::default(),
        dashboard: Vec::new(),
        clips,
    }
}

fn grid() -> Vj {
    Vj {
        layers: vec![
            layer("l1", vec![clip("a"), clip("b")]),
            layer("l2", vec![clip("c")]),
        ],
        ..Default::default()
    }
}

fn shown(vj: &Vj, play: &GridPlay, tick: Tick) -> Vec<Shown> {
    let probe = |_: &str| Some((Rate::new(25, 1), 100));
    play.layers(vj, tick, &probe)
        .into_iter()
        .map(|layer| Shown {
            frame: Some(Arc::new(flux_video::Frame {
                index: layer.frame,
                width: 1920,
                height: 800,
                rgba: Vec::new(),
                alpha: false,
            })),
            layer,
        })
        .collect()
}

#[test]
fn preview_grid_should_keep_only_the_clip_at_full_strength() {
    let vj = preview_grid(&grid(), "a").unwrap();
    assert_eq!(vj.layers.len(), 1);
    assert_eq!(vj.layers[0].clips.len(), 1);
    assert!(!vj.layers[0].bypass && vj.layers[0].crossfade.is_none());
    assert!(preview_grid(&grid(), "nope").is_none());
}

#[test]
fn preview_should_place_a_clip_as_the_output_does() {
    let size = (1920, 1080);
    let tick = Tick::default();
    let mut on_air = grid();
    on_air.layers[0] = Layer {
        bypass: false,
        crossfade: None,
        opacity: 1.0,
        master: 1.0,
        transition: 0.0,
        blend: Blend::Alpha,
        ..on_air.layers[0].clone()
    };
    let mut play = GridPlay::default();
    play.launch(&on_air, "a", tick);
    let output = shown(&on_air, &play, tick);

    let cut = preview_grid(&grid(), "a").unwrap();
    let mut preview_play = GridPlay::default();
    preview_play.launch(&cut, "a", tick);
    let preview = shown(&cut, &preview_play, tick);

    assert_eq!(preview.len(), 1, "a bypassed layer still previews");
    let (p, o) = (
        &pictures(&preview, size, 0.0)[0],
        &pictures(&output, size, 0.0)[0],
    );
    assert_eq!(p.place, o.place);
    assert_eq!(p.opacity, 1.0);
}

#[test]
fn preview_should_start_at_the_clip_start_and_again_when_launched_again() {
    let cut = preview_grid(&grid(), "a").unwrap();
    let mut play = GridPlay::default();
    let later = Tick {
        time: Duration::from_secs(1),
        beats: 2.0,
    };
    play.launch(&cut, "a", Tick::default());
    assert!(shown(&cut, &play, later)[0].layer.frame > 0, "it plays");
    play.launch(&cut, "a", later);
    assert_eq!(shown(&cut, &play, later)[0].layer.frame, 0, "it restarts");
}
