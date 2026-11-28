use super::*;
#[test]
fn project_destinations_resolve_locally_and_preview_preserves_silent_tracks() {
    use flux_project::{
        Track, TrackKind,
        routing::{AudioAssignment, AudioBus},
    };
    let mut project = flux_project::Project::new("Show", flux_project::FrameRate::Fps25);
    let id = "5fda6c22-87d3-40e9-9710-589f8c1ef8f0".to_owned();
    project.output.audio.push(AudioBus {
        id: id.clone(),
        name: "Monitor".into(),
    });
    for (name, output) in [
        ("Inherit", AudioAssignment::Inherit),
        ("Custom", AudioAssignment::Output { id: id.clone() }),
        ("Silent", AudioAssignment::Silent),
    ] {
        project.timelines[0].tracks.push(Track {
            id: name.into(),
            name: name.into(),
            muted: false,
            color: None,
            gain_db: None,
            audio_output: output,
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![],
            },
        });
    }
    let route = |name: &str| AudioOutput {
        device_name: name.into(),
        channels: vec![1, 2],
    };
    let mut prefs = Preferences {
        operator: Some(route("Headphones")),
        preview: Some(route("Preview")),
        ..Default::default()
    };
    let live = prefs.resolve_project(&project, &project.timelines[0], false);
    assert!(
        live.audio_routes
            .iter()
            .find(|b| b.id == id)
            .unwrap()
            .route
            .is_none()
    );
    prefs
        .audio_routes
        .insert(id.clone(), route("Monitor device"));
    let live = prefs.resolve_project(&project, &project.timelines[0], false);
    assert_eq!(
        live.audio_routes[2].route.as_ref().unwrap().device_name,
        "Monitor device"
    );
    let edit = prefs.resolve_project(&project, &project.timelines[0], true);
    assert!(
        edit.audio_routes
            .iter()
            .all(|b| b.route.as_ref().unwrap().device_name == "Preview")
    );
    assert_eq!(
        edit.track_routes
            .iter()
            .find(|r| r.track_id == "Silent")
            .unwrap()
            .output,
        None
    );
    assert_eq!(
        edit.track_routes
            .iter()
            .find(|r| r.track_id == "Inherit")
            .unwrap()
            .output
            .as_deref(),
        Some("operator")
    );
    let json = serde_json::to_string(&project).unwrap();
    assert!(!json.contains("Monitor device") && !json.contains("Headphones"));
}
#[test]
fn named_hardware_routes_survive_preferences_roundtrip_and_driver_switch_clears_them() {
    let id = "5fda6c22-87d3-40e9-9710-589f8c1ef8f0".to_owned();
    let mut prefs = Preferences::default();
    prefs.audio_routes.insert(
        id.clone(),
        AudioOutput {
            device_name: "Monitor".into(),
            channels: vec![5],
        },
    );
    prefs
        .apply(Change::AudioRoute {
            id: id.clone(),
            field: "channels".into(),
            text: "7".into(),
        })
        .unwrap();
    let loaded: Preferences =
        serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
    assert_eq!(loaded.audio_routes[&id].channels, vec![7]);
    prefs
        .apply(Change::Route {
            kind: "driver".into(),
            field: "driver".into(),
            text: "Other".into(),
            slot: 0,
        })
        .unwrap();
    assert!(prefs.audio_routes.is_empty());
}
#[test]
fn locale_changes_do_not_count_as_hardware_changes() {
    let before = Preferences::default();
    let mut after = before.clone();
    after.locale = Some(Locale::Es);
    assert!(before.same_routing(&after));
    after.audio = Some(AudioOutput {
        device_name: "Main".into(),
        channels: vec![1, 2],
    });
    assert!(!before.same_routing(&after));
}

#[test]
fn saving_replaces_previous_preferences_and_failure_keeps_old_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    let mut p = Preferences {
        locale: Some(Locale::Es),
        ..Default::default()
    };
    save(&path, &p).unwrap();
    p.locale = Some(Locale::En);
    save(&path, &p).unwrap();
    assert!(matches!(load(&path).locale, Some(Locale::En)));
    let before = fs::read(&path).unwrap();
    fs::create_dir(path.with_extension("json.tmp")).unwrap();
    assert!(save(&path, &p).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn corrupt_preferences_have_a_visible_warning_and_safe_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("preferences.json");
    fs::write(&path, "broken json").unwrap();
    let p = load(&path);
    assert!(p.warning.is_some());
    assert!(p.ltc.is_empty() && p.mtc.is_empty());
}

