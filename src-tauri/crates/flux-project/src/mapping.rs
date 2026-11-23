//! How the video reaches the screen, like the advanced output of a media server: the picture of
//! the timeline (the composition) is cut into slices, and each slice is a rectangle of it placed on
//! the output by its four corners, which can lean it to fit a surface seen at an angle. No slices
//! is the plain output: the whole picture fitted to the screen.
//!
//! Every number is a fraction of a width or height, from the top left corner, so a mapping keeps
//! its shape on a screen of another resolution.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

/// Most slices an output has.
pub const MAX_SLICES: usize = 64;
/// Longest slice name, in characters.
const MAX_NAME: usize = 80;
/// Smallest side of a slice's rectangle, so it never vanishes.
const MIN_SIDE: f32 = 0.005;
/// How far a corner may go past the screen's edges: a projector's picture can hang off them.
const CORNER_REACH: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
}

/// The corners of the whole screen: top left, top right, bottom right, bottom left.
pub const FULL_CORNERS: [Point; 4] = [
    Point { x: 0.0, y: 0.0 },
    Point { x: 1.0, y: 0.0 },
    Point { x: 1.0, y: 1.0 },
    Point { x: 0.0, y: 1.0 },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub id: String,
    pub name: String,
    /// The part of the composition it shows.
    pub input: Rect,
    /// Where that part's top left, top right, bottom right and bottom left corners go on the
    /// output.
    pub corners: [Point; 4],
    /// Left out of the output without losing its place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MappingOp {
    /// A new slice of the whole composition on the whole screen, named "Slice n".
    Add,
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    SetInput {
        id: String,
        input: Rect,
    },
    SetCorners {
        id: String,
        corners: [Point; 4],
    },
    /// Sets one side of the rectangle to a typed number of pixels of a picture of `size`.
    SetInputValue {
        id: String,
        field: RectField,
        text: String,
        size: Size,
    },
    /// Sets one coordinate of a corner to a typed number of pixels of a screen of `size`.
    SetCornerValue {
        id: String,
        corner: usize,
        axis: Axis,
        text: String,
        size: Size,
    },
    /// Puts the slice's corners back on the corners of its rectangle, as the screen's.
    ResetCorners {
        id: String,
    },
    SetHidden {
        id: String,
        hidden: bool,
    },
    /// Takes every slice away: the output shows the whole picture again.
    Clear,
}

/// A picture's or a screen's size in pixels, to turn typed pixels into fractions.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RectField {
    X,
    Y,
    Width,
    Height,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    X,
    Y,
}

/// A typed number of pixels as a fraction of `of` pixels.
fn fraction(text: &str, of: f32) -> Result<f32> {
    let pixels = text
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| ProjectError::Invalid("not a number of pixels".into()))?;
    if !(of.is_finite() && of >= 1.0) {
        return Err(ProjectError::Invalid("no size to measure pixels in".into()));
    }
    Ok(pixels / of)
}

