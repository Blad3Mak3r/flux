use super::*;

pub(super) fn fail<T>(text: impl Into<String>) -> Result<T> {
    Err(ProjectError::Invalid(text.into()))
}

pub(super) fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub(super) fn is_monitor(panel: &str) -> bool {
    panel.starts_with(MONITOR_PREFIX)
}

/// The least size, in px, a panel is usable at.
pub fn panel_min_size(panel: &str) -> (u32, u32) {
    match panel {
        _ if is_monitor(panel) => (200, 150),
        "mappings" => (300, 200),
        "dashboard" => (260, 140),
        "files" | "generators" | "effects" => (240, 180),
        _ => (260, 180),
    }
}

/// The least size of the deck, in px.
pub const DECK_MIN_SIZE: (u32, u32) = (480, 260);

/// The least size of a subtree: a row sums widths and gaps and takes the tallest, a column the
/// converse, tabs the largest of their panels.
pub fn min_size(node: &Node) -> (u32, u32) {
    match node {
        Node::Deck => DECK_MIN_SIZE,
        Node::Tabs { panels, .. } => panels
            .iter()
            .map(|p| panel_min_size(p))
            .fold((0, 0), |(w, h), (pw, ph)| (w.max(pw), h.max(ph))),
        Node::Split { dir, children, .. } => {
            let gaps = SPLITTER_GAP * children.len().saturating_sub(1) as u32;
            let sizes = children.iter().map(min_size);
            match dir {
                Dir::Row => sizes.fold((gaps, 0), |(w, h), (cw, ch)| (w + cw, h.max(ch))),
                Dir::Column => sizes.fold((0, gaps), |(w, h), (cw, ch)| (w.max(cw), h + ch)),
            }
        }
    }
}

pub(super) fn good_sizes(sizes: &[f32]) -> bool {
    sizes.iter().all(|s| s.is_finite() && *s > 0.0)
}

/// Drops empty tab groups, collapses single-child splits, merges a split into a parent of its own
/// direction, renormalises the shares and clamps `active`.
pub(super) fn normalise(node: Node) -> Option<Node> {
    match node {
        Node::Deck => Some(Node::Deck),
        Node::Tabs { id, panels, active } => (!panels.is_empty()).then(|| Node::Tabs {
            id,
            active: active.min(panels.len() - 1),
            panels,
        }),
        Node::Split {
            id,
            dir,
            children,
            sizes,
        } => {
            let sizes = if sizes.len() == children.len() && good_sizes(&sizes) {
                sizes
            } else {
                vec![1.0; children.len()]
            };
            let mut out: Vec<(Node, f32)> = Vec::new();
            for (child, share) in children.into_iter().zip(sizes) {
                match normalise(child) {
                    Some(Node::Split {
                        dir: inner,
                        children,
                        sizes,
                        ..
                    }) if inner == dir => {
                        out.extend(
                            children
                                .into_iter()
                                .zip(sizes.into_iter().map(|s| s * share)),
                        );
                    }
                    Some(other) => out.push((other, share)),
                    None => {}
                }
            }
            if out.len() <= 1 {
                return out.pop().map(|(node, _)| node);
            }
            let total: f32 = out.iter().map(|(_, s)| s).sum();
            let (children, sizes) = out.into_iter().map(|(n, s)| (n, s / total)).unzip();
            Some(Node::Split {
                id,
                dir,
                children,
                sizes,
            })
        }
    }
}

pub(super) fn tabs_mut<'a>(
    node: &'a mut Node,
    group: &str,
) -> Option<(&'a mut Vec<PanelId>, &'a mut usize)> {
    match node {
        Node::Tabs { id, panels, active } if id == group => Some((panels, active)),
        Node::Split { children, .. } => children.iter_mut().find_map(|c| tabs_mut(c, group)),
        _ => None,
    }
}

pub(super) fn collect<'a>(
    node: &'a Node,
    tabs: &mut Vec<(&'a str, &'a [PanelId])>,
    decks: &mut usize,
) {
    match node {
        Node::Deck => *decks += 1,
        Node::Tabs { id, panels, .. } => tabs.push((id, panels)),
        Node::Split { children, .. } => children.iter().for_each(|c| collect(c, tabs, decks)),
    }
}

/// Replaces the node addressed by `target` (a tab group id or the deck) with `f` of it.
pub(super) fn replace(node: &mut Node, target: &str, f: &mut Option<impl FnOnce(Node) -> Node>) {
    let here = match node {
        Node::Deck => target == DECK,
        Node::Tabs { id, .. } => id == target,
        Node::Split { children, .. } => {
            children.iter_mut().for_each(|c| replace(c, target, f));
            false
        }
    };
    if here && let Some(f) = f.take() {
        *node = f(std::mem::replace(node, Node::Deck));
    }
}

/// The area, as a fraction of the screen, of each tab group.
pub(super) fn areas<'a>(node: &'a Node, share: f32, out: &mut Vec<(&'a str, f32)>) {
    match node {
        Node::Tabs { id, .. } => out.push((id, share)),
        Node::Split {
            children, sizes, ..
        } => children
            .iter()
            .zip(sizes)
            .for_each(|(c, s)| areas(c, share * s, out)),
        Node::Deck => {}
    }
}
