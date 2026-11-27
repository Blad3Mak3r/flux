//! The `.flxmap` file: the mappings and dashboard links of a VJ grid, to carry to another show.
//!
//! A mapping names its target by **id** (the id of a layer, a clip), which means nothing in
//! another project, so the file names it by **position** instead, 0-based, and import looks the
//! position up in the project it is read into:
//!
//! | Id of | In the file |
//! | --- | --- |
//! | layer | the layer's place from the bottom: `"2"` |
//! | deck | the deck's place: `"0"` |
//! | column | `"deck.column"`: `"0.3"` |
//! | clip | `"layer/deck.column"`, or `"layer/sN"` for a slot of a global layer: `"2/0.3"`, `"1/s5"` |
//! | effect | `"owner:slot"`, the owner being `c` (the composition), `lN` (layer N) or `kN/…` (a clip, as above) |
//!
//! Everything else (a source, a range, a global target, a knob's number, an effect's setting by
//! name) is stored as it is. What does not resolve, or no longer fits (a place that is not there,
//! a setting the effect in that slot does not have), is skipped and counted.

use serde::{Deserialize, Serialize};

use crate::vj::Vj;
use crate::vj_mapping::{
    ButtonMode, Follow, Link, Source, Target, add_mapping, link_knob, rename_knob, set_mapping,
};
use crate::{ProjectError, Result};

/// The `format` of the file.
pub const FORMAT: &str = "flux-vj-mappings";
/// The version this build writes and the newest it reads.
pub const VERSION: u32 = 1;

/// What the user decided about the mappings an import would overwrite. The default (nothing
/// wiped, nothing kept) merges: an incoming mapping replaces the one with the same source or the
/// same target, and the rest of the show's are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportChoice {
    /// Throws away the show's mappings and dashboard links first: the file takes their place.
    pub wipe: bool,
    /// The conflicts to leave as they are, by their [`Conflict::index`]; the incoming mapping
    /// is not imported.
    pub keep: Vec<usize>,
}

/// What an import did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    /// Left out because their place is not in this show, or they no longer fit.
    pub skipped: usize,
    /// Conflicts the user chose to keep as they were.
    pub kept: usize,
}

/// An incoming mapping that would overwrite one the show has.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    /// The place of the mapping in the file, which [`ImportChoice::keep`] names.
    pub index: usize,
    pub source: Source,
    /// What the show's mapping drives now.
    pub existing: Target,
    /// What the file's would drive, in this show.
    pub incoming: Target,
}

/// What reading a file into a show would do, before it is done.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ImportPlan {
    /// Mappings and dashboard links that resolve in this show.
    pub taken: usize,
    /// Those that do not.
    pub skipped: usize,
    pub conflicts: Vec<Conflict>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMapping {
    source: Source,
    /// A [`Target`] whose ids are positions.
    target: Target,
    /// The same target by id, for a file read back into the show it came from; where the ids are
    /// not there, the positions are used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ids: Option<Target>,
    #[serde(default)]
    follow: Follow,
    min: f32,
    max: f32,
    #[serde(default)]
    invert: bool,
    #[serde(default)]
    mode: ButtonMode,
    /// The pad colours of this mapping; files written before them have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lights: Option<crate::vj_feedback::Lights>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileKnob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default)]
    links: Vec<Link>,
}

/// The content of a `.flxmap` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MapFile {
    format: String,
    version: u32,
    #[serde(default)]
    mappings: Vec<FileMapping>,
    #[serde(default)]
    dashboard: Vec<FileKnob>,
}

// --- ids and positions -------------------------------------------------------------------------

fn clip_key(vj: &Vj, clip: &str) -> Option<String> {
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

fn clip_at(vj: &Vj, key: &str) -> Option<String> {
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

fn column_key(vj: &Vj, column: &str) -> Option<String> {
    let (deck, col) = vj.column_place(column)?;
    let deck = vj.decks.iter().position(|d| d.id == deck.id)?;
    Some(format!("{deck}.{col}"))
}

fn column_at(vj: &Vj, key: &str) -> Option<String> {
    let (deck, col) = key.split_once('.')?;
    let deck = vj.decks.get(deck.parse::<usize>().ok()?)?;
    Some(deck.columns.get(col.parse::<usize>().ok()?)?.id.clone())
}

fn layer_key(vj: &Vj, layer: &str) -> Option<String> {
    Some(vj.layers.iter().position(|l| l.id == layer)?.to_string())
}

fn layer_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.layers.get(key.parse::<usize>().ok()?)?.id.clone())
}