#[test]
fn two_shows_use_shared_routes_and_independent_levels() {
    let mut p = Preferences::default();
    p.ltc.insert(
        2,
        LtcOutput {
            device_name: "Sync".into(),
            channel: 8,
            level: None,
            offset_ms: Some(15),
        },
    );
    let show = |level| ShowOutputs {
        audio: Vec::new(),
        ltc: vec![flux_project::LtcSignal {
            slot: 2,
            level: Some(level),
        }],
        ..Default::default()
    };
    let first = p.resolve(&show(0.2), false);
    let second = p.resolve(&show(0.7), false);
    assert_eq!(first.ltc[0].device_name, second.ltc[0].device_name);
    assert_eq!(
        (first.ltc[0].level, second.ltc[0].level),
        (Some(0.2), Some(0.7))
    );
    assert_eq!(p.ltc[&2].offset_ms, Some(15));
}

#[test]
fn import_preserves_unmentioned_routes_and_language() {
    let mut p = Preferences {
        locale: Some(Locale::Es),
        ..Default::default()
    };
    p.mtc.insert(
        2,
        MtcOutput {
            port_name: "Keep".into(),
            port_index: 1,
            offset_ms: Some(-10),
        },
    );
    let imported = OutputConfig {
        audio_routes: Vec::new(),
        track_routes: Vec::new(),
        mtc: vec![MtcOutput {
            port_name: "New".into(),
            port_index: 2,
            offset_ms: Some(12),
        }],
        ..Default::default()
    };
    p.apply(Change::Import {
        routes: Box::new(imported),
    })
    .unwrap();
    assert_eq!(p.mtc[&1].port_index, 2);
    assert_eq!(p.mtc[&2].port_name, "Keep");
    assert!(matches!(p.locale, Some(Locale::Es)));
}

#[test]
fn route_edits_parse_and_validate_channel_and_offset_text() {
    let mut p = Preferences::default();
    p.mtc.insert(
        1,
        MtcOutput {
            port_name: "Bus".into(),
            port_index: 1,
            offset_ms: None,
        },
    );
    for text in ["abc", "1.5", "1001", "-1001", ""] {
        assert!(
            p.apply(Change::Offset {
                kind: "mtc".into(),
                slot: 1,
                text: text.into()
            })
            .is_err()
        );
    }
    p.apply(Change::Offset {
        kind: "mtc".into(),
        slot: 1,
        text: " -12 ".into(),
    })
    .unwrap();
    assert_eq!(p.mtc[&1].offset_ms, Some(-12));
    p.apply(Change::Route {
        kind: "mtc".into(),
        field: "port".into(),
        text: "2|Other".into(),
        slot: 1,
    })
    .unwrap();
    assert_eq!((p.mtc[&1].port_index, p.mtc[&1].offset_ms), (2, Some(-12)));
    p.audio = Some(AudioOutput {
        device_name: "Main".into(),
        channels: vec![1, 2],
    });
    assert!(p.edit_route("audio", "channels", "1-1", 0).is_err());
}

#[test]
fn midi_clock_should_go_out_only_when_the_show_and_this_computer_both_ask() {
    let mut p = Preferences::default();
    let mut show = ShowOutputs::default();
    assert!(p.resolve(&show, false).midi_clock.is_none());
    p.apply(Change::MidiClock {
        port: Some("Drum machine".into()),
        index: 1,
    })
    .unwrap();
    assert!(
        p.resolve(&show, false).midi_clock.is_none(),
        "the show is off"
    );
    show.midi_clock = true;
    let clock = p.resolve(&show, false).midi_clock.unwrap();
    assert_eq!(
        (clock.port_name.as_str(), clock.port_index),
        ("Drum machine", 1)
    );
    assert!(!clock.free_run, "a cue show unless the project says VJ");
    assert!(
        p.resolve(&show, true).midi_clock.is_none(),
        "no clock while auditioning"
    );
    p.apply(Change::MidiClock {
        port: None,
        index: 0,
    })
    .unwrap();
    assert!(p.resolve(&show, false).midi_clock.is_none());
}

#[test]
fn a_vj_show_should_run_its_midi_clock_free() {
    let mut p = Preferences::default();
    p.apply(Change::MidiClock {
        port: Some("Port".into()),
        index: 0,
    })
    .unwrap();
    let mut project = flux_project::Project::new("Show", flux_project::FrameRate::Fps25);
    project.output.midi_clock = true;
    let timeline = project.timelines[0].clone();
    let cue = p.resolve_project(&project, &timeline, false);
    assert!(!cue.midi_clock.unwrap().free_run);
    project.mode = flux_project::ProjectMode::Vj;
    let vj = p.resolve_project(&project, &timeline, false);
    assert!(vj.midi_clock.unwrap().free_run);
}

