use super::*;

/// A level as a fader stands, 0 to 1.
pub(super) fn level(value: f32) -> Result<f32> {
    within(value, 0.0, 1.0)
}

pub(super) fn within(value: f32, min: f32, max: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value.clamp(min, max))
    } else {
        Err(ProjectError::Invalid("a level is a number".into()))
    }
}

/// The length in [`TEMPO_BEATS`] nearest to `beats`.
fn nearest_beats(beats: u32) -> u32 {
    TEMPO_BEATS
        .into_iter()
        .min_by_key(|length| length.abs_diff(beats))
        .unwrap_or(1)
}

/// Applies `op` to a VJ project; `new_id` names whatever it creates. The caller validates the result.
pub fn apply(project: &mut Project, op: VjOp, mut new_id: impl FnMut() -> String) -> Result<()> {
    if project.mode != ProjectMode::Vj {
        return Err(ProjectError::Invalid("only a VJ project has a grid".into()));
    }
    let prune = op.may_remove();
    let vj = &mut project.vj;
    match op {
        VjOp::AddLayer { global } => {
            if vj.layers.len() >= MAX_LAYERS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_LAYERS} layers"
                )));
            }
            vj.layers.push(Layer::new(new_id(), global));
        }
        VjOp::RemoveLayer { layer } => {
            let index = layer_index(vj, &layer)?;
            if vj.layers.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a layer".into()));
            }
            vj.layers.remove(index);
        }
        VjOp::MoveLayer { layer, to } => {
            let index = layer_index(vj, &layer)?;
            let moved = vj.layers.remove(index);
            vj.layers.insert(to.min(vj.layers.len()), moved);
            crate::vj_groups::settle_moved(vj, &layer);
        }
        VjOp::RenameLayer { layer, name } => vj.layer_mut(&layer)?.name = typed_name(&name)?,
        VjOp::LayerMaster { layer, value } => vj.layer_mut(&layer)?.master = level(value)?,
        VjOp::LayerOpacity { layer, value } => vj.layer_mut(&layer)?.opacity = level(value)?,
        VjOp::LayerVolume { layer, value } => vj.layer_mut(&layer)?.volume = level(value)?,
        VjOp::LayerLevelReset { layer, level } => {
            let new = Layer::new(String::new(), false);
            let layer = vj.layer_mut(&layer)?;
            match level {
                Level::Master => layer.master = new.master,
                Level::Opacity => layer.opacity = new.opacity,
                Level::Volume => layer.volume = new.volume,
                Level::Transition => layer.transition = new.transition,
            }
        }
        VjOp::LayerBlend { layer, blend } => vj.layer_mut(&layer)?.blend = blend,
        VjOp::LayerTransition { layer, value } => {
            vj.layer_mut(&layer)?.transition = level(value / MAX_TRANSITION)? * MAX_TRANSITION;
        }
        VjOp::LayerTransitionEffect {
            layer,
            effect,
            random,
        } => {
            let layer = vj.layer_mut(&layer)?;
            layer.transition_effect = effect;
            layer.transition_random = random;
        }
        VjOp::LayerCrossfade { layer, side } => vj.layer_mut(&layer)?.crossfade = side,
        VjOp::LayerAutopilot { layer, mode } => vj.layer_mut(&layer)?.autopilot = mode,
        VjOp::LayerBypass { layer, on } => vj.layer_mut(&layer)?.bypass = on,
        VjOp::LayerSolo { layer, on } => vj.layer_mut(&layer)?.solo = on,
        VjOp::LayerCollapse { layer, on } => vj.layer_mut(&layer)?.collapsed = on,
        VjOp::GroupLayers { layers } => crate::vj_groups::group_layers(vj, &layers, new_id())?,
        VjOp::LayerGroup { layer, group, to } => {
            crate::vj_groups::layer_group(vj, &layer, group, to, new_id())?;
        }
        VjOp::Ungroup { group } => crate::vj_groups::ungroup(vj, &group)?,
        VjOp::RenameGroup { group, name } => vj.group_mut(&group)?.name = typed_name(&name)?,
        VjOp::GroupMaster { group, value } => vj.group_mut(&group)?.master = level(value)?,
        VjOp::GroupOpacity { group, value } => vj.group_mut(&group)?.opacity = level(value)?,
        VjOp::GroupLevelReset { group, level } => {
            let new = crate::vj_groups::Group::new(String::new());
            let group = vj.group_mut(&group)?;
            match level {
                Level::Master => group.master = new.master,
                Level::Opacity => group.opacity = new.opacity,
                _ => return Err(ProjectError::Invalid("a group has no such fader".into())),
            }
        }
        VjOp::GroupBlend { group, blend } => vj.group_mut(&group)?.blend = blend,
        VjOp::GroupBypass { group, on } => vj.group_mut(&group)?.bypass = on,
        VjOp::GroupSolo { group, on } => vj.group_mut(&group)?.solo = on,
        VjOp::GroupCollapse { group, on } => vj.group_mut(&group)?.collapsed = on,
        VjOp::SetClock { source } => vj.clock = source,
        VjOp::SetLink { on } => vj.link = on,
        VjOp::SetBpm { bpm } => set_bpm(&mut project.timelines, |_| bpm)?,
        VjOp::NudgeBpm { by } => set_bpm(&mut project.timelines, |now| now + by)?,
        VjOp::ScaleBpm { factor } => set_bpm(&mut project.timelines, |now| now * factor)?,
        VjOp::TapBpm { taps } => {
            let tapped = tapped_bpm(&taps)
                .ok_or_else(|| ProjectError::Invalid("tap at least twice".into()))?;
            set_bpm(&mut project.timelines, |_| tapped)?;
        }
        VjOp::AddDeck => {
            if vj.decks.len() >= MAX_DECKS {
                return Err(ProjectError::Invalid(format!(
                    "a grid has at most {MAX_DECKS} decks"
                )));
            }
            let deck = new_deck(&mut new_id);
            vj.deck = deck.id.clone();
            vj.decks.push(deck);
        }
        VjOp::RemoveDeck { deck } => {
            let index = vj.deck_index(&deck)?;
            if vj.decks.len() == 1 {
                return Err(ProjectError::Invalid("the grid needs a deck".into()));
            }
            let removed = vj.decks.remove(index);
            let gone: HashSet<&str> = removed.columns.iter().map(|c| c.id.as_str()).collect();
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| !clip.column.as_deref().is_some_and(|c| gone.contains(c)));
            }
            if vj.deck == deck {
                vj.deck = vj.decks[index.min(vj.decks.len() - 1)].id.clone();
            }
        }
        VjOp::MoveDeck { deck, to } => {
            let index = vj.deck_index(&deck)?;
            let moved = vj.decks.remove(index);
            vj.decks.insert(to.min(vj.decks.len()), moved);
        }
        VjOp::RenameDeck { deck, name } => {
            let index = vj.deck_index(&deck)?;
            vj.decks[index].name = typed_name(&name)?;
        }
        VjOp::SelectDeck { deck } => {
            vj.deck_index(&deck)?;
            vj.deck = deck;
        }
        VjOp::AddColumn => {
            let index = vj.deck_index(&vj.deck.clone())?;
            let deck = &mut vj.decks[index];
            if deck.columns.len() >= MAX_COLUMNS {
                return Err(ProjectError::Invalid(format!(
                    "a deck has at most {MAX_COLUMNS} columns"
                )));
            }
            deck.columns.push(Column {
                id: new_id(),
                name: None,
            });
        }
        VjOp::RemoveColumn { column } => {
            let (deck, at) = vj.column_index(&column)?;
            if vj.decks[deck].columns.len() == 1 {
                return Err(ProjectError::Invalid("a deck needs a column".into()));
            }
            vj.decks[deck].columns.remove(at);
            for layer in &mut vj.layers {
                layer
                    .clips
                    .retain(|clip| clip.column.as_deref() != Some(&column));
            }
        }
        VjOp::MoveColumn { column, to } => {
            let (deck, at) = vj.column_index(&column)?;
            let columns = &mut vj.decks[deck].columns;
            let moved = columns.remove(at);
            columns.insert(to.min(columns.len()), moved);
        }
        VjOp::RenameColumn { column, name } => {
            let (deck, at) = vj.column_index(&column)?;
            vj.decks[deck].columns[at].name = typed_name(&name)?;
        }
        VjOp::PlaceVideo {
            asset,
            layer,
            column,
        } => {
            if !project.videos.iter().any(|video| video.id == asset) {
                return Err(ProjectError::MissingAsset(asset));
            }
            place_clip(vj, &layer, &column, new_clip(new_id(), asset, None))?;
        }
        VjOp::PlaceGenerator {
            generator,
            layer,
            column,
        } => {
            let clip = new_clip(new_id(), String::new(), Some(generator.tidy()));
            place_clip(vj, &layer, &column, clip)?;
        }
        VjOp::MoveClip {
            clip,
            layer,
            column,
        } => {
            let to = vj.cell_of(&layer, &column)?;
            let (from_layer, from) = vj
                .layers
                .iter()
                .find_map(|l| {
                    let c = l.clips.iter().find(|c| c.id == clip)?;
                    Some((l.id.clone(), c.cell()?))
                })
                .ok_or_else(|| ProjectError::Invalid(format!("no clip {clip}")))?;
            let moved = take_clip(vj, &from_layer, |c| c.id == clip);
            let occupant = take_clip(vj, &layer, |c| c.cell().as_ref() == Some(&to));
            if let Some(mut occupant) = occupant {
                occupant.put(from);
                vj.layer_mut(&from_layer)?.clips.push(occupant);
            }
            if let Some(mut moved) = moved {
                moved.put(to);
                vj.layer_mut(&layer)?.clips.push(moved);
            }
        }
        VjOp::RemoveClip { clip } => {
            vj.clip_mut(&clip)?;
            for layer in &mut vj.layers {
                layer.clips.retain(|c| c.id != clip);
            }
        }
        VjOp::RenameClip { clip, name } => vj.clip_mut(&clip)?.name = typed_name(&name)?,
        VjOp::ClipPlayback { clip, playback } => vj.clip_mut(&clip)?.playback = playback,
        VjOp::ClipSpeed { clip, value } => {
            vj.clip_mut(&clip)?.speed = within(value, MIN_SPEED, MAX_SPEED)?;
        }
        VjOp::ClipDuration { clip, value } => {
            vj.clip_mut(&clip)?.duration = within(value, MIN_DURATION, MAX_DURATION)?;
        }
        VjOp::ClipDirection { clip, direction } => vj.clip_mut(&clip)?.direction = direction,
        VjOp::ClipTempo { clip, tempo } => {
            vj.clip_mut(&clip)?.tempo = match tempo {
                Tempo::Free => Tempo::Free,
                Tempo::Beats { beats } => Tempo::Beats {
                    beats: nearest_beats(beats),
                },
            };
        }
        VjOp::ClipRegion { clip, start, end } => {
            vj.clip_mut(&clip)?.region = Region::tidy(start, end)?;
        }
        VjOp::ClipCue {
            clip,
            index,
            position,
        } => {
            let clip = vj.clip_mut(&clip)?;
            let slot = clip
                .cues
                .get_mut(index)
                .ok_or_else(|| ProjectError::Invalid(format!("a clip has {CUES} cues")))?;
            *slot = position.map(|p| within(p, 0.0, 1.0)).transpose()?;
        }
        VjOp::ClipGenerator { clip, generator } => {
            let clip = vj.clip_mut(&clip)?;
            if clip.generator.is_none() {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is a video, not a generator",
                    clip.id
                )));
            }
            let generator = generator.tidy();
            // A modulation of a number the new kind of generator does not have goes with it.
            clip.modulation.retain(|field, _| {
                field
                    .strip_prefix(crate::vj_modulation::GENERATOR_FIELD)
                    .is_none_or(|name| generator.numbers().contains(&name))
            });
            clip.generator = Some(generator);
        }
        VjOp::TextLine { clip, step } => {
            let clip = vj.clip_mut(&clip)?;
            let Some(Generator::Text { text, line, .. }) = &mut clip.generator else {
                return Err(ProjectError::Invalid(format!(
                    "clip {} is not a text block",
                    clip.id
                )));
            };
            *line = crate::generator::step_line(text, *line, step);
        }
        VjOp::ClipTrigger { clip, trigger } => vj.clip_mut(&clip)?.trigger = trigger,
        VjOp::ClipFit { clip, fit } => vj.clip_mut(&clip)?.fit = fit,
        VjOp::ClipScale { clip, value } => {
            vj.clip_mut(&clip)?.scale = within(value, 0.0, MAX_SCALE)?;
        }
        VjOp::ClipPosition { clip, x, y } => {
            let offset = |v| within(v, -MAX_OFFSET, MAX_OFFSET);
            vj.clip_mut(&clip)?.position = [offset(x)?, offset(y)?];
        }
        VjOp::AddEffect { target, kind } => {
            let kind = EffectKind::default_of(&kind)?;
            if kind.composition_only() && target != EffectTarget::Composition {
                return Err(ProjectError::Invalid(format!(
                    "a {} effect only works on the composition",
                    kind.name()
                )));
            }
            let stack = match target {
                EffectTarget::Clip(clip) => &mut vj.clip_mut(&clip)?.effects,
                EffectTarget::Layer(layer) => &mut vj.layer_mut(&layer)?.effects,
                EffectTarget::Group(group) => &mut vj.group_mut(&group)?.effects,
                EffectTarget::Composition => &mut vj.composition_effects,
            };
            if stack.len() >= MAX_EFFECTS {
                return Err(ProjectError::Invalid(format!(
                    "a stack has at most {MAX_EFFECTS} effects"
                )));
            }
            stack.push(Effect {
                id: new_id(),
                bypass: false,
                modulation: BTreeMap::new(),
                kind,
            });
        }
        VjOp::ApplyEffectPreset { effect, preset } => {
            crate::vj_presets::apply_to_effect(vj, &effect, &preset)?;
        }
        VjOp::AddEffectPreset { target, preset } => {
            crate::vj_presets::add_to_stack(vj, &target, &preset, new_id)?;
        }
        VjOp::CompositionBypass { bypass } => vj.composition_bypass = bypass,
        VjOp::RemoveEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack.remove(at);
        }
        VjOp::MoveEffect { effect, index } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            let moved = stack.remove(at);
            stack.insert(index.min(stack.len()), moved);
        }
        VjOp::EffectParams { effect, kind } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            if !stack[at].kind.same_kind(&kind) {
                return Err(ProjectError::Invalid(format!(
                    "effect {effect} is a {}, not a {}",
                    stack[at].kind.name(),
                    kind.name()
                )));
            }
            stack[at].kind = kind.tidy()?;
        }
        VjOp::EffectBypass { effect, bypass } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack[at].bypass = bypass;
        }
        VjOp::DuplicateEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            if stack.len() >= MAX_EFFECTS {
                return Err(ProjectError::Invalid(format!(
                    "a stack has at most {MAX_EFFECTS} effects"
                )));
            }
            let copy = Effect {
                id: new_id(),
                ..stack[at].clone()
            };
            stack.insert(at + 1, copy);
        }
        VjOp::ResetEffect { effect } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            stack[at].kind = EffectKind::default_of(&stack[at].kind.name())?;
            stack[at].modulation.clear();
        }
        VjOp::SetModulation { target, modulation } => set_modulation(vj, target, modulation)?,
        VjOp::AddMapping { source, target } => {
            crate::vj_mapping::add_mapping(vj, source, target, new_id())?;
        }
        VjOp::RemoveMapping { mapping } => crate::vj_mapping::remove_mapping(vj, &mapping)?,
        VjOp::ClearMapping { target } => vj.mappings.retain(|m| m.target != target),
        VjOp::SetMapping {
            mapping,
            min,
            max,
            invert,
            mode,
        } => crate::vj_mapping::set_mapping(vj, &mapping, min, max, invert, mode)?,
        VjOp::SetMappingFollow { mapping, follow } => {
            crate::vj_mapping::set_mapping_follow(vj, &mapping, follow)?;
        }
        VjOp::SetMappingLights { mapping, lights } => {
            crate::vj_mapping::set_mapping_lights(vj, &mapping, lights)?;
        }
        VjOp::SetLights { lights } => {
            if let Some(lights) = lights {
                lights.validate()?;
            }
            vj.lights = lights;
        }
        VjOp::SetOscFeedback { destinations } => {
            vj.osc_feedback = crate::vj_osc_feedback::parse_destinations(&destinations)?;
        }
        VjOp::SetMappingFine { mapping, fine } => {
            crate::vj_mapping::set_mapping_fine(vj, &mapping, fine)?;
        }
        VjOp::DashboardValue { scope, knob, value } => {
            crate::vj_dashboard::turn(project, &scope, knob, value)?;
        }
        VjOp::DashboardLink {
            scope,
            knob,
            target,
            min,
            max,
        } => crate::vj_dashboard::link(vj, &scope, knob, target, min, max)?,
        VjOp::DashboardUnlink {
            scope,
            knob,
            target,
        } => crate::vj_dashboard::unlink(vj, &scope, knob, &target)?,
        VjOp::RenameKnob { scope, knob, name } => {
            let name = typed_name(&name)?;
            crate::vj_dashboard::rename(vj, &scope, knob, name)?;
        }
    }
    project.vj.tidy_groups();
    // Position mappings follow their slot, after whatever moved things.
    crate::vj_mapping::rebind(&mut project.vj);
    if prune {
        crate::vj_mapping::prune(&mut project.vj);
    }
    Ok(())
}

