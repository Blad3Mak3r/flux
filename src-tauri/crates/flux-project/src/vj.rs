//! The VJ grid of a project, played like a media server's clip deck: layers stacked one over the
//! other, columns across them, and in each cell at most one clip, a video the user launches by hand.
//! A project is of one kind for good, chosen when it is created: a timeline project has no grid, and
//! a VJ project plays its grid. A VJ project still holds a timeline, so that one day its grid can
//! follow the clock.
//!
//! The columns come in decks: sets of columns of their own, one of which the grid shows. The layers
//! are the same in every deck, and a deck holds the clips its columns have on each layer. A global
//! layer keeps its clips in slots of its own instead, the same in every deck, so what it plays (a
//! logo, a mask) does not change with the deck shown, and launching a column leaves it alone.
//!
//! The layers are listed in drawing order: the first is at the bottom, the last on top. The levels,
//! blend, transition and crossfader side of a layer are stored with the project; which clip is
//! playing is not, it belongs to the show running.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{MAX_CLIP_NAME, Project, VideoFit};
use crate::{ProjectError, Result};

/// Most layers a grid has.
pub const MAX_LAYERS: usize = 16;
/// Most columns a deck has, and most slots a global layer has.
pub const MAX_COLUMNS: usize = 64;
/// Most decks a grid has.
pub const MAX_DECKS: usize = 32;
/// The longest transition between two clips of a layer, in seconds.
pub const MAX_TRANSITION: f32 = 10.0;
/// The grid a new VJ project starts with.
pub const DEFAULT_LAYERS: usize = 3;
pub const DEFAULT_COLUMNS: usize = 8;

/// The kind of a project, chosen when it is created and never changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectMode {
    /// The timelines, their tracks and cues, and the timecode.
    #[default]
    Timeline,
    /// The VJ grid.
    Vj,
}

