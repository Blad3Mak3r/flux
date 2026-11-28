use super::*;

/// Frees the clip sets the audio callback no longer holds. A set only this list references has
/// been swapped out already, and nothing else can clone it, so dropping it here, on the control
/// side, is the last release: the callback never frees one.
pub(super) fn prune_clip_sets(sets: &mut Vec<Arc<ClipSet>>) {
    sets.retain(|set| Arc::strong_count(set) > 1);
}

pub(super) struct AudioSpec {
    /// The audio API `device_hint` is looked up on; `None` is the system's default.
    pub(super) driver: Option<String>,
    pub(super) device_hint: String,
    pub(super) needed: u16,
    /// 1-based interface channels of the programme.
    pub(super) programme: Vec<u16>,
    pub(super) clips: Arc<ClipSet>,
    /// The LTC outputs on this stream. Empty when LTC is off or plays on devices of its own.
    pub(super) ltc: Vec<LtcSend>,
    /// The metronome, when it plays on this stream.
    pub(super) click: Option<ClickSpec>,
    pub(super) ltc_config: LtcConfig,
    pub(super) rate: FrameRate,
    pub(super) start: u64,
    pub(super) clock: Clock,
}

pub(super) struct AudioInfo {
    pub(super) device: String,
    pub(super) channels: u16,
}

#[derive(Clone)]
pub(super) struct LtcSpec {
    pub(super) driver: Option<String>,
    pub(super) device_hint: String,
    /// The LTC outputs on this device. Empty when the stream plays only the metronome.
    pub(super) ltc: Vec<LtcSend>,
    /// The metronome, when it plays on this device.
    pub(super) click: Option<ClickSpec>,
    pub(super) config: LtcConfig,
    pub(super) rate: FrameRate,
    pub(super) start: u64,
    pub(super) clock: Clock,
}

/// Which stream the metronome plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClickPlace {
    /// The programme's stream.
    Programme,
    /// The stream of the LTC device at this position of the plan.
    Group(usize),
    /// A stream of its own, for a device that carries nothing else.
    Own,
}

/// The metronome and the 1-based channels it plays on, before the device says how many it has.
#[derive(Debug, Clone)]
pub(super) struct ClickSpec {
    pub(super) shared: Arc<SharedMetronome>,
    pub(super) channels: Vec<u16>,
}

impl ClickSpec {
    /// The click as a stream with `opened` channels plays it: channels the device lacks are left out.
    pub(super) fn out(&self, opened: u16) -> ClickOut {
        ClickOut {
            shared: Arc::clone(&self.shared),
            channels: self
                .channels
                .iter()
                .map(|&channel| usize::from(channel) - 1)
                .filter(|&channel| channel < usize::from(opened))
                .collect(),
        }
    }
}

/// Whether two device name hints open the same audio device. Different hints can: an empty one is
/// the system's default output, and names match loosely.
pub(super) fn same_device(driver: Option<&str>, a: &str, b: &str) -> bool {
    let name = |hint: &str| {
        devices::open_audio(hint, driver).and_then(|device| devices::device_name(&device))
    };
    a == b || matches!((name(a), name(b)), (Some(a), Some(b)) if a == b)
}

/// One LTC signal on one interface channel, and the project outputs that asked for it.
#[derive(Debug, PartialEq)]
pub(super) struct LtcSlot {
    /// 1-based.
    pub(super) channel: u16,
    /// Linear peak level: the first output that asked for this slot sets it.
    pub(super) level: f32,
    /// Latency offset in ms, set by the first output like the level.
    pub(super) offset_ms: i32,
    /// Positions in the project's `output.ltc`. More than one when the same device and channel is
    /// listed twice, which plays once.
    pub(super) outputs: Vec<usize>,
}

/// Where each LTC output of a project plays.
#[derive(Debug, PartialEq)]
pub(super) struct LtcPlan {
    /// Channels written into the programme's own stream.
    pub(super) shared: Vec<LtcSlot>,
    /// The other devices, each with the channels it carries.
    pub(super) own: Vec<LtcGroup>,
}

#[derive(Debug, PartialEq)]
pub(super) struct LtcGroup {
    /// The name hint of the first output that named the device.
    pub(super) device: String,
    pub(super) slots: Vec<LtcSlot>,
}

/// What each slot sends: its 1-based channel, level and offset.
pub(super) fn sends_of(slots: &[LtcSlot]) -> Vec<LtcSend> {
    slots
        .iter()
        .map(|slot| LtcSend {
            channel: slot.channel,
            level: slot.level,
            offset_ms: slot.offset_ms,
        })
        .collect()
}

/// Splits the project's LTC outputs between the programme's stream and one stream per other
/// device, so no device is opened twice.
pub(super) fn plan_ltc(
    outputs: &[LtcOutput],
    programme_device: &str,
    same: impl Fn(&str, &str) -> bool,
) -> LtcPlan {
    let mut plan = LtcPlan {
        shared: Vec::new(),
        own: Vec::new(),
    };
    for (index, output) in outputs.iter().enumerate() {
        if output.device_name == flux_project::UNASSIGNED_DEVICE {
            continue;
        }
        let slots = if same(&output.device_name, programme_device) {
            &mut plan.shared
        } else {
            let at = plan
                .own
                .iter()
                .position(|group| same(&group.device, &output.device_name));
            let at = at.unwrap_or_else(|| {
                plan.own.push(LtcGroup {
                    device: output.device_name.clone(),
                    slots: Vec::new(),
                });
                plan.own.len() - 1
            });
            &mut plan.own[at].slots
        };
        match slots.iter_mut().find(|slot| slot.channel == output.channel) {
            Some(slot) => slot.outputs.push(index),
            None => slots.push(LtcSlot {
                channel: output.channel,
                level: output.level.unwrap_or(DEFAULT_LTC_LEVEL),
                offset_ms: output.offset_ms.unwrap_or(0),
                outputs: vec![index],
            }),
        }
    }
    plan
}
