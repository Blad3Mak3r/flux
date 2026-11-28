use super::*;

/// The kind of MIDI message a control sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MidiKind {
    /// A control change: a knob, a fader or a button.
    Cc,
    /// A pad or a key: 0 is released.
    Note,
    /// A wheel or a touch strip, 14 bits; its number is 0.
    PitchBend,
}

/// What drives a target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Source {
    Midi {
        /// A hint of the name of the input port the control was learned on, as the project keeps
        /// those of its outputs; absent matches any port.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        port: Option<String>,
        /// From 1 to 16.
        channel: u8,
        kind: MidiKind,
        /// The control, note or program number, 0 to 127.
        number: u8,
    },
    Osc {
        address: String,
    },
    /// A DMX channel of an Art-Net universe, as a console sends it: 0 to 255, or 0 to 65535 with
    /// a `fine` channel beside it.
    Artnet {
        /// The 15-bit Port-Address (net, sub-net and universe), as on the wire.
        universe: u16,
        /// From 1 to 512.
        channel: u16,
        /// The channel of the low byte of a 16-bit value, from 1 to 512; absent is 8 bits.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fine: Option<u16>,
    },
    /// A key of the computer's keyboard, by its position (`KeyboardEvent.code`, whatever the
    /// layout), with the modifiers held.
    Key {
        code: String,
        #[serde(default, skip_serializing_if = "is_false")]
        ctrl: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        alt: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        shift: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        meta: bool,
    },
}

/// Most channels a DMX universe has.
pub const DMX_CHANNELS: u16 = 512;

/// The largest Port-Address of Art-Net (15 bits).
pub const MAX_UNIVERSE: u16 = 0x7FFF;

/// Longest key code a mapping keeps.
const MAX_CODE: usize = 32;

/// A key the app uses itself: `cmd` is Ctrl (Cmd on macOS); the file's `action` is the window's.
#[derive(Deserialize)]
struct AppShortcut {
    code: String,
    cmd: bool,
    shift: bool,
}

/// The list the window reads too, so the menus, the keys and the refusal cannot drift apart.
fn app_shortcuts() -> &'static [AppShortcut] {
    static LIST: OnceLock<Vec<AppShortcut>> = OnceLock::new();
    LIST.get_or_init(|| {
        serde_json::from_str(include_str!("../vj_shortcuts.json")).unwrap_or_default()
    })
}

impl Source {
    /// Whether this is a key the app uses itself, so a mapping would fight its shortcuts: one of
    /// the combinations in `vj_shortcuts.json` (the list the window reads too; Resolume's
    /// defaults: Ctrl or Cmd with N, O, S, Z, Y, K, L, G, T, Ctrl or Cmd and Shift with S, C, K, M,
    /// O, X, A, D, L, P, 1 to 9, and Esc), or a modifier alone. Every other key is free.
    pub fn is_app_shortcut(&self) -> bool {
        let Self::Key {
            code,
            ctrl,
            alt,
            shift,
            meta,
        } = self
        else {
            return false;
        };
        if ["Control", "Meta", "Shift", "Alt", "OS"]
            .iter()
            .any(|modifier| code.starts_with(modifier))
        {
            return true;
        }
        !*alt
            && app_shortcuts()
                .iter()
                .any(|s| s.code == *code && s.cmd == (*ctrl || *meta) && s.shift == *shift)
    }

