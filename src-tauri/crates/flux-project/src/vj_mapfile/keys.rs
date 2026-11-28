//! Ids to places and back: how a target is named by position in the file.

use crate::vj::Vj;
use crate::vj_mapping::Target;

pub(super) fn clip_key(vj: &Vj, clip: &str) -> Option<String> {
    let (layer, found) = vj.find_clip(clip)?;
    let at = vj.layers.iter().position(|l| l.id == layer.id)?;
    let place = match (&found.column, found.slot) {
        (Some(column), _) => {
            let (deck, col) = vj.column_place(column)?;
            let deck = vj.decks.iter().position(|d| d.id == deck.id)?;
            format!("{deck}.{col}")
        }
        (None, Some(slot)) => format!("s{slot}"),
        (None, None) => return None,
    };
    Some(format!("{at}/{place}"))
}

pub(super) fn clip_at(vj: &Vj, key: &str) -> Option<String> {
    let (layer, place) = key.split_once('/')?;
    let layer = vj.layers.get(layer.parse::<usize>().ok()?)?;
    let column = match place.strip_prefix('s') {
        Some(slot) => {
            let slot = slot.parse::<usize>().ok()?;
            return layer
                .clips
                .iter()
                .find(|clip| clip.column.is_none() && clip.slot == Some(slot))
                .map(|clip| clip.id.clone());
        }
        None => column_at(vj, place)?,
    };
    layer
        .clips
        .iter()
        .find(|clip| clip.column.as_deref() == Some(&column))
        .map(|clip| clip.id.clone())
}

pub(super) fn column_key(vj: &Vj, column: &str) -> Option<String> {
    let (deck, col) = vj.column_place(column)?;
    let deck = vj.decks.iter().position(|d| d.id == deck.id)?;
    Some(format!("{deck}.{col}"))
}

pub(super) fn column_at(vj: &Vj, key: &str) -> Option<String> {
    let (deck, col) = key.split_once('.')?;
    let deck = vj.decks.get(deck.parse::<usize>().ok()?)?;
    Some(deck.columns.get(col.parse::<usize>().ok()?)?.id.clone())
}

pub(super) fn layer_key(vj: &Vj, layer: &str) -> Option<String> {
    Some(vj.layers.iter().position(|l| l.id == layer)?.to_string())
}

pub(super) fn layer_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.layers.get(key.parse::<usize>().ok()?)?.id.clone())
}

pub(super) fn group_key(vj: &Vj, group: &str) -> Option<String> {
    Some(vj.groups.iter().position(|g| g.id == group)?.to_string())
}

pub(super) fn group_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.groups.get(key.parse::<usize>().ok()?)?.id.clone())
}

pub(super) fn deck_key(vj: &Vj, deck: &str) -> Option<String> {
    Some(vj.decks.iter().position(|d| d.id == deck)?.to_string())
}

pub(super) fn deck_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.decks.get(key.parse::<usize>().ok()?)?.id.clone())
}

pub(super) fn effect_key(vj: &Vj, effect: &str) -> Option<String> {
    let slot = |stack: &[crate::vj_effects::Effect]| stack.iter().position(|e| e.id == effect);
    if let Some(at) = slot(&vj.composition_effects) {
        return Some(format!("c:{at}"));
    }
    for (i, group) in vj.groups.iter().enumerate() {
        if let Some(at) = slot(&group.effects) {
            return Some(format!("g{i}:{at}"));
        }
    }
    for (i, layer) in vj.layers.iter().enumerate() {
        if let Some(at) = slot(&layer.effects) {
            return Some(format!("l{i}:{at}"));
        }
        for clip in &layer.clips {
            if let Some(at) = slot(&clip.effects) {
                return Some(format!("k{}:{at}", clip_key(vj, &clip.id)?));
            }
        }
    }
    None
}

pub(super) fn effect_at(vj: &Vj, key: &str) -> Option<String> {
    let (owner, at) = key.rsplit_once(':')?;
    let at = at.parse::<usize>().ok()?;
    let stack = if owner == "c" {
        &vj.composition_effects
    } else if let Some(group) = owner.strip_prefix('g') {
        &vj.groups.get(group.parse::<usize>().ok()?)?.effects
    } else if let Some(layer) = owner.strip_prefix('l') {
        &vj.layers.get(layer.parse::<usize>().ok()?)?.effects
    } else {
        let clip = clip_at(vj, owner.strip_prefix('k')?)?;
        &vj.find_clip(&clip)?.1.effects
    };
    Some(stack.get(at)?.id.clone())
}