impl ProjectMode {
    pub(crate) fn is_timeline(&self) -> bool {
        *self == Self::Timeline
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Vj {
    /// Bottom first.
    pub layers: Vec<Layer>,
    /// In the order of their tabs.
    pub decks: Vec<Deck>,
    /// Id of the deck the grid shows.
    #[serde(default)]
    pub deck: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    /// Absent is the layer's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A global layer keeps its clips in slots, the same in every deck; chosen when the layer is
    /// added.
    #[serde(default, skip_serializing_if = "is_false")]
    pub global: bool,
    /// The layer's master, 0 to 1: scales both its picture and its sound. Absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub master: f32,
    /// How much the picture covers the layers below, 0 to 1; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub opacity: f32,
    /// The level of the sound of the layer's clips, 0 to 1 as the fader stands; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub volume: f32,
    /// How the picture mixes with the layers below; absent is [`Blend::Alpha`].
    #[serde(default, skip_serializing_if = "Blend::is_alpha")]
    pub blend: Blend,
    /// How long a clip launched on the layer takes to replace the one playing, in seconds, 0 to
    /// [`MAX_TRANSITION`]; absent is 0, a cut.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transition: f32,
    /// How the two clips mix during a transition; absent is [`Transition::Alpha`].
    #[serde(default, skip_serializing_if = "Transition::is_alpha")]
    pub transition_effect: Transition,
    /// The side of the crossfader the layer is on; absent is neither, so the crossfader leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossfade: Option<Side>,
    /// Bypassed: the layer shows nothing and sounds nothing, though its clip keeps playing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Soloed: while any layer is, only the soloed ones show.
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// At most one per column of each deck (per slot on a global layer), in no particular order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
}

fn full() -> f32 {
    1.0
}

fn is_full(value: &f32) -> bool {
    *value == 1.0
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Layer {
    fn new(id: String, global: bool) -> Self {
        Self {
            id,
            name: None,
            global,
            master: 1.0,
            opacity: 1.0,
            volume: 1.0,
            blend: Blend::Alpha,
            transition: 0.0,
            transition_effect: Transition::Alpha,
            crossfade: None,
            bypass: false,
            solo: false,
            clips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    A,
    B,
}

/// How a layer's picture mixes with what the layers below show, at the layer's opacity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Blend {
    /// Covers what is below.
    #[default]
    Alpha,
    /// Adds its light to what is below.
    Add,
    /// Takes its light from what is below.
    Subtract,
    /// Darkens what is below by its colour.
    Multiply,
    /// Lightens what is below, softer than adding.
    Screen,
    /// Keeps the lighter of the two.
    Lighten,
    /// Keeps the darker of the two.
    Darken,
}

impl Blend {
    fn is_alpha(&self) -> bool {
        *self == Self::Alpha
    }
}

/// How the clip leaving a layer and the one arriving mix while the layer's transition runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transition {
    /// The new clip appears over the old one, which stays whole until it is covered.
    #[default]
    Alpha,
    /// The old clip fades out while the new one fades in, their light added.
    Add,
}

impl Transition {
    fn is_alpha(&self) -> bool {
        *self == Self::Alpha
    }
}

/// What launching a clip by hand does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Trigger {
    /// A press launches the clip, which plays on.
    #[default]
    Normal,
    /// The clip plays while it is held down and stops when it is let go.
    Piano,
}

impl Trigger {
    fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}

/// A video in a cell of the grid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    /// On a deck layer, the id of the column the clip is in; the layer is the one that lists it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
    /// On a global layer, the place of the clip from the left, the same in every deck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<usize>,
    /// Id of an entry in `Project::videos`.
    pub asset: String,
    /// Absent is the video's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the clip does when it reaches its end; absent is [`Playback::Loop`].
    #[serde(default, skip_serializing_if = "Playback::is_loop")]
    pub playback: Playback,
    /// What a press on the clip does; absent is [`Trigger::Normal`].
    #[serde(default, skip_serializing_if = "Trigger::is_normal")]
    pub trigger: Trigger,
    /// How the video is sized into the composition; absent is [`VideoFit::Fit`].
    #[serde(default, skip_serializing_if = "is_fit")]
    pub fit: VideoFit,
    /// How much larger than its fit the clip is drawn, about its centre: 1 is 100%. Absent is 1.
    #[serde(default = "full_scale", skip_serializing_if = "is_full_scale")]
    pub scale: f32,
    /// How far its centre is moved from where its fit puts it, as fractions of the composition's
    /// width and height, right and down. Absent is not moved.
    #[serde(default, skip_serializing_if = "is_centred")]
    pub position: [f32; 2],
}

fn is_fit(fit: &VideoFit) -> bool {
    *fit == VideoFit::Fit
}

fn full_scale() -> f32 {
    1.0
}

fn is_full_scale(scale: &f32) -> bool {
    *scale == 1.0
}

fn is_centred(position: &[f32; 2]) -> bool {
    *position == [0.0, 0.0]
}

/// Largest scale of a clip: 1000%.
pub const MAX_SCALE: f32 = 10.0;
/// Farthest a clip's centre moves from its place, in compositions.
pub const MAX_OFFSET: f32 = 2.0;

/// What a launched clip does when it reaches the end of its video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Playback {
    /// Starts again from the beginning.
    #[default]
    Loop,
    /// Stops, and the layer shows nothing.
    Once,
    /// Plays backwards to the beginning, then forwards again.
    PingPong,
}

impl Playback {
    fn is_loop(&self) -> bool {
        *self == Self::Loop
    }
}

/// Where a clip sits: a column of a deck, or a slot of a global layer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Cell {
    Column(String),
    Slot(usize),
}

impl Clip {
    fn cell(&self) -> Option<Cell> {
        match (&self.column, self.slot) {
            (Some(column), None) => Some(Cell::Column(column.clone())),
            (None, Some(slot)) => Some(Cell::Slot(slot)),
            _ => None,
        }
    }

    fn put(&mut self, cell: Cell) {
        match cell {
            Cell::Column(column) => (self.column, self.slot) = (Some(column), None),
            Cell::Slot(slot) => (self.column, self.slot) = (None, Some(slot)),
        }
    }
}