#[test]
fn local_routes_resolve_without_importing_legacy_hints() {
    let legacy = OutputConfig {
        audio_routes: Vec::new(),
        track_routes: Vec::new(),
        ltc: vec![LtcOutput {
            device_name: "Old".into(),
            channel: 3,
            level: Some(0.2),
            offset_ms: Some(20),
        }],
        ..Default::default()
    };
    let show = ShowOutputs::from(legacy.clone());
    let mut p = Preferences::default();
    assert_eq!(p.resolve(&show, false).ltc[0].device_name, UNASSIGNED);
    p.apply(Change::Import {
        routes: Box::new(legacy),
    })
    .unwrap();
    let resolved = p.resolve(&show, false);
    assert_eq!(
        (
            resolved.ltc[0].channel,
            resolved.ltc[0].level,
            resolved.ltc[0].offset_ms
        ),
        (3, Some(0.2), Some(20))
    );
    assert!(p.resolve(&show, true).ltc.is_empty());
}
#[test]
fn offsets_and_locale_migration_are_checked() {
    let mut p = Preferences::default();
    p.apply(Change::Locale {
        locale: Locale::Es,
        only_if_unset: true,
    })
    .unwrap();
    p.apply(Change::Locale {
        locale: Locale::En,
        only_if_unset: true,
    })
    .unwrap();
    assert!(matches!(p.locale, Some(Locale::Es)));
    assert!(
        p.apply(Change::Offset {
            kind: "ltc".into(),
            slot: 1,
            text: "1001".into()
        })
        .is_err()
    );
}

#[test]
fn the_analysis_source_should_default_to_the_grid_and_round_trip() {
    assert_eq!(AnalysisSource::default(), AnalysisSource::Grid);
    assert!(Preferences::default().analysis.is_none());
    let input = AnalysisSource::Input {
        driver: None,
        device: "Interface".into(),
        channel: 2,
    };
    let json = serde_json::to_value(&input).unwrap();
    assert_eq!(json["kind"], "input");
    assert_eq!(
        serde_json::from_value::<AnalysisSource>(json).unwrap(),
        input
    );
    let change: Change =
        serde_json::from_str(r#"{"op":"analysis","source":{"kind":"off"}}"#).unwrap();
    let mut prefs = Preferences::default();
    prefs.apply(change).unwrap();
    assert_eq!(prefs.analysis, Some(AnalysisSource::Off));
    prefs.analysis = Some(AnalysisSource::Input {
        driver: None,
        device: "x".into(),
        channel: 0,
    });
    assert!(validate(&prefs).is_err());
}

#[test]
fn shortcuts_round_trip_and_refuse_a_key_in_use() {
    let mut prefs = Preferences::default();
    let change = |json: &str| serde_json::from_str::<Change>(json).unwrap();
    prefs
        .apply(change(
            r#"{"op":"shortcut","action":"addMarker","key":"k"}"#,
        ))
        .unwrap();
    let error = prefs
        .apply(change(
            r#"{"op":"shortcut","action":"addRegion","key":"K"}"#,
        ))
        .unwrap_err();
    assert_eq!(error, "conflict:addMarker");
    let dir = std::env::temp_dir().join(format!("flux-keys-{}", std::process::id()));
    let path = dir.join("preferences.json");
    save(&path, &prefs.clone().sent()).unwrap();
    let loaded = load(&path);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(loaded.warning, None);
    assert_eq!(loaded.shortcuts, prefs.shortcuts);
    let keys = |p: Preferences| {
        p.sent()
            .keymap
            .into_iter()
            .find(|a| a.id == "addMarker")
            .unwrap()
            .keys
    };
    assert_eq!(keys(loaded), ["K"]);
    prefs
        .apply(change(r#"{"op":"shortcutReset","action":null}"#))
        .unwrap();
    assert_eq!(keys(prefs), ["M"]);
}

#[test]
fn every_action_the_window_uses_is_in_the_table() {
    let ts = include_str!("../../../src/keymap/actions.ts");
    let used: std::collections::BTreeSet<&str> = ts.split('"').skip(1).step_by(2).collect();
    let table: std::collections::BTreeSet<&str> =
        flux_project::keymap::ACTIONS.iter().map(|a| a.id).collect();
    assert_eq!(used, table);
}
