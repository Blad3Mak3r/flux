//! The events of the MIDI and OSC tracks, and the rules that make one valid. Positions are frame
//! counts from the timeline origin, like every other position in a project.

use serde::{Deserialize, Serialize};

use super::error::{ProjectError, Result};

/// The MIDI port a track sends to. Only a name hint is stored, like for MTC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiPortRef {
    pub port_name: String,
    /// Which of the ports named `port_name` this is, counting from 0 in the system's order.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub port_index: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiEvent {
    #[serde(default, skip_serializing_if = "is_false")]
    pub first_turn_only: bool,
    pub id: String,
    pub at: u64,
    pub message: MidiMessage,
}

/// What a MIDI event sends. Channels are 1 to 16, as a musician counts them; every other number
/// is the 7-bit value that goes on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MidiMessage {
    /// With a `length` the note is released by itself that many frames later.
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<u64>,
    },
    NoteOff {
        channel: u8,
        note: u8,
    },
    ControlChange {
        channel: u8,
        controller: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    /// MIDI Show Control. `cue` and `list` are written as the console expects them, e.g. `"12.5"`.
    Msc {
        /// 0 to 127; 127 reaches every device.
        device_id: u8,
        /// The command format, 1 to 127 (1 is lighting, 127 every kind of device).
        format: u8,
        command: MscCommand,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cue: Option<String>,
        /// Only with a `cue`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list: Option<String>,
    },
    /// The bytes between `F0` and `F7`, each below `0x80`. The frame is added when it is sent.
    SysEx {
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MscCommand {
    Go,
    Stop,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OscEvent {
    #[serde(default, skip_serializing_if = "is_false")]
    pub first_turn_only: bool,
    pub id: String,
    pub at: u64,
    /// Starts with `/`, e.g. `/cue/1/start`.
    pub address: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<OscArg>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum OscArg {
    Int(i32),
    Float(f32),
    String(String),
    Bool(bool),
}

/// A receiver of OSC over UDP, defined once for the project and named by the tracks that use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OscDestination {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
}

fn invalid(what: impl std::fmt::Display) -> ProjectError {
    ProjectError::Invalid(what.to_string())
}

fn check_channel(id: &str, channel: u8) -> Result<()> {
    if (1..=16).contains(&channel) {
        Ok(())
    } else {
        Err(invalid(format!("event {id}: MIDI channels are 1 to 16")))
    }
}

fn check_data(id: &str, what: &str, value: u8) -> Result<()> {
    if value < 0x80 {
        Ok(())
    } else {
        Err(invalid(format!("event {id}: {what} must be 0 to 127")))
    }
}

/// A cue or list number: digits and dots, as MSC writes them.
fn check_number(id: &str, what: &str, text: &str) -> Result<()> {
    let valid = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    if valid {
        Ok(())
    } else {
        Err(invalid(format!(
            "event {id}: the {what} number holds digits and dots only"
        )))
    }
}

impl MidiEvent {
    pub(super) fn validate(&self) -> Result<()> {
        let id = &self.id;
        match &self.message {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
                length,
            } => {
                check_channel(id, *channel)?;
                check_data(id, "the note", *note)?;
                check_data(id, "the velocity", *velocity)?;
                if *length == Some(0) {
                    return Err(invalid(format!(
                        "event {id} lasts no time; leave the length out instead"
                    )));
                }
            }
            MidiMessage::NoteOff { channel, note } => {
                check_channel(id, *channel)?;
                check_data(id, "the note", *note)?;
            }
            MidiMessage::ControlChange {
                channel,
                controller,
                value,
            } => {
                check_channel(id, *channel)?;
                check_data(id, "the controller", *controller)?;
                check_data(id, "the value", *value)?;
            }
            MidiMessage::ProgramChange { channel, program } => {
                check_channel(id, *channel)?;
                check_data(id, "the program", *program)?;
            }
            MidiMessage::Msc {
                device_id,
                format,
                cue,
                list,
                ..
            } => {
                check_data(id, "the device id", *device_id)?;
                if !(1..0x80).contains(format) {
                    return Err(invalid(format!(
                        "event {id}: the command format must be 1 to 127"
                    )));
                }
                if let Some(cue) = cue {
                    check_number(id, "cue", cue)?;
                }
                match (cue, list) {
                    (Some(_), Some(list)) => check_number(id, "list", list)?,
                    (None, Some(_)) => {
                        return Err(invalid(format!("event {id}: a list needs a cue")));
                    }
                    _ => {}
                }
            }
            MidiMessage::SysEx { data } => {
                if data.is_empty() || data.iter().any(|&byte| byte >= 0x80) {
                    return Err(invalid(format!(
                        "event {id}: SysEx holds one or more bytes of 0 to 127"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl OscEvent {
    pub(super) fn validate(&self) -> Result<()> {
        let id = &self.id;
        let address_ok = self.address.len() > 1
            && self.address.starts_with('/')
            && !self
                .address
                .chars()
                .any(|c| c.is_whitespace() || c.is_control());
        if !address_ok {
            return Err(invalid(format!(
                "event {id}: an OSC address starts with / and has no spaces"
            )));
        }
        for arg in &self.args {
            match arg {
                OscArg::Float(value) if !value.is_finite() => {
                    return Err(invalid(format!("event {id}: a float is not a number")));
                }
                OscArg::String(text) if text.contains('\0') => {
                    return Err(invalid(format!("event {id}: a string holds a NUL")));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl OscDestination {
    pub(super) fn validate(&self) -> Result<()> {
        let host_ok = !self.host.is_empty() && !self.host.chars().any(char::is_whitespace);
        if self.id.is_empty() || !host_ok || self.port == 0 {
            return Err(invalid(format!(
                "OSC destination {} needs a host and a port",
                self.id
            )));
        }
        Ok(())
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn midi(message: MidiMessage) -> MidiEvent {
        MidiEvent {
            first_turn_only: false,
            id: "e1".into(),
            at: 0,
            message,
        }
    }

    fn osc(address: &str, args: Vec<OscArg>) -> OscEvent {
        OscEvent {
            first_turn_only: false,
            id: "e1".into(),
            at: 0,
            address: address.into(),
            args,
        }
    }

    fn note_on(channel: u8, note: u8, velocity: u8) -> MidiMessage {
        MidiMessage::NoteOn {
            channel,
            note,
            velocity,
            length: None,
        }
    }

    #[test]
    fn a_note_on_inside_the_ranges_should_be_valid() {
        assert!(midi(note_on(1, 60, 127)).validate().is_ok());
        assert!(midi(note_on(16, 0, 0)).validate().is_ok());
    }

    #[test]
    fn a_channel_outside_1_to_16_should_be_refused() {
        assert!(midi(note_on(0, 60, 100)).validate().is_err());
        assert!(midi(note_on(17, 60, 100)).validate().is_err());
    }

    #[test]
    fn a_data_byte_above_127_should_be_refused() {
        assert!(midi(note_on(1, 128, 100)).validate().is_err());
        assert!(midi(note_on(1, 60, 128)).validate().is_err());
    }

    #[test]
    fn a_note_with_a_zero_length_should_be_refused() {
        let message = MidiMessage::NoteOn {
            channel: 1,
            note: 60,
            velocity: 100,
            length: Some(0),
        };
        assert!(midi(message).validate().is_err());
    }

    #[test]
    fn msc_should_need_digits_and_dots_and_a_cue_for_a_list() {
        let msc = |cue: Option<&str>, list: Option<&str>| {
            midi(MidiMessage::Msc {
                device_id: 127,
                format: 1,
                command: MscCommand::Go,
                cue: cue.map(Into::into),
                list: list.map(Into::into),
            })
        };
        assert!(msc(Some("12.5"), Some("1")).validate().is_ok());
        assert!(msc(None, None).validate().is_ok());
        assert!(msc(Some("12a"), None).validate().is_err());
        assert!(msc(None, Some("1")).validate().is_err());
    }

    #[test]
    fn sysex_should_hold_seven_bit_bytes_only() {
        let sysex = |data: Vec<u8>| midi(MidiMessage::SysEx { data });
        assert!(sysex(vec![0x7F, 0x01]).validate().is_ok());
        assert!(sysex(vec![]).validate().is_err());
        assert!(sysex(vec![0xF7]).validate().is_err());
    }

    #[test]
    fn an_osc_address_should_start_with_a_slash_and_have_no_spaces() {
        assert!(osc("/cue/1/start", vec![]).validate().is_ok());
        assert!(osc("cue", vec![]).validate().is_err());
        assert!(osc("/", vec![]).validate().is_err());
        assert!(osc("/cue 1", vec![]).validate().is_err());
    }

    #[test]
    fn an_osc_float_should_be_a_number() {
        assert!(osc("/a", vec![OscArg::Float(f32::NAN)]).validate().is_err());
        assert!(osc("/a", vec![OscArg::Float(0.5)]).validate().is_ok());
    }

    #[test]
    fn a_destination_should_need_a_host_and_a_port() {
        let destination = |host: &str, port| OscDestination {
            id: "d1".into(),
            name: "QLab".into(),
            host: host.into(),
            port,
        };
        assert!(destination("192.168.1.20", 53000).validate().is_ok());
        assert!(destination("", 53000).validate().is_err());
        assert!(destination("a b", 53000).validate().is_err());
        assert!(destination("localhost", 0).validate().is_err());
    }

    #[test]
    fn messages_should_use_the_documented_json_shape() {
        let json = serde_json::to_value(note_on(1, 60, 100)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "noteOn", "channel": 1, "note": 60, "velocity": 100 })
        );
        let json = serde_json::to_value(OscArg::Int(5)).unwrap();
        assert_eq!(json, serde_json::json!({ "type": "int", "value": 5 }));
    }
}