/// What the user can do to the grid. Every id names something of the grid; a name that is empty
/// after trimming goes back to the default one. A level out of its range is brought into it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VjOp {
    /// A new empty layer on top of the others, global or of the decks.
    AddLayer {
        #[serde(default)]
        global: bool,
    },
    /// Takes the layer and its clips away; the last layer stays.
    RemoveLayer {
        layer: String,
    },
    /// Puts the layer at `to` of the drawing order, 0 being the bottom.
    MoveLayer {
        layer: String,
        to: usize,
    },
    RenameLayer {
        layer: String,
        name: String,
    },
    LayerMaster {
        layer: String,
        value: f32,
    },
    LayerOpacity {
        layer: String,
        value: f32,
    },
    LayerVolume {
        layer: String,
        value: f32,
    },
    LayerBlend {
        layer: String,
        blend: Blend,
    },
    /// In seconds.
    LayerTransition {
        layer: String,
        value: f32,
    },
    LayerTransitionEffect {
        layer: String,
        effect: Transition,
    },
    LayerCrossfade {
        layer: String,
        side: Option<Side>,
    },
    LayerBypass {
        layer: String,
        on: bool,
    },
    LayerSolo {
        layer: String,
        on: bool,
    },
    /// A new deck with [`DEFAULT_COLUMNS`] empty columns, after the others, which the grid shows.
    AddDeck,
    /// Takes the deck, its columns and their clips away; the last deck stays.
    RemoveDeck {
        deck: String,
    },
    MoveDeck {
        deck: String,
        to: usize,
    },
    RenameDeck {
        deck: String,
        name: String,
    },
    /// Shows the deck; what plays keeps playing.
    SelectDeck {
        deck: String,
    },
    /// A new empty column on the right of the deck the grid shows.
    AddColumn,
    /// Takes the column and its clips away; the last column of a deck stays.
    RemoveColumn {
        column: String,
    },
    /// Moves the column to `to` within its deck.
    MoveColumn {
        column: String,
        to: usize,
    },
    RenameColumn {
        column: String,
        name: String,
    },
    /// Puts a video of the project in a cell, in place of the clip that was there. The cell is the
    /// layer's in `column`; on a global layer, the slot at the place of `column` in its deck.
    PlaceVideo {
        asset: String,
        layer: String,
        column: String,
    },
    /// Moves a clip to a cell, named as for [`VjOp::PlaceVideo`]; a clip already there takes the
    /// place the moved one left.
    MoveClip {
        clip: String,
        layer: String,
        column: String,
    },
    RemoveClip {
        clip: String,
    },
    RenameClip {
        clip: String,
        name: String,
    },
    ClipPlayback {
        clip: String,
        playback: Playback,
    },
    ClipTrigger {
        clip: String,
        trigger: Trigger,
    },
    ClipFit {
        clip: String,
        fit: VideoFit,
    },
    /// Draws the clip `value` times its fitted size (1 is 100%), up to [`MAX_SCALE`].
    ClipScale {
        clip: String,
        value: f32,
    },
    /// Moves the clip's centre from where its fit puts it, in fractions of the composition, up to
    /// [`MAX_OFFSET`] either way.
    ClipPosition {
        clip: String,
        x: f32,
        y: f32,
    },
}

impl Vj {
    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty() && self.decks.is_empty()
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
            deck: deck.id.clone(),
            decks: vec![deck],
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

    /// The layer showing `clip`, and the clip.
    pub fn find_clip(&self, clip: &str) -> Option<(&Layer, &Clip)> {
        self.layers.iter().find_map(|layer| {
            let found = layer.clips.iter().find(|c| c.id == clip)?;
            Some((layer, found))
        })
    }

