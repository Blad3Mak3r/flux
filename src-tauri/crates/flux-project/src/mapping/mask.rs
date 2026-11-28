//! Masks: shapes of a screen blacked out, or everything but them.

use serde::{Deserialize, Serialize};

use super::*;
use crate::{ProjectError, Result};

/// A shape on a screen that hides the picture inside it, or outside it when `invert`: a pillar,
/// a door, or everything around an odd-shaped surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mask {
    pub id: String,
    pub name: String,
    /// The outline, in order, as fractions of the screen from its top left corner.
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MaskOp {
    /// A square in the middle of the screen, named "Mask n".
    Add,
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    /// Moves the points; there must be as many as before.
    SetPoints {
        id: String,
        points: Vec<Point>,
    },
    /// A point halfway between point `after` and the next one.
    AddPoint {
        id: String,
        after: usize,
    },
    /// A mask keeps at least three points.
    RemovePoint {
        id: String,
        index: usize,
    },
    SetInvert {
        id: String,
        invert: bool,
    },
    SetHidden {
        id: String,
        hidden: bool,
    },
}

/// Applies `op` to a screen's `masks`; `new_id` names a new mask. The caller validates the result.
pub fn edit_masks(
    masks: &mut Vec<Mask>,
    op: MaskOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    let find = |masks: &mut Vec<Mask>, id: &str| -> Result<usize> {
        masks
            .iter()
            .position(|mask| mask.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no mask {id}")))
    };
    let inside = |p: Point| Point {
        x: finite_or(p.x, 0.5).clamp(0.0, 1.0),
        y: finite_or(p.y, 0.5).clamp(0.0, 1.0),
    };
    match op {
        MaskOp::Add => {
            if masks.len() >= MAX_MASKS {
                return Err(ProjectError::Invalid("too many masks".into()));
            }
            let number = (1..)
                .find(|n| !masks.iter().any(|m| m.name == format!("Mask {n}")))
                .unwrap_or(1);
            let (a, b) = (0.4, 0.6);
            masks.push(Mask {
                id: new_id(),
                name: format!("Mask {number}"),
                points: vec![
                    Point { x: a, y: a },
                    Point { x: b, y: a },
                    Point { x: b, y: b },
                    Point { x: a, y: b },
                ],
                invert: false,
                hidden: false,
            });
        }
        MaskOp::Remove { id } => {
            let at = find(masks, &id)?;
            masks.remove(at);
        }
        MaskOp::Rename { id, name } => {
            let at = find(masks, &id)?;
            masks[at].name = name.trim().to_owned();
        }
        MaskOp::SetPoints { id, points } => {
            let at = find(masks, &id)?;
            if points.len() != masks[at].points.len() {
                return Err(ProjectError::Invalid(
                    "a mask's points cannot be counted anew".into(),
                ));
            }
            masks[at].points = points.into_iter().map(inside).collect();
        }
        MaskOp::AddPoint { id, after } => {
            let at = find(masks, &id)?;
            let points = &mut masks[at].points;
            if points.len() >= MAX_MASK_POINTS || after >= points.len() {
                return Err(ProjectError::Invalid("no room for another point".into()));
            }
            let (a, b) = (points[after], points[(after + 1) % points.len()]);
            let middle = Point {
                x: (a.x + b.x) / 2.0,
                y: (a.y + b.y) / 2.0,
            };
            points.insert(after + 1, middle);
        }
        MaskOp::RemovePoint { id, index } => {
            let at = find(masks, &id)?;
            let points = &mut masks[at].points;
            if points.len() <= 3 || index >= points.len() {
                return Err(ProjectError::Invalid("a mask keeps three points".into()));
            }
            points.remove(index);
        }
        MaskOp::SetInvert { id, invert } => {
            let at = find(masks, &id)?;
            masks[at].invert = invert;
        }
        MaskOp::SetHidden { id, hidden } => {
            let at = find(masks, &id)?;
            masks[at].hidden = hidden;
        }
    }
    Ok(())
}

pub(super) fn finite_or(v: f32, or: f32) -> f32 {
    if v.is_finite() { v } else { or }
}

pub(super) fn validate_masks(masks: &[Mask]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video masks: {why}")));
    if masks.len() > MAX_MASKS {
        return invalid("too many masks");
    }
    let mut ids = std::collections::HashSet::new();
    for mask in masks {
        if mask.id.is_empty() || !ids.insert(mask.id.as_str()) {
            return invalid("a mask id is empty or repeated");
        }
        if mask.name.is_empty() || mask.name.chars().count() > MAX_NAME {
            return invalid("a mask name is empty or too long");
        }
        let on_screen = |v: f32| (0.0..=1.0).contains(&v);
        if !(3..=MAX_MASK_POINTS).contains(&mask.points.len())
            || !mask.points.iter().all(|p| on_screen(p.x) && on_screen(p.y))
        {
            return invalid("a mask needs 3 to 32 points on the screen");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_should_keep_three_points_on_the_screen() {
        let mut masks = Vec::new();
        edit_masks(&mut masks, MaskOp::Add, || "m".into()).unwrap();
        assert_eq!(
            (masks[0].name.as_str(), masks[0].points.len()),
            ("Mask 1", 4)
        );
        let add = MaskOp::AddPoint {
            id: "m".into(),
            after: 3,
        };
        edit_masks(&mut masks, add, String::new).unwrap();
        // Between the last point and the first: the middle of the left side.
        assert_eq!(masks[0].points[4], Point { x: 0.4, y: 0.5 });
        let mut far = masks[0].points.clone();
        far[0] = Point { x: -3.0, y: 2.0 };
        let set = MaskOp::SetPoints {
            id: "m".into(),
            points: far,
        };
        edit_masks(&mut masks, set, String::new).unwrap();
        assert_eq!(masks[0].points[0], Point { x: 0.0, y: 1.0 });
        let fewer = MaskOp::SetPoints {
            id: "m".into(),
            points: vec![Point { x: 0.0, y: 0.0 }; 3],
        };
        assert!(edit_masks(&mut masks, fewer, String::new).is_err());
        for _ in 0..2 {
            let remove = MaskOp::RemovePoint {
                id: "m".into(),
                index: 0,
            };
            edit_masks(&mut masks, remove, String::new).unwrap();
        }
        let last = MaskOp::RemovePoint {
            id: "m".into(),
            index: 0,
        };
        assert!(edit_masks(&mut masks, last, String::new).is_err());
        assert!(validate_masks(&masks).is_ok());
    }
}