/// Puts `clip` in the cell of `layer` that `column` names, in place of the clip that was there.
fn place_clip(vj: &mut Vj, layer: &str, column: &str, mut clip: Clip) -> Result<()> {
    let cell = vj.cell_of(layer, column)?;
    let layer = vj.layer_mut(layer)?;
    layer
        .clips
        .retain(|clip| clip.cell().as_ref() != Some(&cell));
    clip.put(cell);
    layer.clips.push(clip);
    Ok(())
}

fn layer_index(vj: &Vj, id: &str) -> Result<usize> {
    vj.layers
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no layer {id}")))
}

fn take_clip(vj: &mut Vj, layer: &str, which: impl Fn(&Clip) -> bool) -> Option<Clip> {
    let layer = vj.layers.iter_mut().find(|l| l.id == layer)?;
    let index = layer.clips.iter().position(which)?;
    Some(layer.clips.remove(index))
}

/// Sets or takes away the modulation `target` names, brought into the range of the setting.
fn set_modulation(vj: &mut Vj, target: ModTarget, modulation: Option<Modulation>) -> Result<()> {
    match target {
        ModTarget::Knob { knob } => {
            return crate::vj_dashboard::modulate(vj, &Scope::Composition, knob, modulation);
        }
        ModTarget::LayerKnob { layer, knob } => {
            return crate::vj_dashboard::modulate(vj, &Scope::Layer { layer }, knob, modulation);
        }
        ModTarget::ClipKnob { clip, knob } => {
            return crate::vj_dashboard::modulate(vj, &Scope::Clip { clip }, knob, modulation);
        }
        _ => {}
    }
    let (map, key, (lo, hi)) = match target {
        ModTarget::Effect { effect, param } => {
            let (stack, at) = vj.effect_mut(&effect)?;
            let range = stack[at].kind.modulatable_range(&param)?;
            (&mut stack[at].modulation, param, range)
        }
        ModTarget::Layer { layer, field } => {
            let range = crate::vj_modulation::layer_range(&field)
                .ok_or_else(|| ProjectError::Invalid(format!("{field} cannot be modulated")))?;
            (&mut vj.layer_mut(&layer)?.modulation, field, range)
        }
        ModTarget::Clip { clip, field } => {
            let clip = vj.clip_mut(&clip)?;
            let range = crate::vj_modulation::clip_field_range(clip, &field)
                .ok_or_else(|| ProjectError::Invalid(format!("{field} cannot be modulated")))?;
            (&mut clip.modulation, field, range)
        }
        ModTarget::Knob { .. } | ModTarget::LayerKnob { .. } | ModTarget::ClipKnob { .. } => {
            unreachable!("a knob is handled above")
        }
    };
    match modulation {
        Some(modulation) => {
            map.insert(key, modulation.tidy(lo, hi)?);
        }
        None => {
            map.remove(&key);
        }
    }
    Ok(())
}

/// A typed name, or `None` for an empty one (the default name).
fn typed_name(name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.chars().count() > MAX_CLIP_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name has at most {MAX_CLIP_NAME} characters"
        )));
    }
    Ok((!name.is_empty()).then(|| name.to_owned()))
}