    fn layer_mut(&mut self, id: &str) -> Result<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|layer| layer.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
    }

    fn clip_mut(&mut self, id: &str) -> Result<&mut Clip> {
        self.layers
            .iter_mut()
            .flat_map(|layer| &mut layer.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no clip {id}")))
    }

    fn deck_index(&self, id: &str) -> Result<usize> {
        self.decks
            .iter()
            .position(|deck| deck.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no deck {id}")))
    }

    /// The deck holding `column` and the column's place in it, by index.
    fn column_index(&self, id: &str) -> Result<(usize, usize)> {
        self.decks
            .iter()
            .enumerate()
            .find_map(|(d, deck)| Some((d, deck.columns.iter().position(|c| c.id == id)?)))
            .ok_or_else(|| ProjectError::Invalid(format!("no column {id}")))
    }

    /// The cell of `layer` that `column` names: the column itself on a deck layer, the slot at the
    /// column's place on a global layer.
    fn cell_of(&self, layer: &str, column: &str) -> Result<Cell> {
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

fn new_deck(new_id: &mut impl FnMut() -> String) -> Deck {
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

/// A level as a fader stands, 0 to 1.
fn level(value: f32) -> Result<f32> {
    within(value, 0.0, 1.0)
}

fn within(value: f32, min: f32, max: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value.clamp(min, max))
    } else {
        Err(ProjectError::Invalid("a level is a number".into()))
    }
}

/// Applies `op` to a VJ project; `new_id` names whatever it creates. The caller validates the result.
pub fn apply(project: &mut Project, op: VjOp, mut new_id: impl FnMut() -> String) -> Result<()> {
    if project.mode != ProjectMode::Vj {
        return Err(ProjectError::Invalid("only a VJ project has a grid".into()));
    }
    let vj = &mut project.vj;
    match op {
        VjOp::AddLayer { global } => {
            if vj.layers.len() >= MAX_LAYERS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_LAYERS} layers"
                )));
            }
            vj.layers.push(Layer::new(new_id(), global));
        }
        VjOp::RemoveLayer { layer } => {
            let index = layer_index(vj, &layer)?;
            if vj.layers.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a layer".into()));
            }
            vj.layers.remove(index);
        }
        VjOp::MoveLayer { layer, to } => {
            let index = layer_index(vj, &layer)?;
            let moved = vj.layers.remove(index);
            vj.layers.insert(to.min(vj.layers.len()), moved);
        }
        VjOp::RenameLayer { layer, name } => vj.layer_mut(&layer)?.name = typed_name(&name)?,
        VjOp::LayerMaster { layer, value } => vj.layer_mut(&layer)?.master = level(value)?,
        VjOp::LayerOpacity { layer, value } => vj.layer_mut(&layer)?.opacity = level(value)?,
        VjOp::LayerVolume { layer, value } => vj.layer_mut(&layer)?.volume = level(value)?,
        VjOp::LayerBlend { layer, blend } => vj.layer_mut(&layer)?.blend = blend,
        VjOp::LayerTransition { layer, value } => {
            vj.layer_mut(&layer)?.transition = level(value / MAX_TRANSITION)? * MAX_TRANSITION;
        }
        VjOp::LayerTransitionEffect { layer, effect } => {
            vj.layer_mut(&layer)?.transition_effect = effect;
        }
        VjOp::LayerCrossfade { layer, side } => vj.layer_mut(&layer)?.crossfade = side,
        VjOp::LayerBypass { layer, on } => vj.layer_mut(&layer)?.bypass = on,
        VjOp::LayerSolo { layer, on } => vj.layer_mut(&layer)?.solo = on,
        VjOp::AddDeck => {
            if vj.decks.len() >= MAX_DECKS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_DECKS} decks"
                )));
            }
            let deck = new_deck(&mut new_id);
            vj.deck = deck.id.clone();
            vj.decks.push(deck);
        }
        VjOp::RemoveDeck { deck } => {
            let index = vj.deck_index(&deck)?;
            if vj.decks.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a deck".into()));
            }
            let removed = vj.decks.remove(index);
            let gone: HashSet<&str> = removed.columns.iter().map(|c| c.id.as_str()).collect();
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| !clip.column.as_deref().is_some_and(|c| gone.contains(c)));
            }
            if vj.deck == deck {
                vj.deck = vj.decks[index.min(vj.decks.len() - 1)].id.clone();
            }
        }
        VjOp::MoveDeck { deck, to } => {
            let index = vj.deck_index(&deck)?;
            let moved = vj.decks.remove(index);
            vj.decks.insert(to.min(vj.decks.len()), moved);
        }
        VjOp::RenameDeck { deck, name } => {
            let index = vj.deck_index(&deck)?;
            vj.decks[index].name = typed_name(&name)?;
        }
        VjOp::SelectDeck { deck } => {
            vj.deck_index(&deck)?;
            vj.deck = deck;
        }
        VjOp::AddColumn => {
            let index = vj.deck_index(&vj.deck.clone())?;
            let deck = &mut vj.decks[index];
            if deck.columns.len() >= MAX_COLUMNS {
                return Err(ProjectError::Invalid(format!(
                    "a deck has at most {MAX_COLUMNS} columns"
                )));
            }
            deck.columns.push(Column {
                id: new_id(),
                name: None,
            });
        }
        VjOp::RemoveColumn { column } => {
            let (deck, at) = vj.column_index(&column)?;
            if vj.decks[deck].columns.len() == 1 {
                return Err(ProjectError::Invalid("a deck needs a column".into()));
            }
            vj.decks[deck].columns.remove(at);
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| clip.column.as_deref() != Some(&column));
            }
        }
        VjOp::MoveColumn { column, to } => {
            let (deck, at) = vj.column_index(&column)?;
            let columns = &mut vj.decks[deck].columns;
            let moved = columns.remove(at);
            columns.insert(to.min(columns.len()), moved);
        }
        VjOp::RenameColumn { column, name } => {
            let (deck, at) = vj.column_index(&column)?;
            vj.decks[deck].columns[at].name = typed_name(&name)?;
        }
        VjOp::PlaceVideo {
            asset,
            layer,
            column,
        } => {
            if !project.videos.iter().any(|video| video.id == asset) {
                return Err(ProjectError::MissingAsset(asset));
            }
            let cell = vj.cell_of(&layer, &column)?;
            let layer = vj.layer_mut(&layer)?;
            layer
                .clips
                .retain(|clip| clip.cell().as_ref() != Some(&cell));
            let mut clip = Clip {
                id: new_id(),
                column: None,
                slot: None,
                asset,
                name: None,
                playback: Playback::Loop,
                trigger: Trigger::Normal,
                fit: VideoFit::Fit,
                scale: 1.0,
                position: [0.0, 0.0],
            };
            clip.put(cell);
            layer.clips.push(clip);
        }
        VjOp::MoveClip {
            clip,
            layer,
            column,
        } => {
            let to = vj.cell_of(&layer, &column)?;
            let (from_layer, from) = vj
                .layers
                .iter()
                .find_map(|l| {
                    let c = l.clips.iter().find(|c| c.id == clip)?;
                    Some((l.id.clone(), c.cell()?))
                })
                .ok_or_else(|| ProjectError::Invalid(format!("no clip {clip}")))?;
            let moved = take_clip(vj, &from_layer, |c| c.id == clip);
            let occupant = take_clip(vj, &layer, |c| c.cell().as_ref() == Some(&to));
            if let Some(mut occupant) = occupant {
                occupant.put(from);
                vj.layer_mut(&from_layer)?.clips.push(occupant);
            }
            if let Some(mut moved) = moved {
                moved.put(to);
                vj.layer_mut(&layer)?.clips.push(moved);
            }
        }
        VjOp::RemoveClip { clip } => {
            vj.clip_mut(&clip)?;
            for layer in &mut vj.layers {
                layer.clips.retain(|c| c.id != clip);
            }
        }
        VjOp::RenameClip { clip, name } => vj.clip_mut(&clip)?.name = typed_name(&name)?,
        VjOp::ClipPlayback { clip, playback } => vj.clip_mut(&clip)?.playback = playback,
        VjOp::ClipTrigger { clip, trigger } => vj.clip_mut(&clip)?.trigger = trigger,
        VjOp::ClipFit { clip, fit } => vj.clip_mut(&clip)?.fit = fit,
        VjOp::ClipScale { clip, value } => {
            vj.clip_mut(&clip)?.scale = within(value, 0.0, MAX_SCALE)?;
        }
        VjOp::ClipPosition { clip, x, y } => {
            let offset = |v| within(v, -MAX_OFFSET, MAX_OFFSET);
            vj.clip_mut(&clip)?.position = [offset(x)?, offset(y)?];
        }
    }
    Ok(())
}

