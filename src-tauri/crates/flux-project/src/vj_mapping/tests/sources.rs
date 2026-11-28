use super::*;

#[test]
fn a_dmx_and_a_key_source_should_survive_a_round_trip_and_stay_short_in_the_file() {
    let sources = [
        artnet(1, 37, None),
        artnet(0x0231, 5, Some(6)),
        key("KeyQ", false),
        key("KeyQ", true),
    ];
    for source in sources {
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(serde_json::from_str::<Source>(&json).unwrap(), source);
    }
    assert_eq!(
        serde_json::to_string(&artnet(1, 37, None)).unwrap(),
        r#"{"type":"artnet","universe":1,"channel":37}"#
    );
    assert_eq!(
        serde_json::to_string(&key("KeyQ", true)).unwrap(),
        r#"{"type":"key","code":"KeyQ","shift":true}"#
    );
}

#[test]
fn a_file_from_before_dmx_keys_and_follow_modes_should_still_load() {
    let old = r#"{"id":"m","source":{"type":"osc","address":"/a"},"target":{"kind":"stopAll"},"min":0.0,"max":1.0}"#;
    let mapping: Mapping = serde_json::from_str(old).unwrap();

    assert_eq!(mapping.follow, Follow::Id);
    assert!(mapping.place.is_none() && !mapping.parked);
    assert!(!serde_json::to_string(&mapping).unwrap().contains("follow"));
}

#[test]
fn dmx_and_key_sources_should_be_checked() {
    assert!(artnet(0, 1, None).validate().is_ok());
    assert!(artnet(MAX_UNIVERSE, 512, Some(511)).validate().is_ok());
    assert!(artnet(MAX_UNIVERSE + 1, 1, None).validate().is_err());
    assert!(artnet(0, 0, None).validate().is_err());
    assert!(artnet(0, 513, None).validate().is_err());
    assert!(
        artnet(0, 5, Some(5)).validate().is_err(),
        "fine is another channel"
    );
    assert!(key("KeyQ", false).validate().is_ok());
    assert!(key("", false).validate().is_err());
    assert!(key("Key Q!", false).validate().is_err());
}

#[test]
fn a_dmx_or_key_source_should_only_match_exactly_itself() {
    assert!(artnet(1, 37, None).matches(&artnet(1, 37, None)));
    assert!(!artnet(1, 37, None).matches(&artnet(2, 37, None)));
    assert!(!artnet(1, 37, Some(38)).matches(&artnet(1, 37, None)));
    assert!(key("KeyQ", false).matches(&key("KeyQ", false)));
    assert!(
        !key("KeyQ", false).matches(&key("KeyQ", true)),
        "Shift+Q is another key"
    );
    assert!(!key("KeyQ", false).matches(&cc(1)));
}

#[test]
fn the_keys_the_app_uses_should_be_refused_and_the_others_taken() {
    let combo = |code: &str, ctrl: bool, shift: bool| Source::Key {
        code: code.into(),
        ctrl,
        alt: false,
        shift,
        meta: false,
    };
    for used in [
        combo("KeyS", true, false),
        combo("KeyS", true, true),
        combo("KeyM", true, true),
        combo("KeyK", true, true),
        combo("KeyY", true, false),
        combo("KeyL", true, false),
        combo("KeyD", true, true),
        combo("KeyL", true, true),
        combo("KeyP", true, true),
        combo("Digit1", true, true),
        combo("Digit9", true, true),
        key("Escape", false),
        key("ShiftLeft", false),
    ] {
        assert!(used.is_app_shortcut(), "{used:?}");
    }
    // Resolume leaves these free by default, so a user can map them.
    for free in [
        combo("KeyM", true, false),
        combo("KeyZ", true, true),
        combo("Digit1", true, false),
        combo("KeyQ", true, false),
        combo("KeyK", false, true),
        key("Space", false),
        key("Enter", false),
        key("Delete", false),
        key("Tab", false),
        key("ArrowLeft", false),
        key("PageUp", false),
        key("KeyB", false),
        key("KeyS", true),
        key("KeyX", false),
        key("Digit1", false),
        key("Digit9", false),
        key("Digit1", true),
        key("Numpad3", false),
        key("KeyQ", false),
        key("F5", false),
        key("Backquote", false),
    ] {
        assert!(!free.is_app_shortcut(), "{free:?}");
    }
    assert!(!cc(1).is_app_shortcut());
}

