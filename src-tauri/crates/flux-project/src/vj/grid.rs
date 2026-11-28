use super::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vj {
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// The groups of consecutive layers, bottom first (see [`crate::vj_groups`]); absent is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    /// In the order of their tabs.
    pub decks: Vec<Deck>,
    /// Id of the deck the grid shows.
    #[serde(default)]
    pub deck: String,
    /// Where the show's timecode clock comes from; absent is [`ClockSource::Internal`].
    #[serde(default, skip_serializing_if = "ClockSource::is_internal")]
    pub clock: ClockSource,
    /// The show's tempo and beat phase are shared with the other applications of an Ableton Link
    /// session on the network; absent is off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub link: bool,
    /// The composition's effects, applied to the final mix of the layers before the output mapping,
    /// the first applied first. Only here the kinds that need the frames before them (feedback,
    /// trails, bloom) are allowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composition_effects: Vec<Effect>,
    /// Draws the composition without its effects; they keep their place and settings.
    #[serde(default, skip_serializing_if = "is_false")]
    pub composition_bypass: bool,
    /// What a MIDI controller or an OSC address drives (see [`crate::vj_mapping`]); absent is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<Mapping>,
    /// The composition dashboard: up to [`crate::vj_mapping::KNOBS`] knobs, each driving the numbers
    /// it is linked to; a knob not listed is at 0 with no links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboard: Vec<Knob>,
    /// The colours a controller's pads get for each state when a mapping has none of its own;
    /// absent is the controller's preset (see [`crate::vj_feedback`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lights: Option<crate::vj_feedback::Lights>,
    /// The OSC destinations (`host:port`) the grid's state is echoed to, on the addresses that
    /// control it (see [`crate::vj_osc_feedback`]); absent is none, and nothing is sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub osc_feedback: Vec<String>,
}

/// Where the timecode clock of a VJ show comes from: Flux's own transport, or timecode coming in
/// by LTC or MTC, which the clock follows and the operator cannot move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClockSource {
    #[default]
    Internal,
    Ltc,
    Mtc,
}

impl ClockSource {
    fn is_internal(&self) -> bool {
        *self == Self::Internal
    }
}

/// Taps further apart than this start a new count.
const TAP_GAP_MS: f64 = 2000.0;

/// How many of the last taps make the tempo.
const TAP_COUNT: usize = 8;

/// The tempo of `taps` (milliseconds, oldest first): the mean time between the last run of taps
/// that are closer than [`TAP_GAP_MS`], at most [`TAP_COUNT`] of them. `None` before two taps.
pub fn tapped_bpm(taps: &[f64]) -> Option<f64> {
    let mut run = taps.len().checked_sub(1)?;
    while run > 0 && taps.len() - run < TAP_COUNT {
        let gap = taps[run] - taps[run - 1];
        if !(gap > 0.0 && gap <= TAP_GAP_MS) {
            break;
        }
        run -= 1;
    }
    let taps = &taps[run..];
    if taps.len() < 2 {
        return None;
    }
    let mean = (taps[taps.len() - 1] - taps[0]) / (taps.len() - 1) as f64;
    (mean > 0.0).then(|| 60_000.0 / mean)
}

