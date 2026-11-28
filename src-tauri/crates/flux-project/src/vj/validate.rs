use super::*;

/// Checks the grid: unique ids (a clip's is unique among every item of the project, in `item_ids`),
/// a shown deck, each clip in a known column (a slot on a global layer), at most one clip per
/// cell, clips showing known videos, levels in range, a grid in a VJ project and none in a timeline
/// project.
pub(crate) fn validate<'a>(
    project: &'a Project,
    video_ids: &HashSet<&str>,
    item_ids: &mut HashSet<&'a str>,
) -> Result<()> {
    let vj = &project.vj;
    match project.mode {
        ProjectMode::Vj
            if vj.layers.is_empty()
                || vj.decks.is_empty()
                || vj.decks.iter().any(|deck| deck.columns.is_empty()) =>
        {
            return Err(ProjectError::Invalid(
                "a VJ project needs a layer and a deck with a column".into(),
            ));
        }
        ProjectMode::Vj if vj.shown_deck().is_none() => {
            return Err(ProjectError::Invalid(format!("no deck {}", vj.deck)));
        }
        // A VJ show is an app of its own: none of a cue show's content, so its clock plays nothing
        // but the grid. Its one timeline only carries the clock's tempo and timecode.
        ProjectMode::Vj
            if project.timelines.len() != 1
                || project
                    .timelines
                    .iter()
                    .any(|t| !t.tracks.is_empty() || !t.controls.is_empty())
                || !project.setlists.is_empty()
                || !project.assets.is_empty()
                || project.voice.is_some()
                || !project.output.ltc.is_empty()
                || !project.output.mtc.is_empty()
                || !project.output.osc.is_empty()
                || !project.output.audio.is_empty()
                || project.output.metronome.is_some() =>
        {
            return Err(ProjectError::Invalid(
                "a VJ show has no timeline tracks, audio, setlists or timecode outputs".into(),
            ));
        }
        ProjectMode::Timeline if !vj.is_empty() => {
            return Err(ProjectError::Invalid(
                "a timeline project has no VJ grid".into(),
            ));
        }
        _ => {}
    }
    if vj.layers.len() > MAX_LAYERS
        || vj.decks.len() > MAX_DECKS
        || vj.decks.iter().any(|deck| deck.columns.len() > MAX_COLUMNS)
    {
        return Err(ProjectError::Invalid(format!(
            "a grid has at most {MAX_LAYERS} layers and {MAX_DECKS} decks of {MAX_COLUMNS} columns"
        )));
    }
    let mut ids = HashSet::new();
    let mut columns = HashSet::new();
    for deck in &vj.decks {
        unique(&mut ids, &deck.id)?;
        check_name(deck.name.as_deref())?;
        for column in &deck.columns {
            unique(&mut ids, &column.id)?;
            columns.insert(column.id.as_str());
            check_name(column.name.as_deref())?;
        }
    }
    crate::vj_effects::validate_stack(&vj.composition_effects, true, item_ids)?;
    crate::vj_groups::validate(vj, &mut ids, item_ids)?;
    for layer in &vj.layers {
        unique(&mut ids, &layer.id)?;
        check_name(layer.name.as_deref())?;
        let levels = [layer.master, layer.opacity, layer.volume];
        if levels.iter().any(|v| !(0.0..=1.0).contains(v))
            || !(0.0..=MAX_TRANSITION).contains(&layer.transition)
        {
            return Err(ProjectError::Invalid(format!(
                "layer {} needs levels of 0 to 1 and a transition of 0 to {MAX_TRANSITION} s",
                layer.id
            )));
        }
        crate::vj_effects::validate_stack(&layer.effects, false, item_ids)?;
        crate::vj_modulation::validate_map(&layer.modulation, crate::vj_modulation::layer_range)?;
        let mut cells = HashSet::new();
        for clip in &layer.clips {
            if !item_ids.insert(&clip.id) {
                return Err(ProjectError::Invalid(format!(
                    "duplicate clip id: {}",
                    clip.id
                )));
            }
            let placed = match (clip.cell(), layer.global) {
                (Some(Cell::Column(column)), false) => columns.contains(column.as_str()),
                (Some(Cell::Slot(slot)), true) => slot < MAX_COLUMNS,
                _ => false,
            };
            if !placed {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is in no cell of its layer",
                    clip.id
                )));
            }
            if !cells.insert(clip.cell()) {
                return Err(ProjectError::Invalid(format!(
                    "layer {} has two clips in one cell",
                    layer.id
                )));
            }
            match &clip.generator {
                Some(generator) if clip.asset.is_empty() => generator.validate()?,
                Some(_) => {
                    return Err(ProjectError::Invalid(format!(
                        "clip {} is both a video and a generator",
                        clip.id
                    )));
                }
                None if !video_ids.contains(clip.asset.as_str()) => {
                    return Err(ProjectError::MissingAsset(clip.asset.clone()));
                }
                None => {}
            }
            check_name(clip.name.as_deref())?;
            check_transport(clip)?;
            crate::vj_effects::validate_stack(&clip.effects, false, item_ids)?;
            crate::vj_modulation::validate_map(&clip.modulation, |field| {
                crate::vj_modulation::clip_field_range(clip, field)
            })?;
            if !(0.0..=MAX_SCALE).contains(&clip.scale)
                || clip
                    .position
                    .iter()
                    .any(|v| !(-MAX_OFFSET..=MAX_OFFSET).contains(v))
            {
                return Err(ProjectError::Invalid(format!(
                    "clip {} needs a scale of 0 to {MAX_SCALE} and a position of -{MAX_OFFSET} to {MAX_OFFSET}",
                    clip.id
                )));
            }
        }
    }
    crate::vj_mapping::validate(vj)?;
    crate::vj_dashboard::validate(vj)?;
    Ok(())
}

/// The speed, tempo, region and cues of a clip are within their ranges.
fn check_transport(clip: &Clip) -> Result<()> {
    let tempo = match clip.tempo {
        Tempo::Free => true,
        Tempo::Beats { beats } => TEMPO_BEATS.contains(&beats),
    };
    let cues = clip
        .cues
        .iter()
        .flatten()
        .all(|cue| (0.0..=1.0).contains(cue));
    if !(MIN_SPEED..=MAX_SPEED).contains(&clip.speed)
        || !(MIN_DURATION..=MAX_DURATION).contains(&clip.duration)
        || !tempo
        || !clip.region.valid()
        || !cues
    {
        return Err(ProjectError::Invalid(format!(
            "clip {} needs a speed of {MIN_SPEED} to {MAX_SPEED}, a duration of {MIN_DURATION} to {MAX_DURATION} seconds, a length of 1 to 128 beats, a region of at least {MIN_REGION} inside the clip and cues inside the clip",
            clip.id
        )));
    }
    Ok(())
}

fn unique<'a>(seen: &mut HashSet<&'a str>, id: &'a str) -> Result<()> {
    if seen.insert(id) {
        Ok(())
    } else {
        Err(ProjectError::Invalid(format!("duplicate grid id: {id}")))
    }
}

pub(crate) fn check_name(name: Option<&str>) -> Result<()> {
    match name {
        Some(name) if name.trim().is_empty() || name.chars().count() > MAX_CLIP_NAME => {
            Err(ProjectError::Invalid(format!(
                "a grid name has 1 to {MAX_CLIP_NAME} characters; leave it out instead"
            )))
        }
        _ => Ok(()),
    }
}
