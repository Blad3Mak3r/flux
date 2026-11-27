//! Layer groups of the VJ grid, as Resolume's: a group holds layers that are next to each other in
//! the drawing order. Its layers are mixed together over a clear picture first, as if they were a
//! composition of their own, and that picture is then mixed with the layers below like a layer's
//! is, by the group's opacity, master and blend, after the group's own effects.
//!
//! Groups do not nest and are never empty. A layer is in at most one, named by [`Layer::group`];
//! [`Vj::groups`] lists them bottom first, which [`Vj::tidy_groups`] keeps true after every edit.
//!
//! Global layers can be in a group like any other: they keep their clips in slots, and a group's
//! column launch leaves them alone as a column's does.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::vj::{Blend, Layer, MAX_LAYERS, Vj, check_name, full, is_false, is_full};
use crate::vj_effects::{Effect, validate_stack};
use crate::{ProjectError, Result};

/// Most groups a grid has: each holds a layer.
pub const MAX_GROUPS: usize = MAX_LAYERS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    /// Absent is the group's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Scales the picture and the sound of every layer of the group, 0 to 1; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub master: f32,
    /// How much the group's picture covers the layers below, 0 to 1; absent is 1.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub opacity: f32,
    /// How the group's picture mixes with the layers below; absent is [`Blend::Alpha`].
    #[serde(default, skip_serializing_if = "Blend::is_alpha")]
    pub blend: Blend,
    /// The group shows and sounds nothing, though its layers keep playing.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    /// Soloed: while any layer or group is, only the soloed ones show; a soloed group shows all
    /// its layers.
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// Folded to its header on the grid.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Drawn on the picture the group's layers make together, the first applied first. Not the
    /// kinds that only work on the composition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
}

impl Group {
    fn new(id: String) -> Self {
        Self {
            id,
            name: None,
            master: 1.0,
            opacity: 1.0,
            blend: Blend::Alpha,
            bypass: false,
            solo: false,
            collapsed: false,
            effects: Vec::new(),
        }
    }
}

impl Vj {
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.id == id)
    }

    pub(crate) fn group_mut(&mut self, id: &str) -> Result<&mut Group> {
        self.groups
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no group {id}")))
    }

    /// The group `layer` is in.
    pub fn group_of(&self, layer: &Layer) -> Option<&Group> {
        self.group(layer.group.as_deref()?)
    }

    /// The layers of `group`, bottom first.
    pub fn group_layers<'a>(&'a self, group: &'a str) -> impl Iterator<Item = &'a Layer> {
        self.layers
            .iter()
            .filter(move |layer| layer.group.as_deref() == Some(group))
    }

    /// Whether any layer or group is soloed.
    pub fn any_solo(&self) -> bool {
        self.layers.iter().any(|layer| layer.solo) || self.groups.iter().any(|group| group.solo)
    }

    /// Whether the solos let `layer` show: nothing is soloed, or it is, or its group is.
    pub fn solo_lets(&self, layer: &Layer) -> bool {
        !self.any_solo() || layer.solo || self.group_of(layer).is_some_and(|group| group.solo)
    }

    /// Brings the groups back to what they must be after an edit: a layer of a group that is gone
    /// is in none, the layers of a group gather where its lowest one is (in their order), a group
    /// without layers goes, and the groups are listed bottom first.
    pub(crate) fn tidy_groups(&mut self) {
        if self.groups.is_empty() && self.layers.iter().all(|layer| layer.group.is_none()) {
            return;
        }
        let known: HashSet<&str> = self.groups.iter().map(|g| g.id.as_str()).collect();
        for layer in &mut self.layers {
            if layer.group.as_deref().is_some_and(|g| !known.contains(g)) {
                layer.group = None;
            }
        }
        let old = std::mem::take(&mut self.layers);
        let mut order = Vec::with_capacity(old.len());
        let mut taken = vec![false; old.len()];
        for at in 0..old.len() {
            if std::mem::replace(&mut taken[at], true) {
                continue;
            }
            order.push(at);
            if let Some(group) = &old[at].group {
                for later in at + 1..old.len() {
                    if !taken[later] && old[later].group.as_ref() == Some(group) {
                        taken[later] = true;
                        order.push(later);
                    }
                }
            }
        }
        let mut old: Vec<Option<Layer>> = old.into_iter().map(Some).collect();
        self.layers = order.into_iter().filter_map(|at| old[at].take()).collect();
        let layers = &self.layers;
        let first = |group: &Group| {
            layers
                .iter()
                .position(|layer| layer.group.as_deref() == Some(&group.id))
        };
        self.groups.retain(|group| first(group).is_some());
        self.groups.sort_by_key(first);
    }
}

