use super::*;

/// Whether a knob can drive `target`: a number stored with the grid (not the runtime crossfader,
/// and not another knob).
pub(crate) fn linkable(target: &Target) -> bool {
    target.class() == Class::Continuous
        && !matches!(
            target,
            Target::Crossfader
                | Target::Dashboard { .. }
                | Target::LayerDashboard { .. }
                | Target::ClipDashboard { .. }
        )
}

/// Where the mapping `id` is in the list.
fn mapping_at(vj: &Vj, id: &str) -> Result<usize> {
    vj.mappings
        .iter()
        .position(|m| m.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no mapping {id}")))
}

pub(crate) fn finite(value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ProjectError::Invalid("a mapping is made of numbers".into()))
    }
}

/// The mode a new mapping of `target` starts with.
fn default_mode(target: &Target) -> ButtonMode {
    if target.class() == Class::Switch {
        ButtonMode::Toggle
    } else {
        ButtonMode::Trigger
    }
}

/// Whether a target of this class takes this button mode.
fn mode_fits(class: Class, mode: ButtonMode) -> bool {
    class == Class::Switch || mode != ButtonMode::Toggle
}

pub(crate) fn add_mapping(vj: &mut Vj, source: Source, target: Target, id: String) -> Result<()> {
    source.validate()?;
    if source.is_app_shortcut() {
        return Err(ProjectError::Invalid(
            "that key is used by the app's own shortcuts".into(),
        ));
    }
    if !target.exists(vj) {
        return Err(ProjectError::Invalid(format!(
            "there is nothing to map: {target:?}"
        )));
    }
    // The options of a target that is learned again stay.
    let kept = vj.mappings.iter().find(|m| m.target == target).cloned();
    let kept_none = kept.is_none();
    vj.mappings
        .retain(|m| m.target != target && !m.source.matches(&source));
    if vj.mappings.len() >= MAX_MAPPINGS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_MAPPINGS} mappings"
        )));
    }
    let (min, max) = target.range(vj).unwrap_or((0.0, 1.0));
    let mapping = match kept {
        Some(old) => Mapping {
            id,
            source,
            target,
            ..old
        },
        None => Mapping {
            id,
            source,
            mode: default_mode(&target),
            target,
            min,
            max,
            invert: false,
            follow: Follow::Id,
            place: None,
            parked: false,
            lights: None,
        },
    };
    let mut mapping = mapping;
    if kept_none && target_has_place(&mapping.target) && !is_clip_or_effect(&mapping.target) {
        // New layer, column and deck mappings follow their slot; clips and effects, themselves.
        if let Some(place) = crate::vj_mapfile::to_places(vj, &mapping.target) {
            mapping.follow = Follow::Position;
            mapping.place = Some(place);
        }
    }
    vj.mappings.push(mapping);
    Ok(())
}

fn target_has_place(target: &Target) -> bool {
    !matches!(
        crate::vj_mapfile::kind_of(target),
        crate::vj_mapfile::Kind::Plain
    )
}

fn is_clip_or_effect(target: &Target) -> bool {
    matches!(
        crate::vj_mapfile::kind_of(target),
        crate::vj_mapfile::Kind::Clip | crate::vj_mapfile::Kind::Effect
    )
}

/// Moves the position mappings to what fills their slot now; one whose slot is empty is parked,
/// and one that fills again is woken. A level is held to the range of its new target.
pub(crate) fn rebind(vj: &mut Vj) {
    let found: Vec<(usize, Option<Target>)> = vj
        .mappings
        .iter()
        .enumerate()
        .filter(|(_, m)| m.follow == Follow::Position)
        .filter_map(|(at, m)| Some((at, m.place.as_ref()?)))
        .map(|(at, place)| {
            let now = crate::vj_mapfile::from_places(vj, place).filter(|t| t.exists(vj));
            (at, now)
        })
        .collect();
    for (at, now) in found {
        let range = now.as_ref().and_then(|t| t.range(vj));
        let mapping = &mut vj.mappings[at];
        match now {
            Some(target) => {
                mapping.target = target;
                mapping.parked = false;
                if let Some((lo, hi)) = range {
                    mapping.min = mapping.min.clamp(lo, hi);
                    mapping.max = mapping.max.clamp(lo, hi);
                }
            }
            None => mapping.parked = true,
        }
    }
}

