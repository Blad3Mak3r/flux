//! Light feedback: what a MIDI controller is sent so that its pads show the state of the show.
//!
//! Which pad is lit is derived from the same facts the Live view draws (the transport, the cues,
//! the show's tick), so a controller can never disagree with the screen. [`light`] decides it for
//! one action; [`Lamps`] remembers what each pad was last sent and writes only what changed. Both
//! are pure and tested with a recording [`MidiWrite`]. The thread that samples the facts and owns
//! the port is the app's (`src-tauri/src/control.rs`); nothing here runs in an audio callback or
//! logs.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::control::{Binding, ControlAction, Feedback, MidiTrigger, TriggerKind, seek_target};
use crate::cues::Cue;
use crate::midi_cue::MidiWrite;
use crate::output::connect_midi;
use crate::show::{GoAction, ShowTick};
use crate::transport::TransportState;
use flux_project::vj_feedback::{Lights, Pad};
use flux_project::vj_mapping::MidiKind;

const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;

const CLIENT_NAME: &str = "FLUX control light";

/// How a pad should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Off,
    On,
    /// Alternates between lit and out; [`Lamps::update`] is told which half of the blink it is.
    Blink,
}

/// What the show is doing, as the Live view knows it.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The transport of the running output, `None` with no output open.
    pub transport: Option<TransportState>,
    /// The position, with the timeline's start offset, and the cues the output plays.
    pub position: u64,
    pub cues: &'a [Cue],
    /// Where the active timeline begins and its nominal frame rate, as [`seek_target`] takes them.
    pub start: u64,
    pub fps: u8,
    /// The tick of a prepared setlist, `None` when the output plays a single timeline.
    pub show: Option<ShowTick>,
    /// The place of the active timeline among the project's, from 0, and how many there are.
    pub timeline: Option<(usize, usize)>,
}

impl Facts<'_> {
    fn open(&self) -> bool {
        self.transport.is_some()
    }

    fn is(&self, state: TransportState) -> Light {
        if self.transport == Some(state) {
            Light::On
        } else {
            Light::Off
        }
    }
}

fn lit(on: bool) -> Light {
    if on { Light::On } else { Light::Off }
}

/// How the pad of `action` looks, or `None` when it has no state to show (the view is the
/// window's, and the backend does not know it).
pub fn light(action: ControlAction, facts: &Facts) -> Option<Light> {
    let show = facts.show.unwrap_or_default();
    let can_change_timeline = facts.transport != Some(TransportState::Playing);
    let timeline = |available: fn(usize, usize) -> bool| {
        lit(can_change_timeline
            && facts
                .timeline
                .is_some_and(|(at, count)| available(at, count)))
    };
    Some(match action {
        ControlAction::Play => facts.is(TransportState::Playing),
        ControlAction::Toggle => facts.is(TransportState::Playing),
        ControlAction::Pause => facts.is(TransportState::Paused),
        ControlAction::Stop => facts.is(TransportState::Stopped),
        // GO waits for the operator, and blinks while a devamp is asked for.
        ControlAction::Go if show.devamp_pending => Light::Blink,
        ControlAction::Go => lit(facts.show.is_some() && show.go != GoAction::None),
        // Devamp is available inside a loop, and the turn counts from 1 while in one.
        ControlAction::Devamp if show.devamp_pending => Light::Blink,
        ControlAction::Devamp => lit(facts.show.is_some() && show.turn > 0),
        ControlAction::CancelDevamp => lit(show.devamp_pending),
        ControlAction::NextCue
        | ControlAction::PrevCue
        | ControlAction::GotoCue(_)
        | ControlAction::StepFrames(_)
        | ControlAction::StepSeconds(_) => lit(facts.open()
            && seek_target(action, facts.position, facts.cues, facts.start, facts.fps).is_some()),
        ControlAction::NextTimeline => timeline(|at, count| at + 1 < count),
        ControlAction::PrevTimeline => timeline(|at, _| at > 0),
        ControlAction::SelectTimeline(place) => {
            let place = place as usize;
            lit(can_change_timeline
                && facts
                    .timeline
                    .is_some_and(|(_, count)| (1..=count).contains(&place)))
        }
        ControlAction::NextMarker
        | ControlAction::PrevMarker
        | ControlAction::ViewEdit
        | ControlAction::ViewLive
        | ControlAction::Panic => return None,
    })
}

/// The message that sets the pad of `trigger` to `value`. A program change has no "off", so it
/// is never lit.
fn message(trigger: MidiTrigger, value: u8) -> Option<[u8; 3]> {
    let status = match trigger.kind {
        TriggerKind::Note => NOTE_ON,
        TriggerKind::Cc => CONTROL_CHANGE,
        TriggerKind::Program => return None,
    };
    // A note on at velocity 0 is how a note off is sent.
    Some([
        status | (trigger.channel.saturating_sub(1) & 0x0F),
        trigger.number,
        value & 0x7F,
    ])
}

/// What each pad was last sent, so that only a change is written.
#[derive(Debug, Default)]
pub struct Lamps {
    /// The value on the pad and the one that puts it out, in case it is cleared later.
    sent: HashMap<MidiTrigger, (u8, u8)>,
}

