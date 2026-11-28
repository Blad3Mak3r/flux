//! What an effect is added to, and the checks of a whole stack.

use serde::Deserialize;

use super::*;
use crate::{ProjectError, Result};

/// What an effect is added to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectTarget {
    /// The clip with this id.
    Clip(String),
    /// The layer with this id.
    Layer(String),
    /// The group with this id: its effects apply to the picture the group's layers make together.
    Group(String),
    /// The composition: the final mix, before the output mapping.
    Composition,
}

/// Checks one stack: at most [`MAX_EFFECTS`], each valid and with an id no other effect has
/// (`seen` holds those met so far). The kinds that are for the composition only are refused unless
/// this is its stack (`composition`).
pub(crate) fn validate_stack<'a>(
    stack: &'a [Effect],
    composition: bool,
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
        if effect.kind.composition_only() && !composition {
            return Err(ProjectError::Invalid(format!(
                "a {} effect only works on the composition",
                effect.kind.name()
            )));
        }
        effect.kind.validate()?;
        for (param, modulation) in &effect.modulation {
            let (lo, hi) = effect.kind.modulatable_range(param)?;
            modulation.validate(lo, hi)?;
        }
    }
    Ok(())
}