#[test]
fn learning_a_key_the_app_uses_should_be_refused_by_the_backend() {
    let mut project = show();
    let target = Target::LayerBypass {
        layer: layer(&project, 0).id.clone(),
    };

    let refused = apply(
        &mut project,
        VjOp::AddMapping {
            source: key("Escape", false),
            target: target.clone(),
        },
    );
    let taken = apply(
        &mut project,
        VjOp::AddMapping {
            source: key("Space", false),
            target,
        },
    );

    assert!(refused.is_err());
    assert!(taken.is_ok());
}

#[test]
fn a_key_should_press_and_release_like_a_pad() {
    let mut project = show();
    let clip = with_generator(&mut project);
    apply(
        &mut project,
        VjOp::AddMapping {
            source: key("KeyQ", false),
            target: Target::ClipLaunch { clip: clip.clone() },
        },
    )
    .unwrap();
    apply(
        &mut project,
        VjOp::AddMapping {
            source: key("KeyW", false),
            target: Target::Crossfader,
        },
    )
    .unwrap();
    let mut held = HashSet::new();

    let down = react(
        &project.vj,
        &heard(key("KeyQ", false), 1.0, true),
        &mut held,
    );
    let up = react(
        &project.vj,
        &heard(key("KeyQ", false), 0.0, false),
        &mut held,
    );
    let level_down = react(
        &project.vj,
        &heard(key("KeyW", false), 1.0, true),
        &mut held,
    );
    let level_up = react(
        &project.vj,
        &heard(key("KeyW", false), 0.0, false),
        &mut held,
    );

    assert_eq!(
        down,
        vec![(Target::ClipLaunch { clip: clip.clone() }, Command::Press)]
    );
    assert_eq!(up, vec![(Target::ClipLaunch { clip }, Command::Release)]);
    assert_eq!(
        level_down[0].1,
        Command::Value(1.0),
        "a key at a level goes to the maximum"
    );
    assert!(
        level_up.is_empty(),
        "and stays there unless the mode is hold"
    );
}

#[test]
fn a_held_key_should_press_once_whatever_the_repeats() {
    let mut project = show();
    let clip = with_generator(&mut project);
    apply(
        &mut project,
        VjOp::AddMapping {
            source: key("KeyQ", false),
            target: Target::ClipLaunch { clip },
        },
    )
    .unwrap();
    let mut held = HashSet::new();
    let down = heard(key("KeyQ", false), 1.0, true);

    assert_eq!(react(&project.vj, &down, &mut held).len(), 1);
    assert!(react(&project.vj, &down, &mut held).is_empty());
}

#[test]
fn a_dmx_button_should_toggle_a_switch_on_each_press() {
    let mut project = show();
    let target = Target::LayerBypass {
        layer: layer(&project, 0).id.clone(),
    };
    apply(
        &mut project,
        VjOp::AddMapping {
            source: artnet(1, 10, None),
            target: target.clone(),
        },
    )
    .unwrap();
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.0,
            max: 1.0,
            invert: false,
            mode: ButtonMode::Toggle,
        },
    )
    .unwrap();
    let mut held = HashSet::new();
    let at = |level: f32| heard(artnet(1, 10, None), level, level >= 128.0 / 255.0);

    assert_eq!(
        react(&project.vj, &at(1.0), &mut held),
        vec![(target, Command::Toggle)]
    );
    assert!(
        react(&project.vj, &at(0.9), &mut held).is_empty(),
        "still down"
    );
    assert!(
        react(&project.vj, &at(0.0), &mut held).is_empty(),
        "a release flips nothing"
    );
}
