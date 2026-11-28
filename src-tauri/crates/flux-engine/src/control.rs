//! Remote control: what a MIDI message or an OSC packet asks Flux to do.
//!
//! Only the pure part lives here: decoding the bytes, mapping them to a [`ControlAction`] and
//! finding the frame a cue action goes to. The ports, the socket and the execution belong to the
//! app, so this is testable without hardware.
//!
//! OSC has fixed addresses (see [`osc_action`]); MIDI has a list of [`Binding`]s the user makes,
//! usually by touching a pad. Flux stays the master clock: this only moves the transport, it never
//! follows a timecode.

use serde::{Deserialize, Serialize};

use flux_project::OscArg;
use flux_project::vj_mapping::{Heard, MidiKind, Source};

use crate::cues::Cue;

mod osc;

pub use osc::*;

#[cfg(test)]
mod tests;

/// What the control surface can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum ControlAction {
    Go,
    Devamp,
    CancelDevamp,
    Play,
    Pause,
    /// Play when stopped or paused, pause when playing.
    Toggle,
    /// Stops and goes back to the start of the timeline.
    Stop,
    NextCue,
    PrevCue,
    /// The cue with this number, from 1.
    GotoCue(u32),
    /// Moves by this many frames, back when negative.
    StepFrames(i32),
    /// Moves by this many seconds, back when negative.
    StepSeconds(i32),
    NextTimeline,
    PrevTimeline,
    /// The timeline in this place, from 1.
    SelectTimeline(u32),
    ViewEdit,
    ViewLive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerKind {
    Note,
    Cc,
    Program,
}

/// The message of a MIDI controller that can trigger an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiTrigger {
    pub kind: TriggerKind,
    /// From 1 to 16, as the cue tracks and the controllers' manuals count them.
    pub channel: u8,
    pub number: u8,
}

/// The values a pad is sent to light it and to put it out: a velocity for a note, a value for a
/// control change. What they mean (a colour, a brightness) is the controller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub on: u8,
    pub off: u8,
}

impl Feedback {
    pub const GENERIC: Self = Self { on: 127, off: 0 };

    /// Whether both values fit a MIDI data byte.
    pub fn is_valid(self) -> bool {
        self.on < 0x80 && self.off < 0x80
    }

    /// The values to start from for the controller on `port`, by the name its port has. Only the
    /// controllers whose manual says what a velocity means are known; any other gets the generic
    /// full-on and off, and the user sets the values per binding.
    pub fn preset(port: &str) -> Self {
        // The AKAI APC family lights its pads with a velocity that picks a colour: 1 is green.
        if port.to_ascii_lowercase().contains("apc") {
            Self { on: 1, off: 0 }
        } else {
            Self::GENERIC
        }
    }
}

/// A trigger and the action it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub trigger: MidiTrigger,
    pub action: ControlAction,
    /// What to send the pad to light it. `None` uses the controller's preset
    /// ([`Feedback::preset`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<Feedback>,
}

/// The action of `trigger`, if the user bound one.
pub fn action_for(bindings: &[Binding], trigger: MidiTrigger) -> Option<ControlAction> {
    bindings
        .iter()
        .find(|binding| binding.trigger == trigger)
        .map(|binding| binding.action)
}

/// `bindings` with one action per trigger: the first binding of a trigger stays.
pub fn dedup(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut kept: Vec<Binding> = Vec::with_capacity(bindings.len());
    for binding in bindings {
        if kept.iter().all(|other| other.trigger != binding.trigger) {
            kept.push(binding);
        }
    }
    kept
}

/// A key of the computer's keyboard the window forwards: pressed or let go, with the modifiers
/// held when it went down. The VJ mappings read it as a button ([`Self::heard`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyMsg {
    /// `KeyboardEvent.code`: the place of the key, whatever the layout.
    pub code: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
    pub down: bool,
}

impl KeyMsg {
    pub fn heard(&self) -> Heard {
        Heard {
            source: Source::Key {
                code: self.code.clone(),
                ctrl: self.ctrl,
                alt: self.alt,
                shift: self.shift,
                meta: self.meta,
            },
            level: if self.down { 1.0 } else { 0.0 },
            down: self.down,
        }
    }
}

/// What kind of MIDI message arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireKind {
    Note,
    Cc,
    Program,
    PitchBend,
}

/// A MIDI message a controller sends, decoded but not interpreted: the cue bindings read it as a
/// pad pressed ([`Self::trigger`]), the VJ mappings as a level ([`Self::heard`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiMsg {
    pub kind: WireKind,
    /// From 1 to 16.
    pub channel: u8,
    /// The note, control or program; 0 for a bend.
    pub number: u8,
    /// The velocity or value, 0 to 127 (0 is a release); a bend's 14 bits, 0 to 16383.
    pub value: u16,
}

impl MidiMsg {
    /// The trigger this presses, or `None` for a release (a note off, a note on with velocity 0, a
    /// control at 0) and for a bend. A program change has no release.
    pub fn trigger(&self) -> Option<MidiTrigger> {
        let kind = match self.kind {
            WireKind::Note => TriggerKind::Note,
            WireKind::Cc => TriggerKind::Cc,
            WireKind::Program => TriggerKind::Program,
            WireKind::PitchBend => return None,
        };
        (kind == TriggerKind::Program || self.value > 0).then_some(MidiTrigger {
            kind,
            channel: self.channel,
            number: self.number,
        })
    }

