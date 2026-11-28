use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub id: String,
    pub name: String,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SavedInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FactoryInfo {
    pub id: &'static str,
}

/// What the UI shows: the current layout and the names it can switch to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub current: Layout,
    pub saved: Vec<SavedInfo>,
    pub factory: Vec<FactoryInfo>,
    pub clip_size: ClipSize,
    /// The least `(width, height)` in px of every split, tab group and `"deck"`, so the UI clamps
    /// a splitter drag without knowing the rules.
    pub mins: BTreeMap<String, (u32, u32)>,
    /// The gap a splitter takes between two children, in px.
    pub gap: u32,
    /// For each monitor that shares a row (a `row` split) with another monitor, the id of that
    /// split: the nearest row around it. The UI offers equal widths for it.
    #[serde(rename = "rowOf")]
    pub row_of: BTreeMap<PanelId, String>,
}

fn collect_rows(node: &Node, row: Option<&str>, out: &mut Vec<(PanelId, String)>) {
    match node {
        Node::Split {
            id, dir, children, ..
        } => {
            let row = if *dir == Dir::Row {
                Some(id.as_str())
            } else {
                row
            };
            children.iter().for_each(|c| collect_rows(c, row, out));
        }
        Node::Tabs { panels, .. } => out.extend(row.into_iter().flat_map(|r| {
            panels
                .iter()
                .filter(|p| is_monitor(p))
                .map(move |p| (p.clone(), r.to_owned()))
        })),
        Node::Deck => {}
    }
}

fn collect_mins(node: &Node, out: &mut BTreeMap<String, (u32, u32)>) {
    let id = match node {
        Node::Deck => DECK,
        Node::Tabs { id, .. } => id,
        Node::Split { id, children, .. } => {
            children.iter().for_each(|c| collect_mins(c, out));
            id
        }
    };
    out.insert(id.to_owned(), min_size(node));
}

/// The file `vj-layout.json`: the current layout and the user's named ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub version: u32,
    pub current: Layout,
    #[serde(default)]
    pub saved: Vec<Saved>,
    #[serde(default, skip_serializing_if = "ClipSize::is_medium")]
    pub clip_size: ClipSize,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: VERSION,
            current: Layout::default(),
            saved: Vec::new(),
            clip_size: ClipSize::default(),
        }
    }
}

impl Store {
    /// Reads the store; a file that is invalid or newer gives the default one, never an error.
    pub fn parse(text: &str) -> Self {
        let Ok(mut store) = serde_json::from_str::<Self>(text) else {
            return Self::default();
        };
        if store.version != VERSION || store.current.validate().is_err() {
            return Self::default();
        }
        store.saved.retain(|s| s.layout.validate().is_ok());
        store
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Self::parse(&text)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(invalid)?)
    }

    pub fn state(&self) -> State {
        let mut mins = BTreeMap::new();
        collect_mins(&self.current.root, &mut mins);
        let mut rows = Vec::new();
        collect_rows(&self.current.root, None, &mut rows);
        let row_of = rows
            .iter()
            .filter(|(_, r)| rows.iter().filter(|(_, o)| o == r).count() > 1)
            .cloned()
            .collect();
        State {
            mins,
            row_of,
            gap: SPLITTER_GAP,
            current: self.current.clone(),
            saved: self
                .saved
                .iter()
                .map(|s| SavedInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                })
                .collect(),
            factory: FACTORY.iter().map(|id| FactoryInfo { id }).collect(),
            clip_size: self.clip_size,
        }
    }

    /// Makes a built-in or saved layout the current one.
    pub fn apply(&mut self, id: &str) -> Result<()> {
        let layout = factory(id).or_else(|| {
            self.saved
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.layout.clone())
        });
        match layout {
            Some(layout) => {
                self.current = layout;
                Ok(())
            }
            None => fail(format!("no layout {id}")),
        }
    }

    /// Keeps the current layout under `name`; the same name is overwritten.
    pub fn save_as(&mut self, name: &str) -> Result<()> {
        let name = name_of(name)?;
        let layout = self.current.clone();
        let full = self.saved.len() >= MAX_SAVED;
        match self.saved.iter_mut().find(|s| s.name == name) {
            Some(saved) => saved.layout = layout,
            None if full => return fail(format!("at most {MAX_SAVED} layouts")),
            None => self.saved.push(Saved {
                id: new_id(),
                name,
                layout,
            }),
        }
        Ok(())
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = name_of(name)?;
        if self.saved.iter().any(|s| s.id != id && s.name == name) {
            return fail("a layout has that name");
        }
        match self.saved.iter_mut().find(|s| s.id == id) {
            Some(saved) => {
                saved.name = name;
                Ok(())
            }
            None => fail(format!("no saved layout {id}")),
        }
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let before = self.saved.len();
        self.saved.retain(|s| s.id != id);
        if self.saved.len() == before {
            return fail(format!("no saved layout {id}"));
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.current = Layout::default();
    }
}