/// Applies `op` to `slices`; `new_id` names a new slice. The caller validates the result.
pub fn apply(
    slices: &mut Vec<Slice>,
    op: MappingOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    let find = |slices: &mut Vec<Slice>, id: &str| -> Result<usize> {
        slices
            .iter()
            .position(|slice| slice.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no slice {id}")))
    };
    match op {
        MappingOp::Add => {
            let number = (1..)
                .find(|n| !slices.iter().any(|s| s.name == format!("Slice {n}")))
                .unwrap_or(1);
            slices.push(Slice {
                id: new_id(),
                name: format!("Slice {number}"),
                input: Rect::FULL,
                corners: FULL_CORNERS,
                hidden: false,
            });
        }
        MappingOp::Remove { id } => {
            let at = find(slices, &id)?;
            slices.remove(at);
        }
        MappingOp::Rename { id, name } => {
            let at = find(slices, &id)?;
            slices[at].name = name.trim().to_owned();
        }
        MappingOp::SetInput { id, input } => {
            let at = find(slices, &id)?;
            slices[at].input = clamp_rect(input);
        }
        MappingOp::SetCorners { id, corners } => {
            let at = find(slices, &id)?;
            slices[at].corners = corners.map(|p| Point {
                x: p.x.clamp(-CORNER_REACH, 1.0 + CORNER_REACH),
                y: p.y.clamp(-CORNER_REACH, 1.0 + CORNER_REACH),
            });
        }
        MappingOp::SetInputValue {
            id,
            field,
            text,
            size,
        } => {
            let at = find(slices, &id)?;
            let mut r = slices[at].input;
            // A side that would run past the picture is shortened, and a position moves the
            // rectangle without changing its size, as far as the picture allows.
            match field {
                RectField::X => r.x = fraction(&text, size.width)?.clamp(0.0, 1.0 - r.width),
                RectField::Y => r.y = fraction(&text, size.height)?.clamp(0.0, 1.0 - r.height),
                RectField::Width => {
                    r.width = fraction(&text, size.width)?.clamp(MIN_SIDE, 1.0 - r.x)
                }
                RectField::Height => {
                    r.height = fraction(&text, size.height)?.clamp(MIN_SIDE, 1.0 - r.y)
                }
            }
            slices[at].input = clamp_rect(r);
        }
        MappingOp::SetCornerValue {
            id,
            corner,
            axis,
            text,
            size,
        } => {
            let at = find(slices, &id)?;
            let point = slices[at]
                .corners
                .get_mut(corner)
                .ok_or_else(|| ProjectError::Invalid(format!("no corner {corner}")))?;
            let reach = -CORNER_REACH..=1.0 + CORNER_REACH;
            match axis {
                Axis::X => {
                    point.x = fraction(&text, size.width)?.clamp(*reach.start(), *reach.end())
                }
                Axis::Y => {
                    point.y = fraction(&text, size.height)?.clamp(*reach.start(), *reach.end())
                }
            }
        }
        MappingOp::ResetCorners { id } => {
            let at = find(slices, &id)?;
            let Rect {
                x,
                y,
                width,
                height,
            } = slices[at].input;
            slices[at].corners = [
                Point { x, y },
                Point { x: x + width, y },
                Point {
                    x: x + width,
                    y: y + height,
                },
                Point { x, y: y + height },
            ];
        }
        MappingOp::SetHidden { id, hidden } => {
            let at = find(slices, &id)?;
            slices[at].hidden = hidden;
        }
        MappingOp::Clear => slices.clear(),
    }
    Ok(())
}

/// Keeps a rectangle inside the composition and at least [`MIN_SIDE`] on each side.
fn clamp_rect(rect: Rect) -> Rect {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let width = finite(rect.width, 1.0).clamp(MIN_SIDE, 1.0);
    let height = finite(rect.height, 1.0).clamp(MIN_SIDE, 1.0);
    Rect {
        x: finite(rect.x, 0.0).clamp(0.0, 1.0 - width),
        y: finite(rect.y, 0.0).clamp(0.0, 1.0 - height),
        width,
        height,
    }
}

pub fn validate(slices: &[Slice]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video mapping: {why}")));
    if slices.len() > MAX_SLICES {
        return invalid("too many slices");
    }
    let mut ids = std::collections::HashSet::new();
    for slice in slices {
        if slice.id.is_empty() || !ids.insert(slice.id.as_str()) {
            return invalid("a slice id is empty or repeated");
        }
        if slice.name.is_empty() || slice.name.chars().count() > MAX_NAME {
            return invalid("a slice name is empty or too long");
        }
        let r = slice.input;
        let inside = |v: f32| (0.0..=1.0).contains(&v);
        if !(inside(r.x)
            && inside(r.y)
            && r.width >= MIN_SIDE
            && r.height >= MIN_SIDE
            && r.x + r.width <= 1.0 + f32::EPSILON
            && r.y + r.height <= 1.0 + f32::EPSILON)
        {
            return invalid("a slice's rectangle is outside the picture");
        }
        let reach = -CORNER_REACH..=1.0 + CORNER_REACH;
        if !slice
            .corners
            .iter()
            .all(|p| reach.contains(&p.x) && reach.contains(&p.y))
        {
            return invalid("a slice's corner is too far off the screen");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn added() -> Vec<Slice> {
        let mut slices = Vec::new();
        apply(&mut slices, MappingOp::Add, || "s1".into()).unwrap();
        slices
    }

    #[test]
    fn add_should_show_the_whole_picture_on_the_whole_screen() {
        let slices = added();
        assert_eq!(slices[0].name, "Slice 1");
        assert_eq!(
            (slices[0].input, slices[0].corners),
            (Rect::FULL, FULL_CORNERS)
        );
        let mut more = slices.clone();
        apply(&mut more, MappingOp::Add, || "s2".into()).unwrap();
        assert_eq!(more[1].name, "Slice 2");
        assert!(validate(&more).is_ok());
    }

    #[test]
    fn set_input_should_keep_the_rectangle_inside_the_picture() {
        let mut slices = added();
        let input = Rect {
            x: 0.8,
            y: -0.2,
            width: 0.5,
            height: 0.0,
        };
        apply(
            &mut slices,
            MappingOp::SetInput {
                id: "s1".into(),
                input,
            },
            String::new,
        )
        .unwrap();
        let r = slices[0].input;
        assert_eq!((r.x, r.y, r.width, r.height), (0.5, 0.0, 0.5, MIN_SIDE));
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn reset_corners_should_put_the_slice_where_its_rectangle_is() {
        let mut slices = added();
        let input = Rect {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        apply(
            &mut slices,
            MappingOp::SetInput {
                id: "s1".into(),
                input,
            },
            String::new,
        )
        .unwrap();
        apply(
            &mut slices,
            MappingOp::ResetCorners { id: "s1".into() },
            String::new,
        )
        .unwrap();
        assert_eq!(slices[0].corners[0], Point { x: 0.5, y: 0.0 });
        assert_eq!(slices[0].corners[2], Point { x: 1.0, y: 1.0 });
    }

    #[test]
    fn corners_should_stop_a_screen_away_from_the_edges() {
        let mut slices = added();
        let far = [Point { x: -9.0, y: 0.0 }; 4];
        apply(
            &mut slices,
            MappingOp::SetCorners {
                id: "s1".into(),
                corners: far,
            },
            String::new,
        )
        .unwrap();
        assert_eq!(slices[0].corners[0].x, -1.0);
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn typed_pixels_should_set_the_rectangle_and_the_corners() {
        let mut slices = added();
        let size = Size {
            width: 1920.0,
            height: 1080.0,
        };
        let input = |field, text: &str| MappingOp::SetInputValue {
            id: "s1".into(),
            field,
            text: text.into(),
            size,
        };
        apply(&mut slices, input(RectField::Width, "960"), String::new).unwrap();
        apply(&mut slices, input(RectField::X, " 1500 "), String::new).unwrap();
        let r = slices[0].input;
        // The rectangle stops at the right edge rather than shrinking.
        assert_eq!((r.x, r.width), (0.5, 0.5));
        assert!(apply(&mut slices, input(RectField::Y, "abc"), String::new).is_err());
        let corner = MappingOp::SetCornerValue {
            id: "s1".into(),
            corner: 2,
            axis: Axis::Y,
            text: "540".into(),
            size,
        };
        apply(&mut slices, corner, String::new).unwrap();
        assert_eq!(slices[0].corners[2], Point { x: 1.0, y: 0.5 });
        let missing = MappingOp::SetCornerValue {
            id: "s1".into(),
            corner: 4,
            axis: Axis::X,
            text: "0".into(),
            size,
        };
        assert!(apply(&mut slices, missing, String::new).is_err());
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn validate_should_refuse_a_repeated_id_or_an_unknown_slice() {
        let mut slices = added();
        slices.push(slices[0].clone());
        assert!(validate(&slices).is_err());
        let mut one = added();
        let op = MappingOp::Remove { id: "nope".into() };
        assert!(apply(&mut one, op, String::new).is_err());
    }
}
