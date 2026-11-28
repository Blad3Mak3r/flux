use super::*;

/// What the source of a mapping did: the 0 to 1 it reads, and whether it is down.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub source: Source,
    pub level: f32,
    /// A note on, a control above 0, an OSC value above 0 (a bend, above half).
    pub down: bool,
}

/// How a button changed since the last message of its mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Press,
    Release,
    /// Nothing new: a knob sends many values above 0, a held button sends nothing.
    Same,
}

/// The edge of `down` for the mapping `id`, which `held` remembers.
pub fn edge(held: &mut HashSet<String>, id: &str, down: bool) -> Edge {
    match (held.contains(id), down) {
        (false, true) => {
            held.insert(id.to_owned());
            Edge::Press
        }
        (true, false) => {
            held.remove(id);
            Edge::Release
        }
        _ => Edge::Same,
    }
}

/// What a target is asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Go to this value, in the target's units.
    Value(f32),
    Switch(bool),
    Toggle,
    Press,
    Release,
}

/// The command a mapping gives for a source at `level` that changed by `edge`; `None` when it
/// asks for nothing (a release of a trigger).
pub fn command(mapping: &Mapping, level: f32, edge: Edge) -> Option<Command> {
    // A pad or a key is a button: its press is the level, its release the minimum if held.
    let is_note = matches!(
        mapping.source,
        Source::Midi {
            kind: MidiKind::Note,
            ..
        } | Source::Key { .. }
    );
    match mapping.target.class() {
        Class::Continuous => match (is_note, edge) {
            // A pad is a button: its velocity is the level, its release the minimum if held.
            (true, Edge::Press) => Some(Command::Value(mapping.scale(level))),
            (true, Edge::Release) if mapping.mode == ButtonMode::Hold => {
                Some(Command::Value(mapping.scale(0.0)))
            }
            (true, _) => None,
            (false, _) => Some(Command::Value(mapping.scale(level))),
        },
        Class::Switch => match (edge, mapping.mode) {
            (Edge::Press, ButtonMode::Toggle) => Some(Command::Toggle),
            (Edge::Press, _) => Some(Command::Switch(true)),
            (Edge::Release, ButtonMode::Hold) => Some(Command::Switch(false)),
            _ => None,
        },
        Class::Action => match edge {
            Edge::Press => Some(Command::Press),
            Edge::Release => Some(Command::Release),
            Edge::Same => None,
        },
    }
}

/// The commands of the mappings that `heard` is the source of. `held` remembers which buttons are
/// down, between calls.
pub fn react(vj: &Vj, heard: &Heard, held: &mut HashSet<String>) -> Vec<(Target, Command)> {
    vj.mappings
        .iter()
        .filter(|m| !m.parked && m.source.matches(&heard.source))
        .filter_map(|m| {
            let edge = edge(held, &m.id, heard.down);
            Some((m.target.clone(), command(m, heard.level, edge)?))
        })
        .collect()
}

/// Of consecutive values for one target keeps the last: a knob turned fast sends hundreds a
/// second, and only where it ended matters. Presses, releases and switches all stay, in order.
pub fn coalesce(commands: Vec<(Target, Command)>) -> Vec<(Target, Command)> {
    let mut out: Vec<(Target, Command)> = Vec::with_capacity(commands.len());
    for (target, command) in commands {
        if let Command::Value(_) = command
            && let Some((last, Command::Value(_))) = out.last()
            && *last == target
        {
            out.pop();
        }
        out.push((target, command));
    }
    out
}

/// What the app does to the playing grid (which is never stored).
#[derive(Debug, Clone, PartialEq)]
pub enum PlayAction {
    Launch(String),
    Release(String),
    LaunchColumn(String),
    /// A layer, or every layer.
    Stop(Option<String>),
    /// 0 is all side A, 1 all side B.
    Crossfader(f32),
    /// Holds the beats, or lets them go on; `None` flips.
    BeatPause(Option<bool>),
    Resync,
    /// Faster (1), slower (-1) or at the tempo (0).
    Nudge(i8),
}

/// What a command comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// An edit of the project, which enters the undo history.
    Edit(VjOp),
    Play(PlayAction),
    /// A tap of the tempo; the app keeps the times of the taps.
    Tap,
}

/// The edit that sets a continuous `target` to `value`, brought into range by the op.
pub fn value_op(vj: &Vj, target: &Target, value: f32) -> Option<VjOp> {
    let value = if value.is_finite() {
        value
    } else {
        return None;
    };
    Some(match target {
        Target::LayerMaster { layer } => VjOp::LayerMaster {
            layer: layer.clone(),
            value,
        },
        Target::LayerVolume { layer } => VjOp::LayerVolume {
            layer: layer.clone(),
            value,
        },
        Target::LayerOpacity { layer } => VjOp::LayerOpacity {
            layer: layer.clone(),
            value,
        },
        Target::GroupMaster { group } => VjOp::GroupMaster {
            group: group.clone(),
            value,
        },
        Target::GroupOpacity { group } => VjOp::GroupOpacity {
            group: group.clone(),
            value,
        },
        Target::LayerTransition { layer } => VjOp::LayerTransition {
            layer: layer.clone(),
            value,
        },
        Target::Bpm => VjOp::SetBpm {
            bpm: f64::from(value),
        },
        Target::ClipSpeed { clip } => VjOp::ClipSpeed {
            clip: clip.clone(),
            value,
        },
        Target::EffectParam { effect, param } => {
            let found = vj.find_effect(effect)?;
            VjOp::EffectParams {
                effect: effect.clone(),
                kind: found.kind.with_numbers(&[(param.clone(), value)])?,
            }
        }
        Target::GeneratorNumber { clip, name } => {
            let (_, found) = vj.find_clip(clip)?;
            let mut generator = found.generator.clone()?;
            generator.set_modulated(name, value);
            VjOp::ClipGenerator {
                clip: clip.clone(),
                generator,
            }
        }
        Target::Dashboard { knob } => VjOp::DashboardValue {
            scope: Scope::Composition,
            knob: *knob,
            value,
        },
        Target::LayerDashboard { layer, knob } => VjOp::DashboardValue {
            scope: Scope::Layer {
                layer: layer.clone(),
            },
            knob: *knob,
            value,
        },
        Target::ClipDashboard { clip, knob } => VjOp::DashboardValue {
            scope: Scope::Clip { clip: clip.clone() },
            knob: *knob,
            value,
        },
        _ => return None,
    })
}