pub(crate) fn set_mapping_follow(vj: &mut Vj, id: &str, follow: Follow) -> Result<()> {
    let at = mapping_at(vj, id)?;
    let target = vj.mappings[at].target.clone();
    if !target_has_place(&target) {
        return Err(ProjectError::Invalid(
            "only a clip, layer, column, deck or effect has a slot to follow".into(),
        ));
    }
    let mapping = &mut vj.mappings[at];
    match follow {
        Follow::Id if mapping.parked => {
            return Err(ProjectError::Invalid(
                "the slot is empty: there is no clip, layer or effect to follow".into(),
            ));
        }
        Follow::Id => mapping.place = None,
        Follow::Position => {
            let place = crate::vj_mapfile::to_places(vj, &target)
                .ok_or_else(|| ProjectError::Invalid("that has no slot".into()))?;
            vj.mappings[at].place = Some(place);
        }
    }
    vj.mappings[at].follow = follow;
    Ok(())
}

pub(crate) fn remove_mapping(vj: &mut Vj, id: &str) -> Result<()> {
    let before = vj.mappings.len();
    vj.mappings.retain(|m| m.id != id);
    if vj.mappings.len() == before {
        return Err(ProjectError::Invalid(format!("no mapping {id}")));
    }
    Ok(())
}

pub(crate) fn set_mapping(
    vj: &mut Vj,
    id: &str,
    min: f32,
    max: f32,
    invert: bool,
    mode: ButtonMode,
) -> Result<()> {
    let at = mapping_at(vj, id)?;
    let target = vj.mappings[at].target.clone();
    if !mode_fits(target.class(), mode) {
        return Err(ProjectError::Invalid(
            "only a switch can be toggled by a button".into(),
        ));
    }
    let (lo, hi) = target.range(vj).unwrap_or((0.0, 1.0));
    let mapping = &mut vj.mappings[at];
    if target.class() == Class::Continuous {
        mapping.min = finite(min)?.clamp(lo, hi);
        mapping.max = finite(max)?.clamp(lo, hi);
    }
    mapping.invert = invert;
    mapping.mode = mode;
    Ok(())
}

/// Sets the colours of a mapping's pad, or with `None` takes them away.
pub(crate) fn set_mapping_lights(
    vj: &mut Vj,
    id: &str,
    lights: Option<crate::vj_feedback::Lights>,
) -> Result<()> {
    if let Some(lights) = lights {
        lights.validate()?;
    }
    let at = mapping_at(vj, id)?;
    vj.mappings[at].lights = lights;
    Ok(())
}

pub(crate) fn set_mapping_fine(vj: &mut Vj, id: &str, fine: Option<u16>) -> Result<()> {
    let at = mapping_at(vj, id)?;
    let mapping = &mut vj.mappings[at];
    let Source::Artnet {
        universe, channel, ..
    } = mapping.source
    else {
        return Err(ProjectError::Invalid(
            "only a DMX mapping has a fine channel".into(),
        ));
    };
    let source = Source::Artnet {
        universe,
        channel,
        fine,
    };
    source.validate()?;
    mapping.source = source;
    // Two mappings of one source would both answer it; the one just set stays.
    let id = mapping.id.clone();
    let source = mapping.source.clone();
    vj.mappings.retain(|m| m.id == id || m.source != source);
    Ok(())
}

pub(crate) fn link_knob(
    vj: &mut Vj,
    knob: usize,
    target: Target,
    min: Option<f32>,
    max: Option<f32>,
) -> Result<()> {
    crate::vj_dashboard::link(vj, &Scope::Composition, knob, target, min, max)
}

/// Sets the number `target` names directly in `vj`, as a knob's link does in the drawn copy of a
/// modulated grid. Ranges are held as the edits hold them; a target that is gone is skipped.
pub(crate) fn drive(vj: &mut Vj, target: &Target, value: f32) {
    if !value.is_finite() {
        return;
    }
    let unit = value.clamp(0.0, 1.0);
    match target {
        Target::LayerMaster { layer } => {
            if let Ok(l) = vj.layer_mut(layer) {
                l.master = unit;
            }
        }
        Target::LayerVolume { layer } => {
            if let Ok(l) = vj.layer_mut(layer) {
                l.volume = unit;
            }
        }
        Target::LayerOpacity { layer } => {
            if let Ok(l) = vj.layer_mut(layer) {
                l.opacity = unit;
            }
        }
        Target::GroupMaster { group } => {
            if let Ok(g) = vj.group_mut(group) {
                g.master = unit;
            }
        }
        Target::GroupOpacity { group } => {
            if let Ok(g) = vj.group_mut(group) {
                g.opacity = unit;
            }
        }
        Target::LayerTransition { layer } => {
            if let Ok(l) = vj.layer_mut(layer) {
                l.transition = value.clamp(0.0, MAX_TRANSITION);
            }
        }
        Target::ClipSpeed { clip } => {
            if let Ok(c) = vj.clip_mut(clip) {
                c.speed = value.clamp(MIN_SPEED, MAX_SPEED);
            }
        }
        Target::EffectParam { effect, param } => {
            if let Ok((stack, at)) = vj.effect_mut(effect)
                && let Some(kind) = stack[at].kind.with_numbers(&[(param.clone(), value)])
            {
                stack[at].kind = kind;
            }
        }
        Target::GeneratorNumber { clip, name } => {
            if let Ok(c) = vj.clip_mut(clip)
                && let Some(generator) = &mut c.generator
            {
                generator.set_modulated(name, value);
            }
        }
        // The tempo is the session's, not the grid's: a modulated knob cannot move it.
        _ => {}
    }
}

