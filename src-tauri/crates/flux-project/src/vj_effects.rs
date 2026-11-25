//! The effects of a VJ grid: an ordered stack on each clip and on each layer, applied to the
//! clip's picture. A clip's stack goes first and then its layer's, each from the top down; a
//! bypassed effect is skipped. The screens and the editor's monitor draw them (`flux-render`).

use serde::{Deserialize, Serialize};

use crate::generator::rgb;
use crate::{ProjectError, Result};

/// Most effects one stack holds.
pub const MAX_EFFECTS: usize = 8;
/// The most and the least a zoom draws the picture larger.
pub const MAX_ZOOM: f32 = 8.0;
pub const MIN_ZOOM: f32 = 0.25;

/// One effect of a stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    /// Unique among every effect of the project.
    pub id: String,
    /// Skipped while drawing; the effect keeps its place and settings.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    #[serde(flatten)]
    pub kind: EffectKind,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Which side of the picture an effect mirrors onto the other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MirrorAxis {
    /// The left half onto the right.
    #[default]
    Horizontal,
    /// The top half onto the bottom.
    Vertical,
    /// Both: the top left quarter onto the other three.
    Both,
}

/// What an effect does, with its parameters. Colour effects work on the light of the picture (see
/// `flux-render`); mirror, zoom and shift move it; blur softens it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "camelCase")]
pub enum EffectKind {
    /// Adds `amount` to every colour, -1 to 1; 0 is none.
    Brightness { amount: f32 },
    /// Pushes the colours away from (above 1) or towards (below 1) mid grey, 0 to 4; 1 is none.
    Contrast { amount: f32 },
    /// Pushes the colours away from (above 1) or towards (below 1) their grey, 0 to 4; 1 is none.
    Saturation { amount: f32 },
    /// Turns the colours about the grey axis, -180 to 180 degrees; 0 is none.
    Hue { degrees: f32 },
    /// Mixes the picture's grey with `colour` (`#rrggbb`), `amount` 0 to 1 of the way.
    Tint { colour: String, amount: f32 },
    /// Mixes the colours with their negative, `amount` 0 to 1 of the way.
    Invert { amount: f32 },
    /// Softens the picture over a radius of `radius` 0 to 1 of the largest blur.
    Blur { radius: f32 },
    /// Folds the picture onto itself.
    Mirror { axis: MirrorAxis },
    /// Draws the picture `amount` times larger about its centre, 0.25 to 8; outside it is
    /// transparent.
    Zoom { amount: f32 },
    /// Moves the picture by `x` and `y` of its size, right and down, -1 to 1; what leaves one side
    /// comes back on the other.
    Shift { x: f32, y: f32 },
}

impl EffectKind {
    /// The names of the kinds, in the order the user is offered them.
    pub const NAMES: [&'static str; 10] = [
        "brightness",
        "contrast",
        "saturation",
        "hue",
        "tint",
        "invert",
        "blur",
        "mirror",
        "zoom",
        "shift",
    ];

    /// The kind named `name` with the settings it starts with, which change nothing of the picture
    /// where they can (a tint starts half way to orange, a blur at a tenth).
    pub fn default_of(name: &str) -> Result<Self> {
        Ok(match name {
            "brightness" => Self::Brightness { amount: 0.0 },
            "contrast" => Self::Contrast { amount: 1.0 },
            "saturation" => Self::Saturation { amount: 1.0 },
            "hue" => Self::Hue { degrees: 0.0 },
            "tint" => Self::Tint {
                colour: "#ff8800".into(),
                amount: 0.5,
            },
            "invert" => Self::Invert { amount: 1.0 },
            "blur" => Self::Blur { radius: 0.1 },
            "mirror" => Self::Mirror {
                axis: MirrorAxis::Horizontal,
            },
            "zoom" => Self::Zoom { amount: 1.0 },
            "shift" => Self::Shift { x: 0.0, y: 0.0 },
            _ => {
                return Err(ProjectError::Invalid(format!("no effect called {name}")));
            }
        })
    }

    /// Its name, as in [`Self::NAMES`].
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brightness { .. } => "brightness",
            Self::Contrast { .. } => "contrast",
            Self::Saturation { .. } => "saturation",
            Self::Hue { .. } => "hue",
            Self::Tint { .. } => "tint",
            Self::Invert { .. } => "invert",
            Self::Blur { .. } => "blur",
            Self::Mirror { .. } => "mirror",
            Self::Zoom { .. } => "zoom",
            Self::Shift { .. } => "shift",
        }
    }

    /// Whether `other` is the same kind of effect, possibly with other settings.
    pub fn same_kind(&self, other: &Self) -> bool {
        self.name() == other.name()
    }

    /// The kind with its settings brought into range; a setting that is not a number or a colour
    /// that is not `#rrggbb` is refused.
    pub fn tidy(mut self) -> Result<Self> {
        let number = |v: &mut f32, min: f32, max: f32| {
            if v.is_finite() {
                *v = v.clamp(min, max);
                Ok(())
            } else {
                Err(ProjectError::Invalid(
                    "an effect setting is a number".into(),
                ))
            }
        };
        match &mut self {
            Self::Brightness { amount } => number(amount, -1.0, 1.0)?,
            Self::Contrast { amount } | Self::Saturation { amount } => number(amount, 0.0, 4.0)?,
            Self::Hue { degrees } => number(degrees, -180.0, 180.0)?,
            Self::Tint { colour, amount } => {
                number(amount, 0.0, 1.0)?;
                if rgb(colour).is_none() {
                    return Err(bad_colour(colour));
                }
            }
            Self::Invert { amount } => number(amount, 0.0, 1.0)?,
            Self::Blur { radius } => number(radius, 0.0, 1.0)?,
            Self::Mirror { .. } => {}
            Self::Zoom { amount } => number(amount, MIN_ZOOM, MAX_ZOOM)?,
            Self::Shift { x, y } => {
                number(x, -1.0, 1.0)?;
                number(y, -1.0, 1.0)?;
            }
        }
        Ok(self)
    }

    /// Checks the settings are in range, as [`Self::tidy`] would bring them.
    pub(crate) fn validate(&self) -> Result<()> {
        if self.clone().tidy()? == *self {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!(
                "a {} effect has a setting out of range",
                self.name()
            )))
        }
    }
}

fn bad_colour(colour: &str) -> ProjectError {
    ProjectError::Invalid(format!("a tint colour must look like #rrggbb: {colour}"))
}

/// What an effect is added to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectTarget {
    /// The clip with this id.
    Clip(String),
    /// The layer with this id.
    Layer(String),
}

/// Checks one stack: at most [`MAX_EFFECTS`], each valid and with an id no other effect has
/// (`seen` holds those met so far).
pub(crate) fn validate_stack<'a>(
    stack: &'a [Effect],
    seen: &mut std::collections::HashSet<&'a str>,
) -> Result<()> {
    if stack.len() > MAX_EFFECTS {
        return Err(ProjectError::Invalid(format!(
            "a stack has at most {MAX_EFFECTS} effects"
        )));
    }
    for effect in stack {
        if !seen.insert(&effect.id) {
            return Err(ProjectError::Invalid(format!(
                "duplicate effect id: {}",
                effect.id
            )));
        }
        effect.kind.validate()?;
    }
    Ok(())
}
