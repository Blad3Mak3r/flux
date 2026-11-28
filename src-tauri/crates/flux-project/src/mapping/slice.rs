//! Slices: their rectangles, corners and soft edges, the composition they cut, and their validation.

use serde::{Deserialize, Serialize};

use super::*;
use crate::{ProjectError, Result};

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
    #[serde(default, skip_serializing_if = "SoftEdge::is_hard")]
    pub soft_edge: SoftEdge,
    /// How the output's test card is drawn on this slice.
    #[serde(default)]
    pub test_card: TestCard,
}

/// How far in from each edge a slice fades to black, each a fraction (0 to 1) of its width or
/// height, and the fade's curve: 1 fades the light evenly, more keeps it brighter for longer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoftEdge {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub gamma: f32,
}

impl Default for SoftEdge {
    fn default() -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            gamma: 1.0,
        }
    }
}

impl SoftEdge {
    pub fn is_hard(&self) -> bool {
        *self == Self::default()
    }
}

/// The curve's range: from a quick fade to a slow one.
pub(super) const GAMMA: std::ops::RangeInclusive<f32> = 0.2..=5.0;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeField {
    Left,
    Right,
    Top,
    Bottom,
    Gamma,
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
        let e = slice.soft_edge;
        let fraction = |v: f32| (0.0..=1.0).contains(&v);
        if !(fraction(e.left)
            && fraction(e.right)
            && fraction(e.top)
            && fraction(e.bottom)
            && GAMMA.contains(&e.gamma))
        {
            return invalid("a slice's soft edge is out of range");
        }
        slice.test_card.validate()?;
    }
    Ok(())
}

/// The size of the composition, the picture every clip is sized into and the slices cut, in
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composition {
    pub width: u32,
    pub height: u32,
}

/// Sides a composition may have, in pixels.
pub const COMPOSITION_SIDES: std::ops::RangeInclusive<u32> = 16..=8192;

impl Composition {
    /// The size typed as two numbers of pixels.
    pub fn parse(width: &str, height: &str) -> Result<Self> {
        let side = |text: &str| {
            text.trim()
                .parse::<u32>()
                .ok()
                .filter(|v| COMPOSITION_SIDES.contains(v))
                .ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "the composition's sides are whole pixels from {} to {}",
                        COMPOSITION_SIDES.start(),
                        COMPOSITION_SIDES.end()
                    ))
                })
        };
        Ok(Self {
            width: side(width)?,
            height: side(height)?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        Self::parse(&self.width.to_string(), &self.height.to_string()).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_should_take_whole_pixels_within_its_range() {
        assert_eq!(
            Composition::parse(" 3840", "1080 ").unwrap(),
            Composition {
                width: 3840,
                height: 1080
            }
        );
        for (w, h) in [
            ("15", "1080"),
            ("1920", "8193"),
            ("19.5", "1080"),
            ("", "1080"),
        ] {
            assert!(Composition::parse(w, h).is_err(), "{w} x {h}");
        }
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
