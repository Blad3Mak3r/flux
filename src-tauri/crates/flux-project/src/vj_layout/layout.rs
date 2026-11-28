use super::*;

impl Layout {
    fn tabs(&self) -> (Vec<(&str, &[PanelId])>, usize) {
        let (mut tabs, mut decks) = (Vec::new(), 0);
        collect(&self.root, &mut tabs, &mut decks);
        (tabs, decks)
    }

    /// The tab group holding `panel`.
    pub(super) fn group_of(&self, panel: &str) -> Option<String> {
        let (tabs, _) = self.tabs();
        tabs.iter()
            .find(|(_, panels)| panels.iter().any(|p| p == panel))
            .map(|(id, _)| (*id).to_owned())
    }

    pub(super) fn settle(&mut self) {
        let root = std::mem::replace(&mut self.root, Node::Deck);
        self.root = normalise(root).unwrap_or(Node::Deck);
    }

    /// Checks the tree and the monitors; the error says what is wrong.
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return fail(format!("layout version {}", self.version));
        }
        let (tabs, decks) = self.tabs();
        if decks != 1 {
            return fail(format!("a layout has one deck, this has {decks}"));
        }
        let mut seen: Vec<&str> = Vec::new();
        for (id, panels) in &tabs {
            if panels.is_empty() {
                return fail(format!("tab group {id} is empty"));
            }
            for p in *panels {
                let known =
                    FIXED_PANELS.contains(&p.as_str()) || self.monitors.iter().any(|m| &m.id == p);
                if !known || seen.contains(&p.as_str()) {
                    return fail(format!("panel {p} is unknown or twice"));
                }
                seen.push(p);
            }
        }
        if self.monitors.len() > MAX_MONITORS {
            return fail(format!("at most {MAX_MONITORS} monitors"));
        }
        if let Some(m) = self
            .monitors
            .iter()
            .find(|m| !is_monitor(&m.id) || !seen.contains(&m.id.as_str()))
        {
            return fail(format!("monitor {} is not in the layout", m.id));
        }
        if let Some(m) = self.monitors.iter().find(|m| m.kind.bad_screen()) {
            return fail(format!("monitor {} shows a screen with no id", m.id));
        }
        let mut ids: Vec<&str> = tabs.iter().map(|(id, _)| *id).collect();
        self.check(&self.root, &mut ids)
    }

    /// Splits have as many sizes as children, positive, and their ids are unique.
    fn check<'a>(&'a self, node: &'a Node, ids: &mut Vec<&'a str>) -> Result<()> {
        if let Node::Split {
            id,
            children,
            sizes,
            ..
        } = node
        {
            if sizes.len() != children.len() || children.len() < 2 || !good_sizes(sizes) {
                return fail(format!("split {id} has bad sizes"));
            }
            if ids.contains(&id.as_str()) {
                return fail(format!("id {id} is twice"));
            }
            ids.push(id);
            children.iter().try_for_each(|c| self.check(c, ids))?;
        }
        Ok(())
    }

    /// Reads a layout; a file that is not one, is invalid or is of another version gives the default.
    pub fn parse(text: &str) -> Self {
        serde_json::from_str::<Self>(text)
            .ok()
            .filter(|layout| layout.validate().is_ok())
            .unwrap_or_default()
    }

    /// Moves a panel to a tab group (centre) or beside a tab group or the deck (an edge).
    pub fn move_panel(&mut self, panel: &str, target: &Target) -> Result<()> {
        let Some(from) = self.group_of(panel) else {
            return fail(format!("panel {panel} is not open"));
        };
        let (tabs, _) = self.tabs();
        let group = tabs.iter().find(|(id, _)| *id == target.node);
        if target.node != DECK && group.is_none() {
            return fail(format!("no dock {}", target.node));
        }
        if target.zone == Zone::Center && target.node == DECK {
            return fail("the deck takes no tabs");
        }
        if target.node == from
            && (target.zone == Zone::Center || group.is_some_and(|g| g.1.len() == 1))
        {
            return Ok(());
        }
        self.take(panel);
        if target.zone == Zone::Center {
            let (panels, active) = tabs_mut(&mut self.root, &target.node).expect("checked above");
            panels.push(panel.to_owned());
            *active = panels.len() - 1;
        } else {
            self.dock(panel, target);
        }
        self.settle();
        Ok(())
    }

    fn take(&mut self, panel: &str) {
        if let Some(group) = self.group_of(panel) {
            let (panels, active) = tabs_mut(&mut self.root, &group).expect("it was found");
            panels.retain(|p| p != panel);
            *active = (*active).min(panels.len().saturating_sub(1));
        }
    }

    fn dock(&mut self, panel: &str, target: &Target) {
        let (dir, before) = match target.zone {
            Zone::Left => (Dir::Row, true),
            Zone::Right => (Dir::Row, false),
            Zone::Top => (Dir::Column, true),
            _ => (Dir::Column, false),
        };
        let new = Node::Tabs {
            id: new_id(),
            panels: vec![panel.to_owned()],
            active: 0,
        };
        replace(
            &mut self.root,
            &target.node,
            &mut Some(|old: Node| {
                let children = if before {
                    vec![new, old]
                } else {
                    vec![old, new]
                };
                Node::Split {
                    id: new_id(),
                    dir,
                    children,
                    sizes: vec![0.5, 0.5],
                }
            }),
        );
    }

    pub fn reorder_tab(&mut self, group: &str, from: usize, to: usize) -> Result<()> {
        let Some((panels, active)) = tabs_mut(&mut self.root, group) else {
            return fail(format!("no dock {group}"));
        };
        if from >= panels.len() || to >= panels.len() {
            return fail("tab out of range");
        }
        let panel = panels.remove(from);
        panels.insert(to, panel);
        *active = to;
        Ok(())
    }

    pub fn activate(&mut self, group: &str, index: usize) -> Result<()> {
        match tabs_mut(&mut self.root, group) {
            Some((panels, active)) if index < panels.len() => {
                *active = index;
                Ok(())
            }
            Some(_) => fail("tab out of range"),
            None => fail(format!("no dock {group}")),
        }
    }

    /// Closes a panel; a monitor loses its settings. The deck cannot be closed.
    pub fn close_panel(&mut self, panel: &str) -> Result<()> {
        let Some(group) = self.group_of(panel) else {
            return fail(if panel == DECK {
                "the deck cannot be closed".to_owned()
            } else {
                format!("panel {panel} is not open")
            });
        };
        self.take(panel);
        if is_monitor(panel) {
            self.monitors.retain(|m| m.id != panel);
        } else {
            self.last.insert(panel.to_owned(), group);
        }
        self.settle();
        Ok(())
    }

    /// Opens a closed fixed panel, or a new monitor of `monitor` when `panel` is `"monitor"`;
    /// answers with the panel's id. It goes back to the group it was last in, else into the
    /// largest group, else right of the deck. A panel already open stays where it is.
    pub fn open_panel(&mut self, panel: &str, monitor: Option<MonitorKind>) -> Result<PanelId> {
        let id = if panel == "monitor" {
            if self.monitors.len() >= MAX_MONITORS {
                return fail(format!("at most {MAX_MONITORS} monitors"));
            }
            if monitor.as_ref().is_some_and(MonitorKind::bad_screen) {
                return fail("a screen monitor needs a screen");
            }
            let id = format!("{MONITOR_PREFIX}{}", new_id());
            self.monitors.push(Monitor {
                id: id.clone(),
                kind: monitor.unwrap_or(MonitorKind::Preview),
            });
            id
        } else if FIXED_PANELS.contains(&panel) {
            if self.group_of(panel).is_some() {
                return Ok(panel.to_owned());
            }
            panel.to_owned()
        } else {
            return fail(format!("no panel {panel}"));
        };
        let remembered = self.last.remove(&id);
        let group = {
            let mut best = Vec::new();
            areas(&self.root, 1.0, &mut best);
            let known = self
                .tabs()
                .0
                .iter()
                .any(|(g, _)| Some(*g) == remembered.as_deref());
            if known {
                remembered
            } else {
                best.iter()
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(g, _)| (*g).to_owned())
            }
        };
        match group {
            Some(group) => {
                let (panels, active) = tabs_mut(&mut self.root, &group).expect("it exists");
                panels.push(id.clone());
                *active = panels.len() - 1;
            }
            None => self.dock(
                &id,
                &Target {
                    node: DECK.into(),
                    zone: Zone::Right,
                },
            ),
        }
        self.settle();
        Ok(id)
    }

    /// Sets the shares of a split, then renormalises them.
    pub fn resize(&mut self, split: &str, new: Vec<f32>) -> Result<()> {
        fn find<'a>(node: &'a mut Node, split: &str) -> Option<(usize, &'a mut Vec<f32>)> {
            let Node::Split {
                id,
                children,
                sizes,
                ..
            } = node
            else {
                return None;
            };
            if id == split {
                return Some((children.len(), sizes));
            }
            children.iter_mut().find_map(|c| find(c, split))
        }
        let Some((count, sizes)) = find(&mut self.root, split) else {
            return fail(format!("no split {split}"));
        };
        if new.len() != count || !good_sizes(&new) {
            return fail("sizes must be positive, one per child");
        }
        let total: f32 = new.iter().sum();
        *sizes = new.into_iter().map(|s| s / total).collect();
        Ok(())
    }

    /// Gives every child of a split the same share.
    pub fn equalize(&mut self, split: &str) -> Result<()> {
        fn find<'a>(node: &'a Node, split: &str) -> Option<&'a Node> {
            match node {
                Node::Split { id, .. } if id == split => Some(node),
                Node::Split { children, .. } => children.iter().find_map(|c| find(c, split)),
                _ => None,
            }
        }
        let Some(Node::Split { children, .. }) = find(&self.root, split) else {
            return fail(format!("no split {split}"));
        };
        let equal = vec![1.0; children.len()];
        self.resize(split, equal)
    }

    pub fn set_monitor(&mut self, id: &str, kind: MonitorKind) -> Result<()> {
        if kind.bad_screen() {
            return fail("a screen monitor needs a screen");
        }
        match self.monitors.iter_mut().find(|m| m.id == id) {
            Some(monitor) => {
                monitor.kind = kind;
                Ok(())
            }
            None => fail(format!("no monitor {id}")),
        }
    }
}