    /// Whether what was heard (`heard`, with the port it came from) is this source. A mapping
    /// without a port hint, or a message without a port, matches any port; otherwise the names
    /// agree, ignoring case.
    pub fn matches(&self, heard: &Source) -> bool {
        match (self, heard) {
            (
                Self::Midi {
                    port,
                    channel,
                    kind,
                    number,
                },
                Self::Midi {
                    port: heard_port,
                    channel: heard_channel,
                    kind: heard_kind,
                    number: heard_number,
                },
            ) => {
                channel == heard_channel
                    && kind == heard_kind
                    && number == heard_number
                    && match (port, heard_port) {
                        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                        _ => true,
                    }
            }
            (Self::Osc { address }, Self::Osc { address: heard }) => address == heard,
            // A DMX or key source is heard as exactly itself.
            (a @ Self::Artnet { .. }, b @ Self::Artnet { .. })
            | (a @ Self::Key { .. }, b @ Self::Key { .. }) => a == b,
            _ => false,
        }
    }

    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Midi {
                channel,
                kind,
                number,
                port,
            } => {
                let number_ok = match kind {
                    MidiKind::PitchBend => *number == 0,
                    _ => *number < 128,
                };
                if !(1..=16).contains(channel)
                    || !number_ok
                    || port.as_ref().is_some_and(|p| p.trim().is_empty())
                {
                    return Err(ProjectError::Invalid(
                        "a MIDI source needs a channel of 1 to 16 and a number of 0 to 127".into(),
                    ));
                }
            }
            Self::Osc { address } => {
                if !address.starts_with('/') || address.len() > MAX_ADDRESS {
                    return Err(ProjectError::Invalid(
                        "an OSC source is an address that starts with /".into(),
                    ));
                }
            }
            Self::Artnet {
                universe,
                channel,
                fine,
            } => {
                let channel_ok = |c: u16| (1..=DMX_CHANNELS).contains(&c);
                if *universe > MAX_UNIVERSE
                    || !channel_ok(*channel)
                    || fine.is_some_and(|f| !channel_ok(f) || f == *channel)
                {
                    return Err(ProjectError::Invalid(
                        "a DMX source needs a universe of 0 to 32767 and channels of 1 to 512"
                            .into(),
                    ));
                }
            }
            Self::Key { code, .. } => {
                if code.is_empty()
                    || code.len() > MAX_CODE
                    || !code.bytes().all(|b| b.is_ascii_alphanumeric())
                {
                    return Err(ProjectError::Invalid(
                        "a key source needs the code of a key".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// What a mapped button does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ButtonMode {
    /// A press runs the target (an action), or turns a switch on.
    #[default]
    Trigger,
    /// A press flips a switch.
    Toggle,
    /// On while the button is held: a piano clip plays and stops, a switch follows the button, a
    /// continuous target goes back to its minimum on release.
    Hold,
}

impl ButtonMode {
    fn is_trigger(&self) -> bool {
        *self == Self::Trigger
    }
}

/// What is driven. Every id names something of the grid; one that is gone makes the mapping go.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Target {
    // Actions.
    /// Launches the clip; released, a piano clip stops.
    ClipLaunch {
        clip: String,
    },
    ColumnLaunch {
        column: String,
    },
    LayerStop {
        layer: String,
    },
    /// Steps the layer's autopilot: off, next, previous, random.
    LayerAutopilot {
        layer: String,
    },
    DeckSelect {
        deck: String,
    },
    DeckNext,
    DeckPrev,
    StopAll,
    BpmTap,
    BpmResync,
    /// One beat a minute more, or less.
    BpmPlus,
    BpmMinus,
    BpmDouble,
    BpmHalf,
    /// Runs the beats a little faster or slower while held.
    BpmNudgeUp,
    BpmNudgeDown,
    // Switches.
    LayerBypass {
        layer: String,
    },
    LayerSolo {
        layer: String,
    },
    GroupBypass {
        group: String,
    },
    GroupSolo {
        group: String,
    },
    EffectBypass {
        effect: String,
    },
    /// Holds the beats.
    BpmPause,
    // Continuous.
    LayerMaster {
        layer: String,
    },
    LayerVolume {
        layer: String,
    },
    LayerOpacity {
        layer: String,
    },
    GroupMaster {
        group: String,
    },
    GroupOpacity {
        group: String,
    },
    /// The transition time, 0 to 10 s.
    LayerTransition {
        layer: String,
    },
    Crossfader,
    Bpm,
    ClipSpeed {
        clip: String,
    },
    EffectParam {
        effect: String,
        param: String,
    },
    /// A number of a generator clip (`size`, `rate`...).
    GeneratorNumber {
        clip: String,
        name: String,
    },
    /// A knob of the dashboard, 0 to 1.
    Dashboard {
        knob: usize,
    },
    /// A knob of a layer's dashboard, 0 to 1 (see [`crate::vj_dashboard`]).
    LayerDashboard {
        layer: String,
        knob: usize,
    },
    /// A knob of a clip's dashboard, 0 to 1.
    ClipDashboard {
        clip: String,
        knob: usize,
    },
}

/// How a target is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Class {
    Continuous,
    Switch,
    Action,
}

impl Target {
    pub fn class(&self) -> Class {
        match self {
            Self::LayerBypass { .. }
            | Self::LayerSolo { .. }
            | Self::GroupBypass { .. }
            | Self::GroupSolo { .. }
            | Self::EffectBypass { .. }
            | Self::BpmPause => Class::Switch,
            Self::LayerMaster { .. }
            | Self::LayerVolume { .. }
            | Self::LayerOpacity { .. }
            | Self::GroupMaster { .. }
            | Self::GroupOpacity { .. }
            | Self::LayerTransition { .. }
            | Self::Crossfader
            | Self::Bpm
            | Self::ClipSpeed { .. }
            | Self::EffectParam { .. }
            | Self::GeneratorNumber { .. }
            | Self::Dashboard { .. }
            | Self::LayerDashboard { .. }
            | Self::ClipDashboard { .. } => Class::Continuous,
            _ => Class::Action,
        }
    }

    /// Whether what it names is still in the grid.
    pub fn exists(&self, vj: &Vj) -> bool {
        let layer = |id: &str| vj.layers.iter().any(|l| l.id == id);
        match self {
            Self::ClipLaunch { clip } | Self::ClipSpeed { clip } => vj.find_clip(clip).is_some(),
            Self::GeneratorNumber { clip, name } => vj
                .find_clip(clip)
                .and_then(|(_, clip)| clip.generator.as_ref())
                .is_some_and(|generator| generator.numbers().contains(&name.as_str())),
            Self::ColumnLaunch { column } => vj.column_place(column).is_some(),
            Self::LayerStop { layer: id }
            | Self::LayerAutopilot { layer: id }
            | Self::LayerBypass { layer: id }
            | Self::LayerSolo { layer: id }
            | Self::LayerMaster { layer: id }
            | Self::LayerVolume { layer: id }
            | Self::LayerOpacity { layer: id }
            | Self::LayerTransition { layer: id } => layer(id),
            Self::GroupBypass { group }
            | Self::GroupSolo { group }
            | Self::GroupMaster { group }
            | Self::GroupOpacity { group } => vj.group(group).is_some(),
            Self::DeckSelect { deck } => vj.decks.iter().any(|d| &d.id == deck),
            Self::EffectBypass { effect } => vj.find_effect(effect).is_some(),
            Self::EffectParam { effect, param } => vj
                .find_effect(effect)
                .is_some_and(|e| e.kind.number_range(param).is_some()),
            Self::Dashboard { knob } => *knob < KNOBS,
            Self::LayerDashboard { layer: id, knob } => *knob < KNOBS && layer(id),
            Self::ClipDashboard { clip, knob } => *knob < KNOBS && vj.find_clip(clip).is_some(),
            _ => true,
        }
    }

    /// The range of a continuous target, in its own units; `None` for another class or a target
    /// that is gone.
    pub fn range(&self, vj: &Vj) -> Option<(f32, f32)> {
        if !self.exists(vj) {
            return None;
        }
        match self {
            Self::LayerMaster { .. }
            | Self::LayerVolume { .. }
            | Self::LayerOpacity { .. }
            | Self::GroupMaster { .. }
            | Self::GroupOpacity { .. }
            | Self::Crossfader
            | Self::Dashboard { .. }
            | Self::LayerDashboard { .. }
            | Self::ClipDashboard { .. } => Some((0.0, 1.0)),
            Self::LayerTransition { .. } => Some((0.0, MAX_TRANSITION)),
            Self::Bpm => Some((MIN_BPM as f32, MAX_BPM as f32)),
            Self::ClipSpeed { .. } => Some((MIN_SPEED, MAX_SPEED)),
            Self::EffectParam { effect, param } => vj.find_effect(effect)?.kind.number_range(param),
            Self::GeneratorNumber { name, .. } => crate::generator::number_range(name),
            _ => None,
        }
    }
}

/// A source driving a target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub id: String,
    pub source: Source,
    pub target: Target,
    /// Where a continuous target goes when the source is at its minimum, and its maximum, in the
    /// target's own units. Either may be the larger: that inverts the movement.
    pub min: f32,
    pub max: f32,
    /// Turns the source round, on top of the range.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// What a button does; absent is [`ButtonMode::Trigger`].
    #[serde(default, skip_serializing_if = "ButtonMode::is_trigger")]
    pub mode: ButtonMode,
    /// How a clip, layer, column, deck or effect target is followed; absent is [`Follow::Id`].
    #[serde(default, skip_serializing_if = "Follow::is_id")]
    pub follow: Follow,
    /// For [`Follow::Position`]: the slot the mapping stands for (a target whose ids are places,
    /// see [`crate::vj_mapfile`]). `target` is whatever fills the slot now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Target>,
    /// A position mapping whose slot is empty: kept, and answering nothing, until something
    /// fills the slot again. `target` is then stale.
    #[serde(default, skip_serializing_if = "is_false")]
    pub parked: bool,
    /// What the pad of a MIDI source is sent for each state (see [`crate::vj_feedback`]); absent
    /// uses the grid's colours, then the controller's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lights: Option<crate::vj_feedback::Lights>,
}

/// What a mapping of a clip, layer, column, deck or effect keeps hold of, as Resolume's shortcut
/// modes do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Follow {
    /// This clip, layer or effect, by id, wherever it moves; deleting it takes the mapping away.
    #[default]
    Id,
    /// The slot (layer N, column M, effect slot K): whatever fills it now.
    Position,
}

impl Follow {
    fn is_id(&self) -> bool {
        *self == Self::Id
    }
}

impl Mapping {
    /// The value of the target for a source at `level` (0 to 1).
    pub fn scale(&self, level: f32) -> f32 {
        let s = level.clamp(0.0, 1.0);
        let s = if self.invert { 1.0 - s } else { s };
        self.min + (self.max - self.min) * s
    }
}

/// One knob of the dashboard.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Knob {
    /// 0 to 1.
    #[serde(default)]
    pub value: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the knob moves, each into a range of its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// What turns the knob by itself, 0 to 1 (see [`crate::vj_modulation`]). The stored `value`
    /// stays as it was: the drawn grid moves what the knob is linked to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modulation: Option<crate::vj_modulation::Modulation>,
}

/// A number a knob drives: at 0 the knob gives `min`, at 1 `max` (units of the target).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub target: Target,
    pub min: f32,
    pub max: f32,
}

impl Link {
    pub(crate) fn at(&self, knob: f32) -> f32 {
        self.min + (self.max - self.min) * knob.clamp(0.0, 1.0)
    }
}