/// Sets the tempo of the show to `change` of the one it has. It is kept in the show's timeline, so
/// the metronome and the engine follow it, brought into range and to two decimals.
pub(super) fn set_bpm(timelines: &mut [Timeline], change: impl FnOnce(f64) -> f64) -> Result<()> {
    let timeline = timelines
        .first_mut()
        .ok_or_else(|| ProjectError::Invalid("a VJ project keeps one timeline".into()))?;
    let bpm = change(timeline.tempo.bpm);
    if !bpm.is_finite() {
        return Err(ProjectError::Invalid("a tempo is a number".into()));
    }
    timeline.tempo.bpm = (bpm.clamp(MIN_BPM, MAX_BPM) * 100.0).round() / 100.0;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    pub id: String,
    /// Absent is the deck's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Left first.
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub id: String,
    /// Absent is the column's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Vj {
    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty()
            && self.decks.is_empty()
            && self.composition_effects.is_empty()
            && self.mappings.is_empty()
            && self.dashboard.is_empty()
            && self.lights.is_none()
            && self.osc_feedback.is_empty()
    }

    /// The grid of a new VJ project: [`DEFAULT_LAYERS`] layers and one deck of [`DEFAULT_COLUMNS`]
    /// columns.
    pub fn starting(mut new_id: impl FnMut() -> String) -> Self {
        let layers = (0..DEFAULT_LAYERS)
            .map(|_| Layer::new(new_id(), false))
            .collect();
        let deck = new_deck(&mut new_id);
        Self {
            layers,
            groups: Vec::new(),
            deck: deck.id.clone(),
            decks: vec![deck],
            clock: ClockSource::Internal,
            link: false,
            composition_effects: Vec::new(),
            composition_bypass: false,
            mappings: Vec::new(),
            dashboard: Vec::new(),
            lights: None,
            osc_feedback: Vec::new(),
        }
    }

    /// Every column of every deck.
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.decks.iter().flat_map(|deck| &deck.columns)
    }

    /// The deck the grid shows.
    pub fn shown_deck(&self) -> Option<&Deck> {
        self.decks.iter().find(|deck| deck.id == self.deck)
    }

    /// The deck holding `column`, and the column's place in it.
    pub fn column_place(&self, column: &str) -> Option<(&Deck, usize)> {
        self.decks.iter().find_map(|deck| {
            let at = deck.columns.iter().position(|c| c.id == column)?;
            Some((deck, at))
        })
    }

    /// The clips of the column launched together, by layer: every deck layer's clip in the
    /// column, `None` for a deck layer without one. Global layers are not in it.
    pub fn column_clips<'a>(
        &'a self,
        column: &'a str,
    ) -> impl Iterator<Item = (&'a Layer, Option<&'a Clip>)> {
        self.layers
            .iter()
            .filter(|layer| !layer.global)
            .map(move |layer| {
                let clip = layer
                    .clips
                    .iter()
                    .find(|clip| clip.column.as_deref() == Some(column));
                (layer, clip)
            })
    }

    /// The first column of the deck shown where `layer` has no clip (no slot, on a global layer).
    pub fn empty_column(&self, layer: &str) -> Option<String> {
        let layer = self.layers.iter().find(|l| l.id == layer)?;
        let deck = self.shown_deck()?;
        deck.columns
            .iter()
            .enumerate()
            .find(|(at, column)| {
                !layer.clips.iter().any(|clip| match clip.cell() {
                    Some(Cell::Column(c)) => c == column.id,
                    Some(Cell::Slot(s)) => s == *at,
                    None => false,
                })
            })
            .map(|(_, column)| column.id.clone())
    }

    /// The clip `layer` launches after `current` by its autopilot, among its clips in the deck
    /// shown (a column each, the same place on a global layer's slots); `roll` is a random number.
    pub fn autopilot_next<'a>(
        &self,
        layer: &'a Layer,
        current: &str,
        roll: u64,
    ) -> Option<&'a Clip> {
        let deck = self.shown_deck()?;
        let cells: Vec<Option<&Clip>> = deck
            .columns
            .iter()
            .enumerate()
            .map(|(at, column)| {
                layer.clips.iter().find(|clip| match clip.cell() {
                    Some(Cell::Column(c)) => c == column.id,
                    Some(Cell::Slot(s)) => s == at,
                    None => false,
                })
            })
            .collect();
        let filled: Vec<bool> = cells.iter().map(Option::is_some).collect();
        let here = cells
            .iter()
            .position(|clip| clip.is_some_and(|clip| clip.id == current));
        layer
            .autopilot
            .pick(&filled, here, roll)
            .and_then(|at| cells[at])
    }

    /// The effect `id` of a clip, a layer or the composition.
    pub fn find_effect(&self, id: &str) -> Option<&Effect> {
        let layers = self.layers.iter().flat_map(|layer| {
            layer
                .effects
                .iter()
                .chain(layer.clips.iter().flat_map(|clip| &clip.effects))
        });
        let groups = self.groups.iter().flat_map(|group| &group.effects);
        self.composition_effects
            .iter()
            .chain(groups)
            .chain(layers)
            .find(|effect| effect.id == id)
    }

    /// The layer showing `clip`, and the clip.
    pub fn find_clip(&self, clip: &str) -> Option<(&Layer, &Clip)> {
        self.layers.iter().find_map(|layer| {
            let found = layer.clips.iter().find(|c| c.id == clip)?;
            Some((layer, found))
        })
    }

    pub(crate) fn layer_mut(&mut self, id: &str) -> Result<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
    }

    pub(crate) fn clip_mut(&mut self, id: &str) -> Result<&mut Clip> {
        self.layers
            .iter_mut()
            .flat_map(|layer| &mut layer.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no clip {id}")))
    }

    /// The stack holding the effect `id` (of a layer, of a clip or of the composition), and the
    /// effect's place in it.
    pub(crate) fn effect_mut(&mut self, id: &str) -> Result<(&mut Vec<Effect>, usize)> {
        let layers = self.layers.iter_mut().flat_map(|layer| {
            std::iter::once(&mut layer.effects)
                .chain(layer.clips.iter_mut().map(|clip| &mut clip.effects))
        });
        std::iter::once(&mut self.composition_effects)
            .chain(self.groups.iter_mut().map(|group| &mut group.effects))
            .chain(layers)
            .find_map(|stack| {
                let at = stack.iter().position(|effect| effect.id == id)?;
                Some((stack, at))
            })
            .ok_or_else(|| ProjectError::Invalid(format!("no effect {id}")))
    }

    pub(super) fn deck_index(&self, id: &str) -> Result<usize> {
        self.decks
            .iter()
            .position(|deck| deck.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no deck {id}")))
    }

    /// The deck holding `column` and the column's place in it, by index.
    pub(super) fn column_index(&self, id: &str) -> Result<(usize, usize)> {
        self.decks
            .iter()
            .enumerate()
            .find_map(|(d, deck)| Some((d, deck.columns.iter().position(|c| c.id == id)?)))
            .ok_or_else(|| ProjectError::Invalid(format!("no column {id}")))
    }

    /// The cell of `layer` that `column` names: the column itself on a deck layer, the slot at the
    /// column's place on a global layer.
    pub(super) fn cell_of(&self, layer: &str, column: &str) -> Result<Cell> {
        let (_, at) = self.column_index(column)?;
        let layer = self
            .layers
            .iter()
            .find(|l| l.id == layer)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {layer}")))?;
        Ok(if layer.global {
            Cell::Slot(at)
        } else {
            Cell::Column(column.to_owned())
        })
    }
}

pub(super) fn new_deck(new_id: &mut impl FnMut() -> String) -> Deck {
    Deck {
        id: new_id(),
        name: None,
        columns: (0..DEFAULT_COLUMNS)
            .map(|_| Column {
                id: new_id(),
                name: None,
            })
            .collect(),
    }
}