/// A new group of `layers`, which leave the groups they were in.
pub(crate) fn group_layers(vj: &mut Vj, layers: &[String], id: String) -> Result<()> {
    let wanted: HashSet<&str> = layers.iter().map(String::as_str).collect();
    if wanted.is_empty() {
        return Err(ProjectError::Invalid("a group needs a layer".into()));
    }
    if let Some(missing) = wanted
        .iter()
        .find(|id| !vj.layers.iter().any(|layer| layer.id == **id))
    {
        return Err(ProjectError::Invalid(format!("no layer {missing}")));
    }
    if vj.groups.len() >= MAX_GROUPS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_GROUPS} groups"
        )));
    }
    for layer in &mut vj.layers {
        if wanted.contains(layer.id.as_str()) {
            layer.group = Some(id.clone());
        }
    }
    vj.groups.push(Group::new(id));
    Ok(())
}

/// Puts `layer` in `group` (or none): on top of the group's layers unless `to` is given. `empty`
/// is the id of the layer that takes the place of the last one of the group it leaves.
pub(crate) fn layer_group(
    vj: &mut Vj,
    layer: &str,
    group: Option<String>,
    to: Option<usize>,
    empty: String,
) -> Result<()> {
    let at = vj
        .layers
        .iter()
        .position(|l| l.id == layer)
        .ok_or_else(|| ProjectError::Invalid(format!("no layer {layer}")))?;
    if let Some(group) = &group
        && vj.group(group).is_none()
    {
        return Err(ProjectError::Invalid(format!("no group {group}")));
    }
    let old = vj.layers[at].group.clone();
    if old == group && to.is_none() {
        return Ok(());
    }
    let mut moved = vj.layers.remove(at);
    moved.group.clone_from(&group);
    let place = match (to, &group) {
        (Some(to), _) => to.min(vj.layers.len()),
        (None, Some(group)) => vj
            .layers
            .iter()
            .rposition(|l| l.group.as_ref() == Some(group))
            .map_or(at, |top| top + 1),
        (None, None) => old
            .as_ref()
            .and_then(|old| {
                vj.layers
                    .iter()
                    .rposition(|l| l.group.as_ref() == Some(old))
            })
            .map_or(at.min(vj.layers.len()), |top| top + 1),
    };
    vj.layers.insert(place, moved);
    // As in Resolume, a group is never empty: its last layer leaves an empty one in its place.
    if let Some(old) = old.filter(|old| Some(old) != group.as_ref())
        && !vj.layers.iter().any(|l| l.group.as_ref() == Some(&old))
        && vj.layers.len() < MAX_LAYERS
    {
        let mut fresh = Layer::new(empty, false);
        fresh.group = Some(old);
        vj.layers.insert(at.min(vj.layers.len()), fresh);
    }
    Ok(())
}

/// Takes the group away, keeping its layers.
pub(crate) fn ungroup(vj: &mut Vj, group: &str) -> Result<()> {
    vj.group(group)
        .ok_or_else(|| ProjectError::Invalid(format!("no group {group}")))?;
    for layer in &mut vj.layers {
        if layer.group.as_deref() == Some(group) {
            layer.group = None;
        }
    }
    Ok(())
}

/// After `layer` was moved: it joins the group it was dropped inside of, and leaves its own when
/// dropped beside none of its layers.
pub(crate) fn settle_moved(vj: &mut Vj, layer: &str) {
    let Some(at) = vj.layers.iter().position(|l| l.id == layer) else {
        return;
    };
    let side = |at: Option<usize>| at.and_then(|at| vj.layers.get(at)?.group.clone());
    let below = side(at.checked_sub(1));
    let above = side(Some(at + 1));
    let own = vj.layers[at].group.clone();
    let next_to = own.is_some() && (below == own || above == own);
    if below.is_some() && below == above {
        vj.layers[at].group = below;
    } else if !next_to {
        vj.layers[at].group = None;
    }
}

/// The groups are valid: unique ids and names, levels in range, effects, every layer's group known,
/// the layers of a group next to each other, and no group without a layer.
pub(crate) fn validate<'a>(
    vj: &'a Vj,
    ids: &mut HashSet<&'a str>,
    effects: &mut HashSet<&'a str>,
) -> Result<()> {
    if vj.groups.len() > MAX_GROUPS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_GROUPS} groups"
        )));
    }
    for group in &vj.groups {
        if !ids.insert(&group.id) {
            return Err(ProjectError::Invalid(format!(
                "duplicate grid id: {}",
                group.id
            )));
        }
        check_name(group.name.as_deref())?;
        if ![group.master, group.opacity]
            .iter()
            .all(|v| (0.0..=1.0).contains(v))
        {
            return Err(ProjectError::Invalid(format!(
                "group {} needs levels of 0 to 1",
                group.id
            )));
        }
        validate_stack(&group.effects, false, effects)?;
        if vj.group_layers(&group.id).next().is_none() {
            return Err(ProjectError::Invalid(format!(
                "group {} has no layer",
                group.id
            )));
        }
    }
    let mut closed: HashSet<&str> = HashSet::new();
    let mut open: Option<&str> = None;
    for layer in &vj.layers {
        let group = layer.group.as_deref();
        if group != open {
            closed.extend(open);
            open = group;
        }
        if let Some(group) = group
            && (vj.group(group).is_none() || closed.contains(group))
        {
            return Err(ProjectError::Invalid(format!(
                "layer {} is in group {group}, which is unknown or not next to its other layers",
                layer.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
