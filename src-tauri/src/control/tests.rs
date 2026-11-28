use super::*;
use flux_engine::control::TriggerKind;
use tempfile::TempDir;

fn binding(number: u8, action: ControlAction) -> Binding {
    Binding {
        trigger: MidiTrigger {
            kind: TriggerKind::Note,
            channel: 1,
            number,
        },
        action,
        feedback: None,
    }
}

#[test]
fn the_defaults_should_listen_to_nothing() {
    let prefs = ControlPrefs::default();
    assert!(!prefs.midi.enabled);
    assert!(!prefs.osc.enabled);
    assert!(!prefs.midi.feedback, "the lights are off until asked for");
    assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
    assert!(prefs.midi.bindings.is_empty());
}

#[test]
fn normalize_should_drop_what_cannot_work() {
    let mut prefs = ControlPrefs::default();
    prefs.midi.port_name = Some("  ".into());
    prefs.osc.port = 0;
    prefs.midi.bindings = vec![
        binding(36, ControlAction::Play),
        binding(36, ControlAction::Stop),
        binding(37, ControlAction::GotoCue(0)),
        binding(38, ControlAction::StepFrames(0)),
        binding(39, ControlAction::GotoCue(2)),
        binding(200, ControlAction::Stop),
    ];
    prefs.midi.bindings[5].trigger.number = 200;
    prefs.midi.bindings.push(Binding {
        trigger: MidiTrigger {
            kind: TriggerKind::Cc,
            channel: 17,
            number: 1,
        },
        action: ControlAction::Stop,
        feedback: None,
    });
    prefs.midi.bindings[4].feedback = Some(Feedback { on: 200, off: 0 });
    prefs.midi.bindings[0].feedback = Some(Feedback { on: 5, off: 1 });

    let prefs = normalize(prefs);

    assert_eq!(prefs.midi.port_name, None);
    assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
    assert_eq!(
        prefs.midi.bindings,
        vec![
            Binding {
                feedback: Some(Feedback { on: 5, off: 1 }),
                ..binding(36, ControlAction::Play)
            },
            binding(39, ControlAction::GotoCue(2)),
        ],
        "a value that is not a data byte is dropped, the others are kept"
    );
}

#[test]
fn preferences_should_survive_a_save_and_a_load() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("flux").join(FILE_NAME);
    let mut prefs = ControlPrefs::default();
    prefs.midi.enabled = true;
    prefs.midi.port_name = Some("APC40 mkII".into());
    prefs.midi.port_index = 1;
    prefs.midi.bindings = vec![binding(0, ControlAction::GotoCue(1))];
    prefs.osc = OscPrefs {
        enabled: true,
        port: 9100,
        ..OscPrefs::default()
    };

    save(&file, &prefs).unwrap();

    assert_eq!(load(&file), prefs);
    assert!(!file.with_extension("json.tmp").exists());
}

#[test]
fn art_net_should_be_off_by_default_hear_every_universe_and_survive_a_save() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join(FILE_NAME);
    assert!(!ControlPrefs::default().artnet.enabled);
    assert_eq!(ControlPrefs::default().artnet.universe, None);

    let prefs = ControlPrefs {
        artnet: ArtnetPrefs {
            enabled: true,
            universe: Some(3),
            bind_address: "192.168.1.5".into(),
        },
        ..ControlPrefs::default()
    };
    save(&file, &prefs).unwrap();

    assert_eq!(load(&file), prefs);
}

#[test]
fn normalize_should_drop_a_universe_that_is_not_a_port_address_and_an_empty_interface() {
    let mut prefs = ControlPrefs::default();
    prefs.artnet.universe = Some(40_000);
    prefs.artnet.bind_address = "  ".into();

    let prefs = normalize(prefs);

    assert_eq!(prefs.artnet.universe, None);
    assert_eq!(prefs.artnet.bind_address, "0.0.0.0");
}

#[test]
fn a_file_from_before_art_net_should_load_with_it_off() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join(FILE_NAME);
    fs::write(
        &file,
        r#"{"midi":{"enabled":false},"osc":{"enabled":true,"port":9100}}"#,
    )
    .unwrap();

    let prefs = load(&file);

    assert_eq!(prefs.osc.port, 9100);
    assert_eq!(prefs.artnet, ArtnetPrefs::default());
}

#[test]
fn a_missing_or_broken_file_should_give_the_defaults() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join(FILE_NAME);
    assert_eq!(load(&file), ControlPrefs::default());
    fs::write(&file, "{ not json").unwrap();
    assert_eq!(load(&file), ControlPrefs::default());
}

#[test]
fn a_file_with_only_some_fields_should_fill_in_the_rest() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join(FILE_NAME);
    fs::write(&file, r#"{"osc":{"enabled":true}}"#).unwrap();

    let prefs = load(&file);

    assert!(prefs.osc.enabled);
    assert_eq!(prefs.osc.port, DEFAULT_OSC_PORT);
    assert!(!prefs.midi.enabled);
}

#[test]
fn a_link_state_should_serialize_for_the_window() {
    let json = |state: LinkState| serde_json::to_string(&state).unwrap();
    assert_eq!(json(LinkState::Off), r#"{"state":"off"}"#);
    assert_eq!(json(LinkState::Missing), r#"{"state":"missing"}"#);
    assert_eq!(
        json(LinkState::Listening("9000".into())),
        r#"{"state":"listening","detail":"9000"}"#
    );
    assert_eq!(
        json(LinkState::Error("in use".into())),
        r#"{"state":"error","detail":"in use"}"#
    );
}
