use super::*;

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
    /// How the clip arriving mixes with the one leaving during a transition, any blend (Resolume's
    /// transition mode); absent is [`Blend::Alpha`], a crossfade.
    #[serde(default, skip_serializing_if = "Blend::is_alpha")]
    pub transition_effect: Blend,
    /// Whether each transition on the layer picks a blend at random, from all of them, instead of
    /// using [`Self::transition_effect`] (Resolume's Random transition); absent is off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub transition_random: bool,
    /// What the layer launches when the clip playing on it ends a pass; absent is [`Autopilot::Off`].
    #[serde(default, skip_serializing_if = "Autopilot::is_off")]
    pub autopilot: Autopilot,
    /// The side of the crossfader the layer is on; absent is neither, so the crossfader leaves it alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crossfade: Option<Side>,
    /// Bypassed: the layer shows nothing and sounds nothing, though its clip keeps playing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Soloed: while any layer is, only the soloed ones show.
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// Folded to a thin row on the grid, as in Resolume; it plays as before.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// The id of the group the layer is in; the layers of a group are next to each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The layer's effects, drawn on each of its clips' pictures after the clip's own; the first
    /// is applied first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// What drives `master`, `opacity` or `volume` by itself (see [`crate::vj_modulation`]), by
    /// field. The stored value is the one the fader stands at.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    /// The layer's dashboard: up to [`crate::vj_mapping::KNOBS`] knobs, each driving the numbers
    /// of the layer it is linked to (see [`crate::vj_dashboard`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboard: Vec<Knob>,
    /// At most one per column of each deck (per slot on a global layer), in no particular order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<Clip>,
}

impl Layer {
    pub(crate) fn new(id: String, global: bool) -> Self {
        Self {
            id,
            name: None,
            global,
            master: 1.0,
            opacity: 1.0,
            volume: 1.0,
            blend: Blend::Alpha,
            transition: 0.0,
            transition_effect: Blend::Alpha,
            transition_random: false,
            autopilot: Autopilot::Off,
            crossfade: None,
            bypass: false,
            solo: false,
            collapsed: false,
            group: None,
            effects: Vec::new(),
            modulation: BTreeMap::new(),
            dashboard: Vec::new(),
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
    /// The distance between the two lights.
    Difference,
    /// Like [`Self::Difference`], with less contrast.
    Exclusion,
    /// Multiplies where what is below is dark, screens where it is light.
    Overlay,
    /// A gentle [`Self::Overlay`]-like contrast, driven by the layer.
    SoftLight,
    /// [`Self::Overlay`] driven by the layer's light.
    HardLight,
    /// Brightens what is below by the layer, to overexposure (Resolume's Dodge).
    ColorDodge,
    /// Darkens what is below by the layer, with more contrast than multiply (Resolume's Burn).
    ColorBurn,
    /// Burns or dodges linearly by the layer's light.
    LinearLight,
    /// Burns or dodges by the layer's light, with more contrast than linear light.
    VividLight,
    /// Keeps the darker or the lighter of the two by the layer's light.
    PinLight,
    /// The layer's hue on the saturation and luminosity below.
    Hue,
    /// The layer's saturation on the hue and luminosity below.
    Saturation,
    /// The layer's hue and saturation on the luminosity below.
    Color,
    /// The layer's luminosity on the hue and saturation below.
    Luminosity,
    /// A hard cut by the layer: where its cover is at least half it replaces what is below, elsewhere it leaves it (Resolume's AlphaCut).
    AlphaCut,
    /// The layer's dark parts turn transparent: its alpha is multiplied by its luma (Resolume's Luma Key).
    LumaKey,
    /// The layer's bright parts turn transparent: its alpha is multiplied by one minus its luma (Resolume's Luma Key I).
    LumaKeyInverted,
    /// The layer's luma replaces its own alpha: bright is opaque, dark is transparent, wherever it has a picture.
    LumaIsAlpha,
    /// The inverse of the distance between the two lights, a lighter picture (Resolume's Difference I).
    DifferenceInverted,
    /// The layer acts as a mask: what is below shows where it is bright and goes dark where it is dark; it is not drawn (Resolume's 50Mask).
    Mask50,
    /// The layer takes over what is below one channel at a time as its cover rises: blue, then green, then red.
    Rgb,
    /// The layer shifts what is below sideways by its luma, none at mid grey; it is not drawn.
    Displace,
}

impl Blend {
    /// Every blend, for the random transition to pick from.
    pub const ALL: [Blend; 29] = [
        Blend::Alpha,
        Blend::Add,
        Blend::Subtract,
        Blend::Multiply,
        Blend::Screen,
        Blend::Lighten,
        Blend::Darken,
        Blend::Difference,
        Blend::Exclusion,
        Blend::Overlay,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::ColorDodge,
        Blend::ColorBurn,
        Blend::LinearLight,
        Blend::VividLight,
        Blend::PinLight,
        Blend::Hue,
        Blend::Saturation,
        Blend::Color,
        Blend::Luminosity,
        Blend::AlphaCut,
        Blend::LumaKey,
        Blend::LumaKeyInverted,
        Blend::LumaIsAlpha,
        Blend::DifferenceInverted,
        Blend::Mask50,
        Blend::Rgb,
        Blend::Displace,
    ];

    pub(crate) fn is_alpha(&self) -> bool {
        *self == Self::Alpha
    }
}

/// Which clip a layer launches by itself when the one playing ends a pass (Resolume's Layer
/// Autopilot): the next or previous one of the layer in the deck shown, or a random one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Autopilot {
    #[default]
    Off,
    Next,
    Previous,
    /// Never the clip that played, when there is another.
    Random,
}

impl Autopilot {
    fn is_off(&self) -> bool {
        *self == Self::Off
    }

    /// The mode after this one, as the cycle button steps: off, next, previous, random, off.
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::Next,
            Self::Next => Self::Previous,
            Self::Previous => Self::Random,
            Self::Random => Self::Off,
        }
    }

    /// The cell to launch after `current` among `filled` (one entry per column, `true` for a clip),
    /// skipping empty cells and wrapping around; `roll` is a random number, used by
    /// [`Self::Random`]. `None` when the layer has no clip in the deck, or autopilot is off. A layer
    /// whose only clip is `current` launches it again.
    pub fn pick(self, filled: &[bool], current: Option<usize>, roll: u64) -> Option<usize> {
        let n = filled.len();
        match self {
            Self::Off => None,
            Self::Next | Self::Previous => {
                let forward = self == Self::Next;
                (1..=n)
                    .map(|k| match (current, forward) {
                        (Some(c), true) => (c + k) % n,
                        (Some(c), false) => (c + n - k % n) % n,
                        (None, true) => k - 1,
                        (None, false) => n - k,
                    })
                    .find(|&at| filled[at])
            }
            Self::Random => {
                let others: Vec<usize> = (0..n)
                    .filter(|&at| filled[at] && Some(at) != current)
                    .collect();
                match others.len() {
                    0 => current.filter(|&at| filled.get(at) == Some(&true)),
                    len => Some(others[(roll % len as u64) as usize]),
                }
            }
        }
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
    pub(super) fn is_normal(&self) -> bool {
        *self == Self::Normal
    }
}