impl Lamps {
    /// Writes to `port` the pads whose value is not the one they hold. The first call writes all
    /// of them, so the controller starts in step with the show. A pad whose write failed is
    /// written again by the next call. Returns how many writes failed, for the caller to count.
    ///
    /// `preset` is the feedback of the bindings that carry none; `blink_on` is which half of the
    /// blink it is.
    pub fn update(
        &mut self,
        bindings: &[Binding],
        preset: Feedback,
        facts: &Facts,
        blink_on: bool,
        port: &mut impl MidiWrite,
    ) -> usize {
        let mut failed = 0;
        for binding in bindings {
            let Some(light) = light(binding.action, facts) else {
                continue;
            };
            let feedback = binding.feedback.unwrap_or(preset);
            let value = match light {
                Light::On => feedback.on,
                Light::Blink if blink_on => feedback.on,
                Light::Off | Light::Blink => feedback.off,
            };
            failed += usize::from(!self.set(binding.trigger, value, feedback.off, port));
        }
        failed
    }

    /// Sends `value` to the pad of `trigger` unless it already holds it. `off` is what puts the
    /// pad out, kept for [`Self::clear`]. `false` when the write failed, so the next call tries
    /// again; a trigger that has no message (a program change) counts as done.
    pub fn set(
        &mut self,
        trigger: MidiTrigger,
        value: u8,
        off: u8,
        port: &mut impl MidiWrite,
    ) -> bool {
        if self.sent.get(&trigger) == Some(&(value, off)) {
            return true;
        }
        let Some(bytes) = message(trigger, value) else {
            return true;
        };
        if !port.write(&bytes) {
            return false;
        }
        self.sent.insert(trigger, (value, off));
        true
    }

    /// Puts out the pads that are not in `keep` any more (a mapping was removed) and forgets
    /// them.
    pub fn retain(
        &mut self,
        keep: &std::collections::HashSet<MidiTrigger>,
        port: &mut impl MidiWrite,
    ) {
        self.sent.retain(|trigger, (value, off)| {
            if keep.contains(trigger) {
                return true;
            }
            if value != off
                && let Some(bytes) = message(*trigger, *off)
            {
                port.write(&bytes);
            }
            false
        });
    }

    /// Puts out every pad that is lit and forgets them all, for when the feedback ends.
    pub fn clear(&mut self, port: &mut impl MidiWrite) {
        for (trigger, (value, off)) in self.sent.drain() {
            if value != off
                && let Some(bytes) = message(trigger, off)
            {
                port.write(&bytes);
            }
        }
    }

    /// Forgets what was sent, so that the next [`Self::update`] writes every pad again: the port
    /// was reopened and the controller may have lost its lights.
    pub fn forget(&mut self) {
        self.sent.clear();
    }
}

/// The pads of the VJ mappings by the port they are lit through: the name the control was learned
/// on, or `default` (the controller of the control preferences) for a mapping that names none.
pub fn by_port<'a>(pads: &'a [Pad], default: &str) -> BTreeMap<String, Vec<&'a Pad>> {
    let mut ports: BTreeMap<String, Vec<&Pad>> = BTreeMap::new();
    for pad in pads {
        ports
            .entry(pad.port.clone().unwrap_or_else(|| default.to_owned()))
            .or_default()
            .push(pad);
    }
    ports
}

fn trigger_of(pad: &Pad) -> MidiTrigger {
    MidiTrigger {
        kind: match pad.kind {
            MidiKind::Note => TriggerKind::Note,
            _ => TriggerKind::Cc,
        },
        channel: pad.channel,
        number: pad.number,
    }
}

impl Lamps {
    /// Brings the pads of the VJ mappings on one port to the state of `pads`, writing only what
    /// changed, and puts out the pads of mappings that are gone. `base` is what a pad with no
    /// colours of its own is sent. Returns how many writes failed.
    pub fn update_pads(&mut self, pads: &[&Pad], base: Lights, port: &mut impl MidiWrite) -> usize {
        let mut failed = 0;
        let mut keep = HashSet::new();
        for pad in pads {
            let trigger = trigger_of(pad);
            keep.insert(trigger);
            failed += usize::from(!self.set(trigger, pad.value(base), pad.off(base), port));
        }
        self.retain(&keep, port);
        failed
    }
}

/// The MIDI output of a controller. Dropping it closes the port.
pub struct LightPort {
    name: String,
    connection: midir::MidiOutputConnection,
}

impl LightPort {
    /// Opens the output port a stored name hint refers to (`index` picks among ports of the same
    /// name). A controller has an input and an output port that carry the same name.
    pub fn open(hint: &str, index: usize) -> Result<Self, String> {
        let (_, name, connection) = connect_midi(hint, index, CLIENT_NAME)?;
        Ok(Self { name, connection })
    }

    /// The name of the port as the system lists it.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl MidiWrite for LightPort {
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.connection.write(bytes)
    }
}

#[cfg(test)]
mod tests;
