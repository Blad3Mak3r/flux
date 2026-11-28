use super::*;

/// The number an OSC message carries: an integer, a float, a boolean or a string with a number.
pub fn osc_level(args: &[OscArg]) -> Option<f32> {
    let value = match args.first()? {
        OscArg::Int(v) => *v as f32,
        OscArg::Float(v) => *v,
        OscArg::Bool(v) => f32::from(u8::from(*v)),
        OscArg::String(text) => text.trim().parse().ok()?,
    };
    value.is_finite().then_some(value)
}

/// A place counted from 1 in an address, as an index.
pub(super) fn place(segment: &str) -> Option<usize> {
    segment.parse::<usize>().ok()?.checked_sub(1)
}

/// The knob an address segment `link3` names, from 0.
fn knob_at(segment: &str) -> Option<usize> {
    let knob = place(segment.strip_prefix("link")?)?;
    (knob < KNOBS).then_some(knob)
}

/// The deck layer's or global layer's clip at the place `column` (from 0) of the deck shown.
fn clip_at(vj: &Vj, layer: usize, column: usize) -> Option<String> {
    let layer = vj.layers.get(layer)?;
    let deck = vj.shown_deck()?;
    let column_id = &deck.columns.get(column)?.id;
    layer
        .clips
        .iter()
        .find(|clip| {
            if layer.global {
                clip.slot == Some(column)
            } else {
                clip.column.as_deref() == Some(column_id)
            }
        })
        .map(|clip| clip.id.clone())
}

