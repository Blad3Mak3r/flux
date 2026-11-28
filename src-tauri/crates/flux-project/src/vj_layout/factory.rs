use super::*;

struct Build {
    monitors: Vec<Monitor>,
}

impl Build {
    fn monitor(&mut self, kind: MonitorKind) -> Node {
        let id = format!("{MONITOR_PREFIX}{}", new_id());
        self.monitors.push(Monitor {
            id: id.clone(),
            kind,
        });
        tabs(&[&id])
    }
}

pub(super) fn tabs(panels: &[&str]) -> Node {
    Node::Tabs {
        id: new_id(),
        panels: panels.iter().map(|p| (*p).to_owned()).collect(),
        active: 0,
    }
}

pub(super) fn split(dir: Dir, parts: Vec<(Node, f32)>) -> Node {
    let (children, sizes) = parts.into_iter().unzip();
    Node::Split {
        id: new_id(),
        dir,
        children,
        sizes,
    }
}

/// A built-in layout by id (`default`, `performance`, `monitors`), with fresh ids.
pub fn factory(id: &str) -> Option<Layout> {
    use Dir::{Column, Row};
    let mut b = Build { monitors: vec![] };
    let browsers = || tabs(&["files", "generators", "effects"]);
    let root = match id {
        "default" => {
            let monitors = split(
                Column,
                vec![
                    (b.monitor(MonitorKind::Composition), 0.5),
                    (b.monitor(MonitorKind::Preview), 0.5),
                ],
            );
            split(
                Column,
                vec![
                    (split(Row, vec![(Node::Deck, 0.75), (monitors, 0.25)]), 0.6),
                    (
                        split(
                            Row,
                            vec![
                                (
                                    tabs(&[
                                        "clip",
                                        "layer",
                                        "group",
                                        "composition",
                                        "dashboard",
                                        "mappings",
                                    ]),
                                    0.65,
                                ),
                                (browsers(), 0.35),
                            ],
                        ),
                        0.4,
                    ),
                ],
            )
        }
        "performance" => {
            let top = split(
                Row,
                vec![
                    (b.monitor(MonitorKind::Composition), 1.0),
                    (b.monitor(MonitorKind::Layer { layer: None }), 1.0),
                    (b.monitor(MonitorKind::Preview), 1.0),
                ],
            );
            let bottom = split(
                Row,
                vec![
                    (tabs(&["composition", "group"]), 0.25),
                    (tabs(&["layer"]), 0.2),
                    (tabs(&["clip"]), 0.25),
                    (browsers(), 0.3),
                ],
            );
            split(Column, vec![(top, 0.28), (Node::Deck, 0.4), (bottom, 0.32)])
        }
        "monitors" => {
            let top = split(
                Row,
                vec![
                    (b.monitor(MonitorKind::Composition), 1.0),
                    (b.monitor(MonitorKind::Layer { layer: None }), 1.0),
                    (b.monitor(MonitorKind::Group { group: None }), 1.0),
                    (b.monitor(MonitorKind::Preview), 1.0),
                ],
            );
            let bottom = split(
                Row,
                vec![(tabs(&["clip", "layer"]), 0.5), (browsers(), 0.5)],
            );
            split(Column, vec![(top, 0.25), (Node::Deck, 0.4), (bottom, 0.35)])
        }
        _ => return None,
    };
    let mut layout = Layout {
        version: VERSION,
        root,
        monitors: b.monitors,
        last: BTreeMap::new(),
    };
    layout.settle();
    Some(layout)
}