fn layer_index(vj: &Vj, id: &str) -> Result<usize> {
    vj.layers
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
}

fn take_clip(vj: &mut Vj, layer: &str, which: impl Fn(&Clip) -> bool) -> Option<Clip> {
    let layer = vj.layers.iter_mut().find(|l| l.id == layer)?;
    let index = layer.clips.iter().position(which)?;
    Some(layer.clips.remove(index))
}

/// A typed name, or `None` for an empty one (the default name).
fn typed_name(name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.chars().count() > MAX_CLIP_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name has at most {MAX_CLIP_NAME} characters"
        )));
    }
    Ok((!name.is_empty()).then(|| name.to_owned()))
}

/// Checks the grid: unique ids (a clip's is unique among every item of the project, in `item_ids`),
/// a shown deck, each clip in a known column (a slot on a global layer), at most one clip per
/// cell, clips showing known videos, levels in range, a grid in a VJ project and none in a timeline
/// project.
pub(crate) fn validate<'a>(
    project: &'a Project,
    video_ids: &HashSet<&str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    let vj = &project.vj;
    match project.mode {
        ProjectMode::Vj
            if vj.layers.is_empty()
                || vj.decks.is_empty()
                || vj.decks.iter().any(|deck| deck.columns.is_empty()) =>
        {
            return Err(ProjectError::Invalid(
                "a VJ project needs a layer and a deck with a column".into(),
            ));
        }
        ProjectMode::Vj if vj.shown_deck().is_none() => {
            return Err(ProjectError::Invalid(format!("no deck {}", vj.deck)));
        }
        ProjectMode::Timeline if !vj.is_empty() => {
            return Err(ProjectError::Invalid(
                "a timeline project has no VJ grid".into(),
            ));
        }
        _ => {}
    }
    if vj.layers.len() > MAX_LAYERS
        || vj.decks.len() > MAX_DECKS
        || vj.decks.iter().any(|deck| deck.columns.len() > MAX_COLUMNS)
    {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_LAYERS} layers and {MAX_DECKS} decks of {MAX_COLUMNS} columns"
        )));
    }
    let mut ids = HashSet::new();
    let mut columns = HashSet::new();
    for deck in &vj.decks {
        unique(&mut ids, &deck.id)?;
        check_name(deck.name.as_deref())?;
        for column in &deck.columns {
            unique(&mut ids, &column.id)?;
            columns.insert(column.id.as_str());
            check_name(column.name.as_deref())?;
        }
    }
    for layer in &vj.layers {
        unique(&mut ids, &layer.id)?;
        check_name(layer.name.as_deref())?;
        let levels = [layer.master, layer.opacity, layer.volume];
        if levels.iter().any(|v| !(0.0..=1.0).contains(v))
            || !(0.0..=MAX_TRANSITION).contains(&layer.transition)
        {
            return Err(ProjectError::Invalid(format!(
                "layer {} needs levels of 0 to 1 and a transition of 0 to {MAX_TRANSITION} s",
                layer.id
            )));
        }
        let mut cells = HashSet::new();
        for clip in &layer.clips {
            if !item_ids.insert(&clip.id) {
                return Err(ProjectError::Invalid(format!(
                    "duplicate clip id: {}",
                    clip.id
                )));
            }
            let placed = match (clip.cell(), layer.global) {
                (Some(Cell::Column(column)), false) => columns.contains(column.as_str()),
                (Some(Cell::Slot(slot)), true) => slot < MAX_COLUMNS,
                _ => false,
            };
            if !placed {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is in no cell of its layer",
                    clip.id
                )));
            }
            if !cells.insert(clip.cell()) {
                return Err(ProjectError::Invalid(format!(
                    "layer {} has two clips in one cell",
                    layer.id
                )));
            }
            if !video_ids.contains(clip.asset.as_str()) {
                return Err(ProjectError::MissingAsset(clip.asset.clone()));
            }
            check_name(clip.name.as_deref())?;
        }
    }
    Ok(())
}