    /// The control as a VJ mapping sees it, learned on `port`: `None` for a program change.
    pub fn heard(&self, port: &str) -> Option<Heard> {
        let (kind, max) = match self.kind {
            WireKind::Note => (MidiKind::Note, 127.0),
            WireKind::Cc => (MidiKind::Cc, 127.0),
            WireKind::PitchBend => (MidiKind::PitchBend, 16383.0),
            WireKind::Program => return None,
        };
        let level = f32::from(self.value) / max;
        Some(Heard {
            source: Source::Midi {
                port: Some(port.to_owned()),
                channel: self.channel,
                kind,
                number: self.number,
            },
            level,
            // A bend rests in the middle: only past it is a button pressed.
            down: if kind == MidiKind::PitchBend {
                level > 0.5
            } else {
                self.value > 0
            },
        })
    }
}

/// The MIDI message in `bytes`, or `None` for anything a controller sends all the time that means
/// nothing here (clock, active sensing, aftertouch).
pub fn decode_midi_message(bytes: &[u8]) -> Option<MidiMsg> {
    let (&status, data) = bytes.split_first()?;
    let channel = (status & 0x0F) + 1;
    let kind = match status & 0xF0 {
        0x80 | 0x90 => WireKind::Note,
        0xB0 => WireKind::Cc,
        0xC0 => WireKind::Program,
        0xE0 => WireKind::PitchBend,
        _ => return None,
    };
    let data_byte = |at: usize| data.get(at).copied().filter(|byte| *byte < 0x80);
    let (number, value) = match kind {
        WireKind::PitchBend => {
            let (low, high) = (data_byte(0)?, data_byte(1)?);
            (0, u16::from(high) << 7 | u16::from(low))
        }
        WireKind::Program => (data_byte(0)?, 0),
        _ if status & 0xF0 == 0x80 => (data_byte(0)?, 0),
        _ => (data_byte(0)?, u16::from(data_byte(1)?)),
    };
    Some(MidiMsg {
        kind,
        channel,
        number,
        value,
    })
}

/// The trigger a MIDI message presses, or `None` for anything else: releases (a note off, a note
/// on with velocity 0, a control at 0), clock, active sensing and the rest of the traffic a
/// controller sends all the time.
pub fn decode_midi(bytes: &[u8]) -> Option<MidiTrigger> {
    decode_midi_message(bytes)?.trigger()
}

/// The frame of the first unmuted cue after `position`.
pub fn next_cue_frame(cues: &[Cue], position: u64) -> Option<u64> {
    cues.iter()
        .filter(|cue| !cue.muted && cue.frame > position)
        .map(|cue| cue.frame)
        .min()
}

/// The frame of the last unmuted cue before `position`. A cue that started less than `grace`
/// frames ago does not count, so that pressing "previous" right after a cue goes to the one
/// before, and later restarts the one that is playing.
pub fn prev_cue_frame(cues: &[Cue], position: u64, grace: u64) -> Option<u64> {
    cues.iter()
        .filter(|cue| !cue.muted && cue.frame.saturating_add(grace) < position)
        .map(|cue| cue.frame)
        .max()
}

/// The frame of the cue with this number, muted or not: it is a place in the show.
pub fn goto_cue_frame(cues: &[Cue], number: u32) -> Option<u64> {
    cues.iter()
        .find(|cue| cue.number == number)
        .map(|cue| cue.frame)
}

/// `position` moved by `delta` frames, held inside `first..=last`.
pub fn step(position: u64, delta: i64, first: u64, last: u64) -> u64 {
    let moved = i128::from(position) + i128::from(delta);
    let clamped = moved.clamp(i128::from(first), i128::from(last.max(first)));
    // Inside two `u64`, so it fits.
    u64::try_from(clamped).unwrap_or(first)
}

/// The frame an action that moves the position goes to, or `None` when it moves nothing (the
/// transport and view actions, or a cue that does not exist). `position` and the result count the
/// timeline's start offset, `start` is where the timeline begins and `fps` is its nominal frame
/// rate, which is also how long "previous" forgives a cue that just started.
pub fn seek_target(
    action: ControlAction,
    position: u64,
    cues: &[Cue],
    start: u64,
    fps: u8,
) -> Option<u64> {
    let fps = u64::from(fps);
    match action {
        ControlAction::NextCue => next_cue_frame(cues, position),
        ControlAction::PrevCue => prev_cue_frame(cues, position, fps),
        ControlAction::GotoCue(number) => goto_cue_frame(cues, number),
        ControlAction::StepFrames(frames) => {
            Some(step(position, i64::from(frames), start, u64::MAX))
        }
        ControlAction::StepSeconds(seconds) => {
            let frames = i64::from(seconds).saturating_mul(i64::try_from(fps).unwrap_or(0));
            Some(step(position, frames, start, u64::MAX))
        }
        _ => None,
    }
}