fn group_key(vj: &Vj, group: &str) -> Option<String> {
    Some(vj.groups.iter().position(|g| g.id == group)?.to_string())
}

fn group_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.groups.get(key.parse::<usize>().ok()?)?.id.clone())
}

fn deck_key(vj: &Vj, deck: &str) -> Option<String> {
    Some(vj.decks.iter().position(|d| d.id == deck)?.to_string())
}

fn deck_at(vj: &Vj, key: &str) -> Option<String> {
    Some(vj.decks.get(key.parse::<usize>().ok()?)?.id.clone())
}

fn effect_key(vj: &Vj, effect: &str) -> Option<String> {
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

fn effect_at(vj: &Vj, key: &str) -> Option<String> {
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
fn rewrite(
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

// --- export and import -------------------------------------------------------------------------

/// The file of the mappings and dashboard links of `vj`, as the text to write. What cannot be
/// named by position (nothing a valid grid holds) is left out.
pub fn export(vj: &Vj) -> Result<String> {
    let mappings = vj
        .mappings
        .iter()
        .filter_map(|m| {
            let target = match (&m.place, m.follow) {
                (Some(place), Follow::Position) => place.clone(),
                _ => to_places(vj, &m.target)?,
            };
            Some(FileMapping {
                source: m.source.clone(),
                target,
                ids: (!m.parked).then(|| m.target.clone()),
                follow: m.follow,
                min: m.min,
                max: m.max,
                invert: m.invert,
                mode: m.mode,
                lights: m.lights,
            })
        })
        .collect();
    let dashboard = vj
        .dashboard
        .iter()
        .map(|knob| FileKnob {
            name: knob.name.clone(),
            links: knob
                .links
                .iter()
                .filter_map(|link| {
                    Some(Link {
                        target: to_places(vj, &link.target)?,
                        ..link.clone()
                    })
                })
                .collect(),
        })
        .collect();
    let file = MapFile {
        format: FORMAT.into(),
        version: VERSION,
        mappings,
        dashboard,
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

/// Reads the text of a `.flxmap`, refusing what is not one and a version newer than this one.
pub fn parse(text: &str) -> Result<MapFile> {
    let file: MapFile = serde_json::from_str(text)
        .map_err(|err| ProjectError::Invalid(format!("not a mappings file: {err}")))?;
    if file.format != FORMAT {
        return Err(ProjectError::Invalid("not a mappings file".into()));
    }
    if file.version > VERSION {
        return Err(ProjectError::Invalid(format!(
            "the mappings file is version {}, and this FLUX reads up to {VERSION}",
            file.version
        )));
    }
    Ok(file)
}

/// Gives the mapping just added the mode and slot the file says (the default of a new one
/// otherwise stands, for a file without them).
fn keep_follow(vj: &mut Vj, id: &str, incoming: &FileMapping) -> bool {
    let Some(mapping) = vj.mappings.iter_mut().find(|m| m.id == id) else {
        return false;
    };
    if kind_of(&mapping.target) != Kind::Plain {
        mapping.follow = incoming.follow;
        mapping.place = (incoming.follow == Follow::Position).then(|| incoming.target.clone());
    }
    true
}

/// Where an incoming mapping lands in `vj`: its id if the show has it, else its position.
fn resolve(vj: &Vj, incoming: &FileMapping) -> Option<Target> {
    incoming
        .ids
        .clone()
        .filter(|ids| ids.exists(vj))
        .or_else(|| from_places(vj, &incoming.target))
}

/// What importing `file` into `vj` would do: how much resolves, and the mappings it would overwrite.
pub fn plan(vj: &Vj, file: &MapFile) -> ImportPlan {
    let mut plan = ImportPlan::default();
    for (index, incoming) in file.mappings.iter().enumerate() {
        let Some(target) = resolve(vj, incoming) else {
            plan.skipped += 1;
            continue;
        };
        plan.taken += 1;
        let existing = vj
            .mappings
            .iter()
            .find(|m| m.source == incoming.source || m.target == target);
        if let Some(existing) = existing {
            plan.conflicts.push(Conflict {
                index,
                source: incoming.source.clone(),
                existing: existing.target.clone(),
                incoming: target,
            });
        }
    }
    for link in file.dashboard.iter().flat_map(|knob| &knob.links) {
        if from_places(vj, &link.target).is_some() {
            plan.taken += 1;
        } else {
            plan.skipped += 1;
        }
    }
    plan
}

/// Puts the mappings and links of `file` into `vj`, looking each place up in it. A mapping or
/// link that does not resolve or does not fit is counted as skipped.
pub fn import(
    vj: &mut Vj,
    file: &MapFile,
    choice: &ImportChoice,
    mut new_id: impl FnMut() -> String,
) -> ImportReport {
    let mut report = ImportReport::default();
    let conflicts = if choice.wipe {
        Vec::new()
    } else {
        plan(vj, file).conflicts
    };
    if choice.wipe {
        vj.mappings.clear();
        vj.dashboard.clear();
    }
    for (index, incoming) in file.mappings.iter().enumerate() {
        if choice.keep.contains(&index) && conflicts.iter().any(|c| c.index == index) {
            report.kept += 1;
            continue;
        }
        let done = resolve(vj, incoming).is_some_and(|target| {
            let id = new_id();
            add_mapping(vj, incoming.source.clone(), target, id.clone()).is_ok()
                && keep_follow(vj, &id, incoming)
                && {
                    if let Some(m) = vj.mappings.iter_mut().find(|m| m.id == id) {
                        m.lights = incoming.lights;
                    }
                    true
                }
                && (set_mapping(
                    vj,
                    &id,
                    incoming.min,
                    incoming.max,
                    incoming.invert,
                    incoming.mode,
                )
                .is_ok()
                    || {
                        vj.mappings.retain(|m| m.id != id);
                        false
                    })
        });
        if done {
            report.imported += 1;
        } else {
            report.skipped += 1;
        }
    }
    for (knob, incoming) in file
        .dashboard
        .iter()
        .enumerate()
        .take(crate::vj_mapping::KNOBS)
    {
        if incoming.name.is_some() && rename_knob(vj, knob, incoming.name.clone()).is_err() {
            report.skipped += 1;
        }
        for link in &incoming.links {
            let linked = from_places(vj, &link.target).is_some_and(|target| {
                link_knob(vj, knob, target, Some(link.min), Some(link.max)).is_ok()
            });
            if linked {
                report.imported += 1;
            } else {
                report.skipped += 1;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj::{EffectTarget, VjOp};
    use crate::vj_mapping::{MidiKind, Target};
    use crate::{Project, ProjectMode};
    use flux_core::FrameRate;

    /// A VJ show whose ids all start with `prefix`, so two shows never share one.
    fn show(prefix: &str) -> Project {
        let mut next = 0;
        let mut ids = move || {
            next += 1;
            format!("{prefix}{next}")
        };
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(&mut ids);
        project
    }

    fn apply(project: &mut Project, op: VjOp) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        crate::vj::apply(project, op, || {
            format!("gen{}", NEXT.fetch_add(1, Ordering::Relaxed))
        })
        .unwrap();
    }

    fn cc(number: u8) -> Source {
        Source::Midi {
            port: Some("APC40".into()),
            channel: 1,
            kind: MidiKind::Cc,
            number,
        }
    }

    fn wipe() -> ImportChoice {
        ImportChoice {
            wipe: true,
            keep: Vec::new(),
        }
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("new{n}")
        }
    }

    /// A show with a blur on its second layer, mapped from every kind of source.
    fn mapped() -> Project {
        let mut project = show("a");
        let layer = project.vj.layers[1].id.clone();
        apply(
            &mut project,
            VjOp::AddEffect {
                target: EffectTarget::Layer(layer.clone()),
                kind: "blur".into(),
            },
        );
        let effect = project.vj.layers[1].effects[0].id.clone();
        let column = project.vj.decks[0].columns[2].id.clone();
        let sources_and_targets = [
            (
                cc(1),
                Target::LayerOpacity {
                    layer: layer.clone(),
                },
            ),
            (
                Source::Artnet {
                    universe: 1,
                    channel: 37,
                    fine: Some(38),
                },
                Target::EffectParam {
                    effect,
                    param: "radius".into(),
                },
            ),
            (
                Source::Key {
                    code: "KeyQ".into(),
                    ctrl: false,
                    alt: false,
                    shift: true,
                    meta: false,
                },
                Target::ColumnLaunch { column },
            ),
            (
                Source::Osc {
                    address: "/go".into(),
                },
                Target::LayerBypass { layer },
            ),
            (cc(2), Target::Crossfader),
        ];
        for (source, target) in sources_and_targets {
            apply(&mut project, VjOp::AddMapping { source, target });
        }
        let id = project.vj.mappings[0].id.clone();
        apply(
            &mut project,
            VjOp::SetMapping {
                mapping: id,
                min: 0.2,
                max: 0.9,
                invert: true,
                mode: ButtonMode::Trigger,
            },
        );
        let target = Target::LayerMaster {
            layer: project.vj.layers[2].id.clone(),
        };
        apply(
            &mut project,
            VjOp::DashboardLink {
                knob: 3,
                target,
                min: Some(0.1),
                max: Some(0.8),
            },
        );
        apply(
            &mut project,
            VjOp::RenameKnob {
                knob: 3,
                name: "Fade".into(),
            },
        );
        project
    }

    #[test]
    fn a_file_should_carry_mappings_and_links_into_a_show_with_other_ids() {
        let from = mapped();
        let text = export(&from.vj).unwrap();
        let mut into = show("b");
        let layer = into.vj.layers[1].id.clone();
        apply(
            &mut into,
            VjOp::AddEffect {
                target: EffectTarget::Layer(layer),
                kind: "blur".into(),
            },
        );

        let report = import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        assert_eq!(
            report,
            ImportReport {
                imported: 6,
                skipped: 0,
                kept: 0
            }
        );
        into.validate().unwrap();
        let target_of = |source: &Source| {
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == *source)
                .unwrap()
                .clone()
        };
        let opacity = target_of(&cc(1));
        assert_eq!(
            opacity.target,
            Target::LayerOpacity {
                layer: into.vj.layers[1].id.clone()
            }
        );
        assert_eq!((opacity.min, opacity.max, opacity.invert), (0.2, 0.9, true));
        assert_eq!(
            target_of(&Source::Artnet {
                universe: 1,
                channel: 37,
                fine: Some(38)
            })
            .target,
            Target::EffectParam {
                effect: into.vj.layers[1].effects[0].id.clone(),
                param: "radius".into()
            }
        );
        assert_eq!(
            target_of(&Source::Osc {
                address: "/go".into()
            })
            .target,
            Target::LayerBypass {
                layer: into.vj.layers[1].id.clone()
            }
        );
        let knob = &into.vj.dashboard[3];
        assert_eq!(knob.name.as_deref(), Some("Fade"));
        assert_eq!(
            knob.links[0].target,
            Target::LayerMaster {
                layer: into.vj.layers[2].id.clone()
            }
        );
        assert_eq!((knob.links[0].min, knob.links[0].max), (0.1, 0.8));
    }

    #[test]
    fn what_does_not_resolve_should_be_skipped_and_counted() {
        let text = export(&mapped().vj).unwrap();
        // No effect on the layer, so its setting is gone; one deck with columns, but fewer layers.
        let mut into = show("b");
        into.vj.layers.truncate(1);

        let report = import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        // Only the crossfader and the column launch fit; the link and four mappings do not.
        assert_eq!(
            report,
            ImportReport {
                imported: 2,
                skipped: 4,
                kept: 0
            }
        );
        assert_eq!(into.vj.mappings.len(), 2);
        into.validate().unwrap();
    }

    #[test]
    fn merge_should_replace_the_same_source_and_keep_the_rest() {
        let text = export(&mapped().vj).unwrap();
        let mut into = show("b");
        let old = into.vj.layers[0].id.clone();
        let kept = Target::LayerMaster { layer: old.clone() };
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(9),
                target: kept.clone(),
            },
        );
        // The same source as the file's cc 1, on another target.
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerVolume { layer: old },
            },
        );

        import(
            &mut into.vj,
            &parse(&text).unwrap(),
            &ImportChoice::default(),
            ids(),
        );

        assert!(
            into.vj
                .mappings
                .iter()
                .any(|m| m.source == cc(9) && m.target == kept)
        );
        let ones: Vec<_> = into
            .vj
            .mappings
            .iter()
            .filter(|m| m.source == cc(1))
            .collect();
        assert_eq!(ones.len(), 1, "the incoming one replaced the old");
        assert_eq!(
            ones[0].target,
            Target::LayerOpacity {
                layer: into.vj.layers[1].id.clone()
            }
        );
    }

    #[test]
    fn replace_should_drop_what_the_show_had() {
        let text = export(&mapped().vj).unwrap();
        let mut into = show("b");
        let layer = into.vj.layers[0].id.clone();
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(9),
                target: Target::LayerMaster { layer },
            },
        );

        import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        assert!(!into.vj.mappings.iter().any(|m| m.source == cc(9)));
    }

    #[test]
    fn a_file_of_a_newer_version_or_another_kind_should_be_refused() {
        let text = export(&mapped().vj).unwrap();
        let newer = text.replace("\"version\": 1", "\"version\": 2");
        let other = text.replace(FORMAT, "something-else");

        assert!(parse(&newer).is_err());
        assert!(parse(&other).is_err());
        assert!(parse("not json").is_err());
        assert!(parse(&text).is_ok());
    }

    #[test]
    fn the_file_should_name_places_and_never_ids() {
        let text = export(&mapped().vj).unwrap();

        let file = parse(&text).unwrap();
        let ids_in_places = format!(
            "{:?}",
            file.mappings.iter().map(|m| &m.target).collect::<Vec<_>>()
        );
        assert!(!ids_in_places.contains("\"a"), "{ids_in_places}");
        assert!(text.contains("\"layer\": \"1\""));
        assert!(text.contains("\"column\": \"0.2\""));
        assert!(text.contains("\"effect\": \"l1:0\""));
    }

    /// A show with the two mappings of `cc` 1 and 2 on its layers 0 and 1.
    fn existing() -> Project {
        let mut into = show("b");
        for (number, layer) in [(1, 0), (2, 1)] {
            let layer = into.vj.layers[layer].id.clone();
            apply(
                &mut into,
                VjOp::AddMapping {
                    source: cc(number),
                    target: Target::LayerMaster { layer },
                },
            );
        }
        into
    }

    fn file() -> MapFile {
        let mut from = show("a");
        let layer = from.vj.layers[2].id.clone();
        // Overwrites the source of the show's first mapping, and the target of its second.
        apply(
            &mut from,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerOpacity { layer },
            },
        );
        let layer = from.vj.layers[1].id.clone();
        apply(
            &mut from,
            VjOp::AddMapping {
                source: cc(7),
                target: Target::LayerMaster { layer },
            },
        );
        let layer = from.vj.layers[0].id.clone();
        apply(
            &mut from,
            VjOp::AddMapping {
                source: cc(8),
                target: Target::LayerSolo { layer },
            },
        );
        parse(&export(&from.vj).unwrap()).unwrap()
    }

    fn sources(project: &Project) -> Vec<Source> {
        let mut found: Vec<_> = project
            .vj
            .mappings
            .iter()
            .map(|m| m.source.clone())
            .collect();
        found.sort_by_key(|source| format!("{source:?}"));
        found
    }

    #[test]
    fn the_plan_should_list_what_would_be_overwritten_and_touch_nothing() {
        let into = existing();

        let plan = plan(&into.vj, &file());

        assert_eq!((plan.taken, plan.skipped), (3, 0));
        let found: Vec<_> = plan
            .conflicts
            .iter()
            .map(|c| (c.index, c.source.clone()))
            .collect();
        assert_eq!(
            found,
            vec![(0, cc(1)), (1, cc(7))],
            "the same source, and the same target"
        );
        assert_eq!(
            plan.conflicts[0].existing,
            Target::LayerMaster {
                layer: into.vj.layers[0].id.clone()
            }
        );
        assert_eq!(
            plan.conflicts[0].incoming,
            Target::LayerOpacity {
                layer: into.vj.layers[2].id.clone()
            }
        );
        assert_eq!(into.vj.mappings.len(), 2);
    }

    #[test]
    fn a_show_with_nothing_to_overwrite_should_have_no_conflicts() {
        assert!(plan(&show("b").vj, &file()).conflicts.is_empty());
    }

    #[test]
    fn overwrite_all_should_take_every_incoming_mapping_and_keep_the_rest() {
        let mut into = existing();

        let report = import(&mut into.vj, &file(), &ImportChoice::default(), ids());

        assert_eq!((report.imported, report.kept), (3, 0));
        assert_eq!(
            sources(&into),
            vec![cc(1), cc(7), cc(8)]
                .into_iter()
                .chain([])
                .collect::<Vec<_>>()
        );
        into.validate().unwrap();
    }

    #[test]
    fn keep_existing_should_import_only_what_does_not_conflict() {
        let mut into = existing();
        let keep = ImportChoice {
            wipe: false,
            keep: vec![0, 1],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (1, 2));
        assert_eq!(sources(&into), vec![cc(1), cc(2), cc(8)]);
        assert_eq!(
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == cc(1))
                .unwrap()
                .target,
            Target::LayerMaster {
                layer: into.vj.layers[0].id.clone()
            },
            "the show's own mapping stands"
        );
    }

    #[test]
    fn a_choice_per_conflict_should_keep_one_and_overwrite_the_other() {
        let mut into = existing();
        let keep = ImportChoice {
            wipe: false,
            keep: vec![1],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (2, 1));
        assert!(into.vj.mappings.iter().any(|m| m.source == cc(2)), "kept");
        assert_eq!(
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == cc(1))
                .unwrap()
                .target,
            Target::LayerOpacity {
                layer: into.vj.layers[2].id.clone()
            },
            "overwritten"
        );
    }

    #[test]
    fn replace_all_should_leave_only_the_files_mappings() {
        let mut into = existing();

        let report = import(&mut into.vj, &file(), &wipe(), ids());

        assert_eq!((report.imported, report.kept), (3, 0));
        assert_eq!(sources(&into), vec![cc(1), cc(7), cc(8)]);
    }

    #[test]
    fn a_keep_that_names_no_conflict_should_change_nothing() {
        let mut into = show("b");
        let keep = ImportChoice {
            wipe: false,
            keep: vec![0, 1, 2],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (3, 0));
    }

    #[test]
    fn a_file_read_back_into_its_own_show_should_find_its_ids_first() {
        let mut project = mapped();
        let text = export(&project.vj).unwrap();
        // The layers are reordered since the export: by id the mapping still finds its layer.
        let layer = project.vj.layers[1].id.clone();
        apply(
            &mut project,
            VjOp::MoveLayer {
                layer: layer.clone(),
                to: 0,
            },
        );
        let id = project
            .vj
            .mappings
            .iter()
            .find(|m| m.source == cc(2))
            .unwrap()
            .id
            .clone();
        apply(&mut project, VjOp::RemoveMapping { mapping: id });
        let by_id = Target::LayerBypass {
            layer: layer.clone(),
        };

        let mut into = project.clone();
        import(
            &mut into.vj,
            &parse(&text).unwrap(),
            &ImportChoice::default(),
            ids(),
        );

        let found = into
            .vj
            .mappings
            .iter()
            .find(|m| {
                m.source
                    == Source::Osc {
                        address: "/go".into(),
                    }
            })
            .unwrap();
        assert_eq!(
            found.target, by_id,
            "the layer by id, though it is no longer at its position"
        );
    }

    #[test]
    fn pad_colours_should_travel_in_the_file_and_an_old_file_should_have_none() {
        let mut from = mapped();
        let id = from.vj.mappings[0].id.clone();
        let lights = crate::vj_feedback::Lights {
            off: 0,
            loaded: 5,
            on: 1,
        };
        apply(
            &mut from,
            VjOp::SetMappingLights {
                mapping: id,
                lights: Some(lights),
            },
        );
        let source = from.vj.mappings[0].source.clone();
        let text = export(&from.vj).unwrap();

        let mut into = show("b");
        import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());
        let got = into
            .vj
            .mappings
            .iter()
            .find(|m| m.source == source)
            .unwrap();
        assert_eq!(got.lights, Some(lights));

        let old = text.replace("\"lights\"", "\"unknown\"");
        let mut into = show("c");
        import(&mut into.vj, &parse(&old).unwrap(), &wipe(), ids());
        assert!(into.vj.mappings.iter().all(|m| m.lights.is_none()));
    }
}