/// The target and command of an OSC message to one of the fixed addresses, which count from 1 by
/// position as Resolume does (layers from the bottom, columns of the deck shown from the left):
///
/// | Address | Value | Does |
/// | --- | --- | --- |
/// | `/composition/layers/{n}/clips/{m}/connect` | 1 or 0 | launches (releases) the clip |
/// | `/composition/columns/{m}/connect` | 1 | launches the column |
/// | `/composition/layers/{n}/video/opacity` | 0 to 1 | opacity |
/// | `/composition/layers/{n}/master` | 0 to 1 | master |
/// | `/composition/layers/{n}/audio/volume` | 0 to 1 | audio level |
/// | `/composition/layers/{n}/transition/duration` | 0 to 1 | transition time (0 to 10 s) |
/// | `/composition/layers/{n}/bypassed`, `/solo` | 0 or 1 | sets the switch |
/// | `/composition/layers/{n}/clear` | 1 | stops the layer |
/// | `/composition/groups/{g}/video/opacity` | 0 to 1 | the group's opacity |
/// | `/composition/groups/{g}/master` | 0 to 1 | the group's master |
/// | `/composition/groups/{g}/bypassed`, `/solo` | 0 or 1 | sets the group's switch |
/// | `/composition/crossfader/phase` | 0 to 1 | the crossfader |
/// | `/composition/tempocontroller/tempo` | beats a minute | the tempo |
/// | `/composition/tempocontroller/{tempotap,resync,plus,minus,double,half}` | 1 | tempo buttons |
/// | `/composition/tempocontroller/pause` | 0 or 1 | holds the beats |
/// | `/composition/decks/{d}/select` | 1 | shows the deck |
/// | `/composition/decks/next`, `/composition/decks/prev` | 1 | next or previous deck |
/// | `/composition/disconnectall` | 1 | stops every layer |
/// | `/composition/dashboard/link{k}` | 0 to 1 | turns knob `k` |
/// | `/composition/layers/{n}/dashboard/link{k}` | 0 to 1 | turns knob `k` of the layer's dashboard |
/// | `/composition/layers/{n}/clips/{m}/dashboard/link{k}` | 0 to 1 | turns knob `k` of the clip's dashboard |
///
/// The tempo addresses also work without `/composition`. A button address with no value is a
/// press, with a value above 0 too, and with 0 a release.
pub fn osc_command(vj: &Vj, address: &str, args: &[OscArg]) -> Option<(Target, Command)> {
    let level = osc_level(args);
    let parts: Vec<&str> = address.strip_prefix('/')?.split('/').collect();
    let layer_id = |n: &str| Some(vj.layers.get(place(n)?)?.id.clone());
    // A button: pressed unless the value is 0.
    let button = |target: Target| {
        let command = if level.is_some_and(|v| v <= 0.0) {
            Command::Release
        } else {
            Command::Press
        };
        Some((target, command))
    };
    // A fader: the 0 to 1 of the address, into the whole range of the target.
    let fader = |target: Target| {
        let (lo, hi) = target.range(vj)?;
        Some((
            target,
            Command::Value(lo + (hi - lo) * level?.clamp(0.0, 1.0)),
        ))
    };
    let switch = |target: Target| Some((target, Command::Switch(level? >= 0.5)));
    let group_id = |n: &str| Some(vj.groups.get(place(n)?)?.id.clone());
    let parts = match parts.as_slice() {
        ["composition", rest @ ..] => rest,
        ["tempocontroller", ..] => parts.as_slice(),
        _ => return None,
    };
    match parts {
        ["layers", n, "clips", m, "connect"] => {
            let clip = clip_at(vj, place(n)?, place(m)?)?;
            button(Target::ClipLaunch { clip })
        }
        ["columns", m, "connect"] => {
            let column = vj.shown_deck()?.columns.get(place(m)?)?.id.clone();
            button(Target::ColumnLaunch { column })
        }
        ["layers", n, "video", "opacity"] => fader(Target::LayerOpacity {
            layer: layer_id(n)?,
        }),
        ["layers", n, "master"] => fader(Target::LayerMaster {
            layer: layer_id(n)?,
        }),
        ["layers", n, "audio", "volume"] => fader(Target::LayerVolume {
            layer: layer_id(n)?,
        }),
        ["layers", n, "transition", "duration"] => fader(Target::LayerTransition {
            layer: layer_id(n)?,
        }),
        ["layers", n, "dashboard", link] => fader(Target::LayerDashboard {
            layer: layer_id(n)?,
            knob: knob_at(link)?,
        }),
        ["layers", n, "clips", m, "dashboard", link] => fader(Target::ClipDashboard {
            clip: clip_at(vj, place(n)?, place(m)?)?,
            knob: knob_at(link)?,
        }),
        ["layers", n, "bypassed"] => switch(Target::LayerBypass {
            layer: layer_id(n)?,
        }),
        ["layers", n, "solo"] => switch(Target::LayerSolo {
            layer: layer_id(n)?,
        }),
        ["layers", n, "clear"] => button(Target::LayerStop {
            layer: layer_id(n)?,
        }),
        ["groups", n, "video", "opacity"] => fader(Target::GroupOpacity {
            group: group_id(n)?,
        }),
        ["groups", n, "master"] => fader(Target::GroupMaster {
            group: group_id(n)?,
        }),
        ["groups", n, "bypassed"] => switch(Target::GroupBypass {
            group: group_id(n)?,
        }),
        ["groups", n, "solo"] => switch(Target::GroupSolo {
            group: group_id(n)?,
        }),
        ["crossfader", "phase"] => fader(Target::Crossfader),
        ["tempocontroller", "tempo"] => Some((
            Target::Bpm,
            Command::Value(level?.clamp(MIN_BPM as f32, MAX_BPM as f32)),
        )),
        ["tempocontroller", "tempotap"] => button(Target::BpmTap),
        ["tempocontroller", "resync"] => button(Target::BpmResync),
        ["tempocontroller", "plus"] => button(Target::BpmPlus),
        ["tempocontroller", "minus"] => button(Target::BpmMinus),
        ["tempocontroller", "double"] => button(Target::BpmDouble),
        ["tempocontroller", "half"] => button(Target::BpmHalf),
        ["tempocontroller", "pause"] => switch(Target::BpmPause),
        ["decks", d, "select"] => {
            let deck = vj.decks.get(place(d)?)?.id.clone();
            button(Target::DeckSelect { deck })
        }
        ["decks", "next"] => button(Target::DeckNext),
        ["decks", "prev"] => button(Target::DeckPrev),
        ["disconnectall"] => button(Target::StopAll),
        ["dashboard", link] => fader(Target::Dashboard {
            knob: knob_at(link)?,
        }),
        _ => None,
    }
}