fn unique<'a>(seen: &mut HashSet<&'a str>, id: &'a str) -> Result<()> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate grid id: {id}")))
    }
}

fn check_name(name: Option<&str>) -> Result<()> {
    match name {
        Some(name) if name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME => {
            Err(ProjectError::Invalid(format!(
                "a grid name has 1 to {MAX_CLIP_NAME} characters; leave it out instead"
            )))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::video_project;

    fn ids() -> impl FnMut() -> String {
        let mut next = 0;
        move || {
            next += 1;
            format!("g{next}")
        }
    }

    /// A VJ project with the starting grid (layers g1 to g3, deck g4 with columns g5 to g12).
    fn vj_project() -> (Project, impl FnMut() -> String) {
        let mut project = video_project();
        let mut new_id = ids();
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(&mut new_id);
        (project, new_id)
    }

    fn run(project: &mut Project, op: VjOp, new_id: impl FnMut() -> String) {
        apply(project, op, new_id).unwrap();
        project.validate().unwrap();
    }

    fn place(project: &mut Project, layer: &str, column: &str, new_id: impl FnMut() -> String) {
        let op = VjOp::PlaceVideo {
            asset: "v1".into(),
            layer: layer.into(),
            column: column.into(),
        };
        run(project, op, new_id);
    }

    #[test]
    fn starting_grid_should_be_a_valid_vj_project() {
        let (project, _) = vj_project();
        assert_eq!(project.vj.layers.len(), DEFAULT_LAYERS);
        assert_eq!(project.vj.decks.len(), 1);
        assert_eq!(
            project.vj.shown_deck().unwrap().columns.len(),
            DEFAULT_COLUMNS
        );
        project.validate().unwrap();
    }

    #[test]
    fn a_timeline_project_should_have_no_grid() {
        let mut project = video_project();
        assert!(apply(&mut project, VjOp::AddLayer { global: false }, ids()).is_err());
        project.vj = Vj::starting(ids());
        assert!(project.validate().is_err());
    }

    #[test]
    fn a_vj_project_should_need_its_grid_and_a_shown_deck() {
        let (mut project, _) = vj_project();
        project.vj.deck = "nope".into();
        assert!(project.validate().is_err());
        project.vj = Vj::default();
        assert!(project.validate().is_err());
    }

    #[test]
    fn place_video_should_replace_the_clip_of_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        place(&mut project, "g1", "g5", &mut new_id);
        let clips = &project.vj.layers[0].clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "g14");
    }

    #[test]
    fn place_video_should_refuse_an_unknown_video() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::PlaceVideo {
            asset: "nope".into(),
            layer: "g1".into(),
            column: "g5".into(),
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn a_global_layer_should_keep_its_clips_in_slots_across_decks() {
        let (mut project, mut new_id) = vj_project();
        run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
        place(&mut project, "g13", "g7", &mut new_id); // slot 2
        let clip = &project.vj.layers[3].clips[0];
        assert_eq!((clip.column.as_deref(), clip.slot), (None, Some(2)));

        run(&mut project, VjOp::AddDeck, &mut new_id); // g15, columns g16 to g23
        assert_eq!(project.vj.deck, "g15");
        place(&mut project, "g13", "g18", &mut new_id); // slot 2 again, from the other deck
        assert_eq!(project.vj.layers[3].clips.len(), 1);
        assert_eq!(project.vj.column_clips("g18").count(), DEFAULT_LAYERS);
    }

    #[test]
    fn a_clip_should_scale_and_move_within_its_limits_and_keep_them_in_the_file() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        let clip = |project: &Project| project.vj.layers[0].clips[0].clone();
        assert_eq!(
            (clip(&project).scale, clip(&project).position),
            (1.0, [0.0, 0.0])
        );
        let scale = |value| VjOp::ClipScale {
            clip: "g13".into(),
            value,
        };
        run(&mut project, scale(50.0), &mut new_id);
        assert_eq!(clip(&project).scale, MAX_SCALE);
        run(&mut project, scale(0.5), &mut new_id);
        let op = VjOp::ClipPosition {
            clip: "g13".into(),
            x: 0.25,
            y: -9.0,
        };
        run(&mut project, op, &mut new_id);
        assert_eq!(clip(&project).position, [0.25, -MAX_OFFSET]);
        assert!(apply(&mut project, scale(f32::NAN), &mut new_id).is_err());

        let json = serde_json::to_value(clip(&project)).unwrap();
        assert_eq!(
            (json["scale"].as_f64(), json["position"][0].as_f64()),
            (Some(0.5), Some(0.25))
        );
        let back: Clip = serde_json::from_value(json).unwrap();
        assert_eq!(back, clip(&project));
    }

    #[test]
    fn move_clip_should_swap_with_the_clip_already_in_the_cell() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        place(&mut project, "g2", "g6", &mut new_id); // g14
        let op = VjOp::MoveClip {
            clip: "g13".into(),
            layer: "g2".into(),
            column: "g6".into(),
        };
        run(&mut project, op, &mut new_id);
        let (layer, clip) = project.vj.find_clip("g13").unwrap();
        assert_eq!(
            (layer.id.as_str(), clip.column.as_deref()),
            ("g2", Some("g6"))
        );
        let (layer, clip) = project.vj.find_clip("g14").unwrap();
        assert_eq!(
            (layer.id.as_str(), clip.column.as_deref()),
            ("g1", Some("g5"))
        );
    }

    #[test]
    fn move_clip_should_turn_a_column_into_a_slot_on_a_global_layer() {
        let (mut project, mut new_id) = vj_project();
        run(&mut project, VjOp::AddLayer { global: true }, &mut new_id); // g13
        place(&mut project, "g1", "g8", &mut new_id); // g14
        let op = VjOp::MoveClip {
            clip: "g14".into(),
            layer: "g13".into(),
            column: "g9".into(),
        };
        run(&mut project, op, &mut new_id);
        let (layer, clip) = project.vj.find_clip("g14").unwrap();
        assert_eq!((layer.id.as_str(), clip.slot), ("g13", Some(4)));
    }

    #[test]
    fn remove_deck_should_take_its_clips_and_show_another_deck() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id); // g13
        run(&mut project, VjOp::AddDeck, &mut new_id); // g14
        run(
            &mut project,
            VjOp::SelectDeck { deck: "g4".into() },
            &mut new_id,
        );
        run(
            &mut project,
            VjOp::RemoveDeck { deck: "g4".into() },
            &mut new_id,
        );
        assert_eq!(project.vj.deck, "g14");
        assert!(project.vj.layers[0].clips.is_empty());
        let op = VjOp::RemoveDeck { deck: "g14".into() };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn remove_column_should_take_its_clips_and_keep_the_last_column() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        run(
            &mut project,
            VjOp::RemoveColumn {
                column: "g5".into(),
            },
            &mut new_id,
        );
        assert!(project.vj.layers[0].clips.is_empty());
        project.vj.decks[0].columns.truncate(1);
        let last = project.vj.decks[0].columns[0].id.clone();
        let op = VjOp::RemoveColumn { column: last };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn move_layer_should_clamp_the_place() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::MoveLayer {
            layer: "g1".into(),
            to: 99,
        };
        run(&mut project, op, &mut new_id);
        let order: Vec<_> = project.vj.layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(order, ["g2", "g3", "g1"]);
    }

    #[test]
    fn rename_should_take_an_empty_name_back_to_the_default() {
        let (mut project, mut new_id) = vj_project();
        for (name, expected) in [(" Intro ", Some("Intro")), ("  ", None)] {
            let op = VjOp::RenameLayer {
                layer: "g1".into(),
                name: name.into(),
            };
            run(&mut project, op, &mut new_id);
            assert_eq!(project.vj.layers[0].name.as_deref(), expected);
        }
    }

    #[test]
    fn levels_should_be_brought_into_range() {
        let (mut project, mut new_id) = vj_project();
        let op = VjOp::LayerMaster {
            layer: "g1".into(),
            value: 1.5,
        };
        run(&mut project, op, &mut new_id);
        let op = VjOp::LayerTransition {
            layer: "g1".into(),
            value: 30.0,
        };
        run(&mut project, op, &mut new_id);
        let layer = &project.vj.layers[0];
        assert_eq!((layer.master, layer.transition), (1.0, MAX_TRANSITION));
        let op = VjOp::LayerVolume {
            layer: "g1".into(),
            value: f32::NAN,
        };
        assert!(apply(&mut project, op, &mut new_id).is_err());
    }

    #[test]
    fn validate_should_refuse_levels_out_of_range() {
        let (mut project, _) = vj_project();
        project.vj.layers[0].opacity = 1.5;
        assert!(project.validate().is_err());
        project.vj.layers[0].opacity = 0.5;
        project.vj.layers[0].transition = -1.0;
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_refuse_a_slot_on_a_deck_layer() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        project.vj.layers[0].clips[0].put(Cell::Slot(0));
        assert!(project.validate().is_err());
    }

    #[test]
    fn validate_should_refuse_a_clip_id_used_by_a_timeline_clip() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        project.vj.layers[0].clips[0].id = "cv".into();
        assert!(project.validate().is_err());
    }

    #[test]
    fn json_should_leave_out_a_timeline_project_without_a_grid() {
        let value = serde_json::to_value(video_project()).unwrap();
        assert!(value.get("mode").is_none());
        assert!(value.get("vj").is_none());
    }

    #[test]
    fn json_should_round_trip_a_grid() {
        let (mut project, mut new_id) = vj_project();
        place(&mut project, "g1", "g5", &mut new_id);
        let layer = &mut project.vj.layers[0];
        layer.crossfade = Some(Side::B);
        layer.blend = Blend::Screen;
        layer.transition = 1.5;
        layer.clips[0].playback = Playback::PingPong;
        layer.clips[0].trigger = Trigger::Piano;
        let json = serde_json::to_string(&project).unwrap();
        assert!(json.contains(r#""mode":"vj""#));
        assert!(json.contains(r#""trigger":"piano""#));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back, project);
    }

    #[test]
    fn used_assets_should_count_the_clips_of_the_grid() {
        let (mut project, mut new_id) = vj_project();
        project.timelines[0].tracks.clear();
        place(&mut project, "g1", "g5", &mut new_id);
        assert!(project.used_assets().contains("v1"));
    }
}