/// The deck `step` places from the one shown, wrapping round.
fn deck_by(vj: &Vj, step: isize) -> Option<String> {
    let here = vj.decks.iter().position(|d| d.id == vj.deck)?;
    let n = vj.decks.len() as isize;
    let at = (here as isize + step).rem_euclid(n);
    vj.decks
        .get(usize::try_from(at).ok()?)
        .map(|d| d.id.clone())
}

/// What `command` asks of `target`, given the grid as it is now (a flip needs the current state).
pub fn plan(vj: &Vj, target: &Target, command: Command) -> Option<Plan> {
    use Command::{Press, Release, Switch, Toggle, Value};
    let layer_of = |id: &str| vj.layers.iter().find(|l| l.id == id);
    let edit = |op| Some(Plan::Edit(op));
    let play = |action| Some(Plan::Play(action));
    match (target, command) {
        (Target::Crossfader, Value(v)) => play(PlayAction::Crossfader(v.clamp(0.0, 1.0))),
        (t, Value(v)) if t.class() == Class::Continuous => value_op(vj, t, v).map(Plan::Edit),
        (Target::ClipLaunch { clip }, Press) => play(PlayAction::Launch(clip.clone())),
        (Target::ClipLaunch { clip }, Release) => play(PlayAction::Release(clip.clone())),
        (Target::ColumnLaunch { column }, Press) => play(PlayAction::LaunchColumn(column.clone())),
        (Target::LayerStop { layer }, Press) => play(PlayAction::Stop(Some(layer.clone()))),
        (Target::StopAll, Press) => play(PlayAction::Stop(None)),
        (Target::LayerAutopilot { layer }, Press) => edit(VjOp::LayerAutopilot {
            layer: layer.clone(),
            mode: layer_of(layer)?.autopilot.cycled(),
        }),
        (Target::DeckSelect { deck }, Press) => edit(VjOp::SelectDeck { deck: deck.clone() }),
        (Target::DeckNext, Press) => edit(VjOp::SelectDeck {
            deck: deck_by(vj, 1)?,
        }),
        (Target::DeckPrev, Press) => edit(VjOp::SelectDeck {
            deck: deck_by(vj, -1)?,
        }),
        (Target::BpmTap, Press) => Some(Plan::Tap),
        (Target::BpmResync, Press) => play(PlayAction::Resync),
        (Target::BpmPlus, Press) => edit(VjOp::NudgeBpm { by: 1.0 }),
        (Target::BpmMinus, Press) => edit(VjOp::NudgeBpm { by: -1.0 }),
        (Target::BpmDouble, Press) => edit(VjOp::ScaleBpm { factor: 2.0 }),
        (Target::BpmHalf, Press) => edit(VjOp::ScaleBpm { factor: 0.5 }),
        (Target::BpmNudgeUp, Press) => play(PlayAction::Nudge(1)),
        (Target::BpmNudgeDown, Press) => play(PlayAction::Nudge(-1)),
        (Target::BpmNudgeUp | Target::BpmNudgeDown, Release) => play(PlayAction::Nudge(0)),
        (Target::BpmPause, Switch(on)) => play(PlayAction::BeatPause(Some(on))),
        (Target::BpmPause, Toggle) => play(PlayAction::BeatPause(None)),
        (Target::LayerBypass { layer }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !layer_of(layer)?.bypass,
            };
            edit(VjOp::LayerBypass {
                layer: layer.clone(),
                on,
            })
        }
        (Target::LayerSolo { layer }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !layer_of(layer)?.solo,
            };
            edit(VjOp::LayerSolo {
                layer: layer.clone(),
                on,
            })
        }
        (Target::GroupBypass { group }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !vj.group(group)?.bypass,
            };
            edit(VjOp::GroupBypass {
                group: group.clone(),
                on,
            })
        }
        (Target::GroupSolo { group }, Switch(_) | Toggle) => {
            let on = match command {
                Switch(on) => on,
                _ => !vj.group(group)?.solo,
            };
            edit(VjOp::GroupSolo {
                group: group.clone(),
                on,
            })
        }
        (Target::EffectBypass { effect }, Switch(_) | Toggle) => {
            let bypass = match command {
                Switch(on) => on,
                _ => !vj.find_effect(effect)?.bypass,
            };
            edit(VjOp::EffectBypass {
                effect: effect.clone(),
                bypass,
            })
        }
        _ => None,
    }
}