/// Moves what the knob `index` of `vj` is linked to, as if it stood at `value`.
pub(crate) fn drive_knob(vj: &mut Vj, index: usize, value: f32) {
    crate::vj_dashboard::drive(vj, &Scope::Composition, index, value);
}

pub(crate) fn rename_knob(vj: &mut Vj, knob: usize, name: Option<String>) -> Result<()> {
    crate::vj_dashboard::rename(vj, &Scope::Composition, knob, name)
}

/// Takes away the mappings and links whose target is gone.
pub fn prune(vj: &mut Vj) {
    let mappings = std::mem::take(&mut vj.mappings);
    vj.mappings = mappings
        .into_iter()
        .filter(|m| m.parked || m.target.exists(vj))
        .collect();
    let mut dashboard = std::mem::take(&mut vj.dashboard);
    for knob in &mut dashboard {
        knob.links.retain(|link| link.target.exists(vj));
    }
    vj.dashboard = dashboard;
    crate::vj_dashboard::prune(vj);
}

/// Checks the mappings and the dashboard: known targets, ranges, ids, modes.
pub(crate) fn validate(vj: &Vj) -> Result<()> {
    if vj.mappings.len() > MAX_MAPPINGS || vj.dashboard.len() > KNOBS {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_MAPPINGS} mappings and {KNOBS} knobs"
        )));
    }
    let mut ids = HashSet::new();
    for mapping in &vj.mappings {
        if !ids.insert(mapping.id.as_str()) {
            return Err(ProjectError::Invalid(format!(
                "duplicate mapping id: {}",
                mapping.id
            )));
        }
        mapping.source.validate()?;
        if let Some(lights) = mapping.lights {
            lights.validate()?;
        }
        let class = mapping.target.class();
        if mapping.parked && mapping.follow != Follow::Position
            || mapping.follow == Follow::Position && mapping.place.is_none()
        {
            return Err(ProjectError::Invalid(format!(
                "mapping {} follows a slot it does not name",
                mapping.id
            )));
        }
        if !mapping.parked && !mapping.target.exists(vj) {
            return Err(ProjectError::Invalid(format!(
                "mapping {} drives something that is not in the grid",
                mapping.id
            )));
        }
        if !mode_fits(class, mapping.mode) {
            return Err(ProjectError::Invalid(format!(
                "mapping {} toggles something that is not a switch",
                mapping.id
            )));
        }
        if class == Class::Continuous && !mapping.parked {
            let (lo, hi) = mapping.target.range(vj).unwrap_or((0.0, 1.0));
            let inside = |v: f32| v.is_finite() && (lo..=hi).contains(&v);
            if !inside(mapping.min) || !inside(mapping.max) {
                return Err(ProjectError::Invalid(format!(
                    "mapping {} has a range outside its target's",
                    mapping.id
                )));
            }
        }
    }
    for knob in &vj.dashboard {
        check_knob(
            vj,
            knob,
            linkable,
            "a knob is linked to a number that is not in the grid, or out of its range",
        )?;
    }
    Ok(())
}

/// Checks a knob: its value, its name, its modulation, and that each of its links takes a number
/// `allowed` that is inside the range of its target. `unlinked` is the error for a bad link.
pub(crate) fn check_knob(
    vj: &Vj,
    knob: &Knob,
    allowed: impl Fn(&Target) -> bool,
    unlinked: &str,
) -> Result<()> {
    if !(0.0..=1.0).contains(&knob.value) || knob.links.len() > MAX_LINKS {
        return Err(ProjectError::Invalid(
            "a knob is 0 to 1 and drives at most 16 numbers".into(),
        ));
    }
    crate::vj::check_name(knob.name.as_deref())?;
    if let Some(m) = &knob.modulation {
        m.validate(0.0, 1.0)?;
    }
    for link in &knob.links {
        let (lo, hi) = link.target.range(vj).unwrap_or((1.0, 0.0));
        let inside = |v: f32| v.is_finite() && (lo..=hi).contains(&v);
        if !allowed(&link.target) || !inside(link.min) || !inside(link.max) {
            return Err(ProjectError::Invalid(unlinked.into()));
        }
    }
    Ok(())
}
