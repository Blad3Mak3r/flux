use super::factory::{split, tabs};
use super::*;

fn target(node: &str, zone: Zone) -> Target {
    Target {
        node: node.into(),
        zone,
    }
}

fn group(l: &Layout, panel: &str) -> String {
    l.group_of(panel).expect("open")
}

fn monitor_ids(l: &Layout) -> Vec<String> {
    l.monitors.iter().map(|m| m.id.clone()).collect()
}

#[test]
fn factories_validate() {
    for id in FACTORY {
        let l = factory(id).unwrap();
        l.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    assert!(factory("nope").is_none());
    assert_eq!(factory("monitors").unwrap().monitors.len(), 4);
    // The default has every fixed panel once.
    let l = Layout::default();
    for p in FIXED_PANELS {
        assert!(l.group_of(p).is_some(), "{p}");
    }
}

#[test]
fn move_as_tab_and_noop() {
    let mut l = Layout::default();
    let files = group(&l, "files");
    l.move_panel("clip", &target(&files, Zone::Center)).unwrap();
    assert_eq!(group(&l, "clip"), files);
    l.validate().unwrap();
    // Alone on its own group's centre: nothing changes.
    let mut l = Layout::default();
    let mon = monitor_ids(&l)[0].clone();
    let g = group(&l, &mon);
    let before = l.clone();
    l.move_panel(&mon, &target(&g, Zone::Center)).unwrap();
    l.move_panel(&mon, &target(&g, Zone::Left)).unwrap();
    assert_eq!(l, before);
    assert!(l.move_panel("clip", &target(DECK, Zone::Center)).is_err());
    assert!(l.move_panel("clip", &target("zzz", Zone::Left)).is_err());
    assert!(l.move_panel("deck", &target(&g, Zone::Left)).is_err());
}

#[test]
fn move_to_edges_of_group_and_deck() {
    let mut l = Layout::default();
    l.move_panel("mappings", &target(DECK, Zone::Top)).unwrap();
    l.validate().unwrap();
    let Node::Split { children, .. } = &l.root else {
        panic!()
    };
    let Node::Split { children, .. } = &children[0] else {
        panic!()
    };
    // The deck now sits under mappings, in a column of its own.
    let Node::Split { dir, children, .. } = &children[0] else {
        panic!()
    };
    assert_eq!(*dir, Dir::Column);
    assert_eq!(children[1], Node::Deck);
    let mut l = Layout::default();
    let files = group(&l, "files");
    l.move_panel("clip", &target(&files, Zone::Left)).unwrap();
    l.validate().unwrap();
    // Left of the browsers: same row, merged, sizes sum to 1.
    let Node::Split { children, .. } = &l.root else {
        panic!()
    };
    let Node::Split {
        children, sizes, ..
    } = &children[1]
    else {
        panic!()
    };
    assert_eq!(children.len(), 3);
    assert!((sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5);
}

#[test]
fn emptied_group_is_dropped_and_split_collapses() {
    let mut l = factory("monitors").unwrap();
    let ids = monitor_ids(&l);
    for id in &ids[1..] {
        l.close_panel(id).unwrap();
    }
    l.validate().unwrap();
    assert_eq!(l.monitors.len(), 1);
    // Top row collapsed to a single group.
    let Node::Split { children, .. } = &l.root else {
        panic!()
    };
    assert!(matches!(&children[0], Node::Tabs { .. }));
}

#[test]
fn reorder_activate() {
    let mut l = Layout::default();
    let g = group(&l, "clip");
    l.reorder_tab(&g, 0, 2).unwrap();
    let (panels, active) = tabs_mut(&mut l.root, &g).unwrap();
    assert_eq!(panels[2], "clip");
    assert_eq!(*active, 2);
    l.activate(&g, 1).unwrap();
    assert!(l.activate(&g, 99).is_err());
    assert!(l.reorder_tab(&g, 0, 99).is_err());
    assert!(l.activate("zzz", 0).is_err());
}

#[test]
fn close_and_open_back() {
    let mut l = Layout::default();
    let g = group(&l, "layer");
    l.close_panel("layer").unwrap();
    assert!(l.group_of("layer").is_none());
    assert!(l.close_panel("layer").is_err());
    assert!(l.close_panel("deck").is_err());
    l.open_panel("layer", None).unwrap();
    assert_eq!(group(&l, "layer"), g);
    // Opening what is open changes nothing.
    let before = l.clone();
    l.open_panel("layer", None).unwrap();
    assert_eq!(l, before);
    assert!(l.open_panel("bogus", None).is_err());
    l.validate().unwrap();
}

#[test]
fn open_goes_to_largest_group_when_unknown() {
    let mut l = Layout::default();
    l.close_panel("files").unwrap();
    l.last.clear();
    l.open_panel("files", None).unwrap();
    // The biggest group holds clip, layer... and now files.
    assert_eq!(group(&l, "files"), group(&l, "clip"));
}

#[test]
fn open_with_no_group_docks_right_of_deck() {
    let mut l = Layout {
        version: VERSION,
        root: Node::Deck,
        monitors: vec![],
        last: BTreeMap::new(),
    };
    l.open_panel("clip", None).unwrap();
    l.validate().unwrap();
    assert!(matches!(&l.root, Node::Split { dir: Dir::Row, .. }));
}

#[test]
fn monitors_open_close_limit() {
    let mut l = Layout::default();
    let id = l
        .open_panel("monitor", Some(MonitorKind::Composition))
        .unwrap();
    assert!(is_monitor(&id));
    l.validate().unwrap();
    while l.monitors.len() < MAX_MONITORS {
        l.open_panel("monitor", None).unwrap();
    }
    assert!(l.open_panel("monitor", None).is_err());
    l.validate().unwrap();
    l.close_panel(&id).unwrap();
    assert_eq!(l.monitors.len(), MAX_MONITORS - 1);
    assert!(l.monitors.iter().all(|m| m.id != id));
    l.validate().unwrap();
}

#[test]
fn equalize_and_rows() {
    fn sizes_of(n: &Node, id: &str) -> Option<Vec<f32>> {
        match n {
            Node::Split { id: i, sizes, .. } if i == id => Some(sizes.clone()),
            Node::Split { children, .. } => children.iter().find_map(|c| sizes_of(c, id)),
            _ => None,
        }
    }
    let mut store = Store::default();
    store.apply("monitors").unwrap();
    let row_of = store.state().row_of;
    assert_eq!(row_of.len(), 4);
    let row = row_of.values().next().unwrap().clone();
    assert!(row_of.values().all(|r| *r == row));
    store
        .current
        .resize(&row, vec![0.1, 0.2, 0.3, 0.4])
        .unwrap();
    store.current.equalize(&row).unwrap();
    let got = sizes_of(&store.current.root, &row).unwrap();
    assert!(got.iter().all(|s| (s - 0.25).abs() < 1e-6));
    assert!(store.current.equalize("nope").is_err());
}

#[test]
fn set_monitor_kind() {
    let mut l = Layout::default();
    let id = monitor_ids(&l)[0].clone();
    let kind = MonitorKind::Layer {
        layer: Some("a".into()),
    };
    l.set_monitor(&id, kind.clone()).unwrap();
    assert_eq!(l.monitors[0].kind, kind);
    assert!(l.set_monitor("monitor:none", kind).is_err());
}

#[test]
fn screen_monitor_needs_a_screen_id() {
    let mut l = Layout::default();
    let id = monitor_ids(&l)[0].clone();
    let screen = |s: &str| MonitorKind::Screen { screen: s.into() };
    assert!(l.set_monitor(&id, screen("")).is_err());
    l.set_monitor(&id, screen("main")).unwrap();
    l.validate().unwrap();
    assert!(l.open_panel("monitor", Some(screen(""))).is_err());
    assert!(l.open_panel("monitor", Some(screen("s2"))).is_ok());
    let json = serde_json::to_string(&l.monitors[0].kind).unwrap();
    assert_eq!(json, r#"{"type":"screen","screen":"main"}"#);
    l.monitors[0].kind = screen("");
    assert!(l.validate().is_err());
}

#[test]
fn resize_validates_and_renormalises() {
    let mut l = Layout::default();
    let Node::Split { id, .. } = l.root.clone() else {
        panic!()
    };
    l.resize(&id, vec![1.0, 3.0]).unwrap();
    let Node::Split { sizes, .. } = &l.root else {
        panic!()
    };
    assert_eq!(sizes, &vec![0.25, 0.75]);
    assert!(l.resize(&id, vec![1.0]).is_err());
    assert!(l.resize(&id, vec![1.0, 0.0]).is_err());
    assert!(l.resize(&id, vec![1.0, f32::NAN]).is_err());
    assert!(l.resize("zzz", vec![1.0, 1.0]).is_err());
}

#[test]
fn min_sizes() {
    assert_eq!(panel_min_size("mappings"), (300, 200));
    assert_eq!(panel_min_size("monitor:x"), (200, 150));
    assert_eq!(min_size(&Node::Deck), (480, 260));
    assert_eq!(min_size(&tabs(&["mappings", "dashboard"])), (300, 200));
    let row = split(Dir::Row, vec![(Node::Deck, 0.5), (tabs(&["files"]), 0.5)]);
    assert_eq!(min_size(&row), (480 + 4 + 240, 260));
    let col = split(Dir::Column, vec![(row, 0.5), (tabs(&["clip"]), 0.5)]);
    assert_eq!(min_size(&col), (724, 260 + 4 + 180));
}

#[test]
fn state_lists_the_min_of_every_node() {
    let state = Store::default().state();
    assert_eq!(state.gap, SPLITTER_GAP);
    assert_eq!(state.mins["deck"], DECK_MIN_SIZE);
    let Node::Split { id, .. } = &state.current.root else {
        panic!("the default layout starts with a split");
    };
    assert_eq!(state.mins[id], min_size(&state.current.root));
}

#[test]
fn invalid_or_newer_falls_back() {
    let default = Layout::default().root;
    let same = |l: Layout| {
        matches!(
            (&l.root, &default),
            (Node::Split { .. }, Node::Split { .. })
        )
    };
    assert!(same(Layout::parse("not json")));
    let mut newer = factory("monitors").unwrap();
    newer.version = VERSION + 1;
    assert_eq!(
        Layout::parse(&serde_json::to_string(&newer).unwrap())
            .monitors
            .len(),
        2
    );
    let mut twice = factory("monitors").unwrap();
    twice.monitors.pop();
    assert_eq!(
        Layout::parse(&serde_json::to_string(&twice).unwrap())
            .monitors
            .len(),
        2
    );
    let ok = factory("performance").unwrap();
    assert_eq!(Layout::parse(&serde_json::to_string(&ok).unwrap()), ok);
    // Two decks are invalid.
    let mut two = Layout::default();
    two.root = split(Dir::Row, vec![(Node::Deck, 1.0), (two.root.clone(), 1.0)]);
    assert!(two.validate().is_err());
}

#[test]
fn store_saves_applies_renames_deletes() {
    let mut s = Store::default();
    s.current.close_panel("clip").unwrap();
    s.save_as(" mine ").unwrap();
    s.save_as("mine").unwrap();
    assert_eq!(s.saved.len(), 1);
    assert!(s.save_as("  ").is_err());
    let id = s.saved[0].id.clone();
    s.apply("default").unwrap();
    assert!(s.current.group_of("clip").is_some());
    s.apply(&id).unwrap();
    assert!(s.current.group_of("clip").is_none());
    s.save_as("other").unwrap();
    let other = s.saved[1].id.clone();
    assert!(s.rename(&other, "mine").is_err());
    s.rename(&other, "second").unwrap();
    assert!(s.apply("zzz").is_err());
    s.delete(&other).unwrap();
    assert!(s.delete(&other).is_err());
    s.reset();
    assert!(s.current.group_of("clip").is_some());
    let state = s.state();
    assert_eq!(state.factory.len(), 3);
    assert_eq!(state.saved.len(), 1);
}

#[test]
fn store_file_round_trip_and_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vj-layout.json");
    assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
    let mut s = Store::default();
    s.save_as("x").unwrap();
    s.save(&path).unwrap();
    assert_eq!(Store::load(&path).unwrap(), s);
    std::fs::write(&path, "{ broken").unwrap();
    assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
    let mut newer = s.clone();
    newer.version = VERSION + 1;
    std::fs::write(&path, serde_json::to_string(&newer).unwrap()).unwrap();
    assert_eq!(Store::load(&path).unwrap().saved.len(), 0);
}

#[test]
fn the_clip_size_is_kept_with_the_store_and_survives_applying_a_layout() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vj-layout.json");
    let mut s = Store::default();
    assert_eq!(s.state().clip_size, ClipSize::Medium);
    s.clip_size = ClipSize::Large;
    s.save(&path).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["clipSize"], "large");
    let mut back = Store::load(&path).unwrap();
    assert_eq!(back.state().clip_size, ClipSize::Large);
    back.apply("performance").unwrap();
    assert_eq!(back.state().clip_size, ClipSize::Large);
    // The default is not written, and an older file has none.
    back.clip_size = ClipSize::Medium;
    assert!(
        serde_json::to_value(&back)
            .unwrap()
            .get("clipSize")
            .is_none()
    );
    assert_eq!(
        Store::parse(&serde_json::to_string(&back).unwrap()).clip_size,
        ClipSize::Medium
    );
}