/// `target` with every id replaced by `map`'s answer for it; `None` when one has none.
pub(super) fn rewrite(
    target: &Target,
    layer: impl Fn(&str) -> Option<String>,
    group: impl Fn(&str) -> Option<String>,
    deck: impl Fn(&str) -> Option<String>,
    column: impl Fn(&str) -> Option<String>,
    clip: impl Fn(&str) -> Option<String>,
    effect: impl Fn(&str) -> Option<String>,
) -> Option<Target> {
    use Target as T;
    Some(match target {
        T::ClipLaunch { clip: id } => T::ClipLaunch { clip: clip(id)? },
        T::ClipSpeed { clip: id } => T::ClipSpeed { clip: clip(id)? },
        T::GeneratorNumber { clip: id, name } => T::GeneratorNumber {
            clip: clip(id)?,
            name: name.clone(),
        },
        T::ColumnLaunch { column: id } => T::ColumnLaunch {
            column: column(id)?,
        },
        T::LayerStop { layer: id } => T::LayerStop { layer: layer(id)? },
        T::LayerAutopilot { layer: id } => T::LayerAutopilot { layer: layer(id)? },
        T::LayerBypass { layer: id } => T::LayerBypass { layer: layer(id)? },
        T::LayerSolo { layer: id } => T::LayerSolo { layer: layer(id)? },
        T::LayerMaster { layer: id } => T::LayerMaster { layer: layer(id)? },
        T::LayerVolume { layer: id } => T::LayerVolume { layer: layer(id)? },
        T::LayerOpacity { layer: id } => T::LayerOpacity { layer: layer(id)? },
        T::LayerTransition { layer: id } => T::LayerTransition { layer: layer(id)? },
        T::GroupBypass { group: id } => T::GroupBypass { group: group(id)? },
        T::GroupSolo { group: id } => T::GroupSolo { group: group(id)? },
        T::GroupMaster { group: id } => T::GroupMaster { group: group(id)? },
        T::GroupOpacity { group: id } => T::GroupOpacity { group: group(id)? },
        T::DeckSelect { deck: id } => T::DeckSelect { deck: deck(id)? },
        T::EffectBypass { effect: id } => T::EffectBypass {
            effect: effect(id)?,
        },
        T::EffectParam { effect: id, param } => T::EffectParam {
            effect: effect(id)?,
            param: param.clone(),
        },
        other => other.clone(),
    })
}

/// What a target's ids name, for the mode a mapping can follow it in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Nothing with an id: a global target or a dashboard knob.
    Plain,
    Layer,
    Group,
    Deck,
    Column,
    Clip,
    Effect,
}

pub(crate) fn kind_of(target: &Target) -> Kind {
    use Target as T;
    match target {
        T::ClipLaunch { .. } | T::ClipSpeed { .. } | T::GeneratorNumber { .. } => Kind::Clip,
        T::ColumnLaunch { .. } => Kind::Column,
        T::DeckSelect { .. } => Kind::Deck,
        T::GroupBypass { .. }
        | T::GroupSolo { .. }
        | T::GroupMaster { .. }
        | T::GroupOpacity { .. } => Kind::Group,
        T::EffectBypass { .. } | T::EffectParam { .. } => Kind::Effect,
        T::LayerStop { .. }
        | T::LayerAutopilot { .. }
        | T::LayerBypass { .. }
        | T::LayerSolo { .. }
        | T::LayerMaster { .. }
        | T::LayerVolume { .. }
        | T::LayerOpacity { .. }
        | T::LayerTransition { .. } => Kind::Layer,
        _ => Kind::Plain,
    }
}

pub(crate) fn to_places(vj: &Vj, target: &Target) -> Option<Target> {
    rewrite(
        target,
        |id| layer_key(vj, id),
        |id| group_key(vj, id),
        |id| deck_key(vj, id),
        |id| column_key(vj, id),
        |id| clip_key(vj, id),
        |id| effect_key(vj, id),
    )
}

pub(crate) fn from_places(vj: &Vj, target: &Target) -> Option<Target> {
    rewrite(
        target,
        |key| layer_at(vj, key),
        |key| group_at(vj, key),
        |key| deck_at(vj, key),
        |key| column_at(vj, key),
        |key| clip_at(vj, key),
        |key| effect_at(vj, key),
    )
}
