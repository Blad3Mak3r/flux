//! Audio output devices (cpal) and MIDI output ports (midir), and resolving the name hints that
//! projects store back to a device that is currently present.
//!
//! Devices come and go, so nothing is cached: every call enumerates again, which is also how the
//! UI refreshes after a hot-plug. A device that is missing is a normal outcome (`None`), never an
//! error, so opening a project can never fail because of hardware.

use std::collections::HashSet;
use std::io;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Serialize, Serializer};
use thiserror::Error;

/// Rates worth offering in a picker; a device usually reports a continuous range instead.
const STANDARD_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

const MIDI_CLIENT_NAME: &str = "Flux Timecode";

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("MIDI is unavailable: {0}")]
    Midi(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for DeviceError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub name: String,
    pub output_channels: u16,
    /// Standard rates the device supports.
    pub sample_rates: Vec<u32>,
    /// Whether it is the default output of the default host.
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiPort {
    pub name: String,
}

/// Opens the output device a project's name hint refers to. An empty hint is the system's default
/// output. It looks in the same list the UI offers, so what is listed is exactly what can be opened.
pub fn open_audio(hint: &str) -> Option<cpal::Device> {
    if hint.is_empty() {
        return cpal::default_host().default_output_device();
    }
    let devices = output_devices();
    find_by_name(hint, &devices, 0, |(name, _)| name).map(|(_, device)| device.clone())
}

/// The output devices worth offering: one per physical output of the default audio host.
pub fn list_audio() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device_name(&device));
    output_devices()
        .into_iter()
        .filter_map(|(name, device)| {
            // A device that vanishes or cannot be probed mid-listing is skipped, not fatal.
            let (output_channels, sample_rates) = output_capabilities(&device)?;
            Some(AudioDevice {
                is_default: default_name.as_deref() == Some(name.as_str()),
                name,
                output_channels,
                sample_rates,
            })
        })
        .collect()
}

/// Named outputs of the default host, each name once; on ALSA only those that pass [`is_listed`].
/// Other hosts (JACK, ...) only repeat the same hardware under another API, so they are left out.
fn output_devices() -> Vec<(String, cpal::Device)> {
    let host = cpal::default_host();
    let alsa = host.id().name() == "ALSA";
    let Ok(outputs) = host.output_devices() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    outputs
        .filter(|device| {
            !alsa
                || device
                    .description()
                    .is_ok_and(|description| is_listed(description.driver()))
        })
        .filter_map(|device| Some((device_name(&device)?, device)))
        // ALSA names one card twice (`plughw:CARD=PCH,...` and `plughw:CARD=0,...`).
        .filter(|(name, _)| seen.insert(name.clone()))
        .collect()
}

/// Whether an ALSA device is a choice a person would make. WASAPI, CoreAudio and the like already give
/// one entry per output, but ALSA lists every plugin of every card (`hw:`, `front:`, `surround51:`,
/// `hdmi:`, `null`, ...) under the same name: only `plughw:` stays, one per physical device that
/// converts whatever format or rate the project needs. The sound servers (`pipewire`, `pulse`,
/// `default`) are left out because the picker's "System default" already goes through them.
fn is_listed(pcm_id: Option<&str>) -> bool {
    pcm_id.is_some_and(|pcm| pcm.starts_with("plughw:"))
}

pub fn list_midi() -> Result<Vec<MidiPort>, DeviceError> {
    let output = midir::MidiOutput::new(MIDI_CLIENT_NAME)
        .map_err(|err| DeviceError::Midi(err.to_string()))?;
    Ok(output
        .ports()
        .iter()
        .filter_map(|port| output.port_name(port).ok())
        .map(|name| MidiPort { name })
        .collect())
}

pub fn device_name(device: &cpal::Device) -> Option<String> {
    Some(device.description().ok()?.name().to_owned())
}

fn output_capabilities(device: &cpal::Device) -> Option<(u16, Vec<u32>)> {
    let configs: Vec<_> = device.supported_output_configs().ok()?.collect();
    let channels = configs.iter().map(|config| config.channels()).max()?;
    let rates = STANDARD_RATES
        .into_iter()
        .filter(|&rate| configs.iter().any(|config| config.contains_rate(rate)))
        .collect();
    Some((channels, rates))
}

/// Finds the item a stored name hint refers to: exact matches first, then those that ignore case,
/// surrounding whitespace and the trailing `client:port` numbers ALSA appends to MIDI port names
/// (they change when devices are re-plugged). Several items can share a name (two interfaces of
/// one model): `index` picks among the matches, in list order, and an index past the last gives `None`.
pub fn find_by_name<'a, T>(
    hint: &str,
    items: &'a [T],
    index: usize,
    name: impl Fn(&T) -> &str,
) -> Option<&'a T> {
    items
        .iter()
        .filter(|item| name(item) == hint)
        .nth(index)
        .or_else(|| {
            let hint = normalize(hint);
            items
                .iter()
                .filter(|item| normalize(name(item)) == hint)
                .nth(index)
        })
}

fn normalize(name: &str) -> String {
    let name = name.trim();
    let without_ports = match name.rsplit_once(' ') {
        Some((head, tail)) if is_alsa_address(tail) => head,
        _ => name,
    };
    without_ports.trim().to_lowercase()
}

/// `24:0`, the client and port numbers at the end of an ALSA port name.
fn is_alsa_address(token: &str) -> bool {
    token.split_once(':').is_some_and(|(client, port)| {
        [client, port]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// Enumeration can probe hardware and block, so the commands run it off the main thread.
async fn blocking<T, F>(op: F) -> Result<T, DeviceError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DeviceError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|err| DeviceError::Io(io::Error::other(err)))?
}

#[tauri::command]
pub async fn devices_list_audio() -> Result<Vec<AudioDevice>, DeviceError> {
    blocking(|| Ok(list_audio())).await
}

#[tauri::command]
pub async fn devices_list_midi() -> Result<Vec<MidiPort>, DeviceError> {
    blocking(list_midi).await
}

/// The audio device a project's name hint refers to, or `null` when it is not connected.
#[tauri::command]
pub async fn devices_find_audio(name: String) -> Result<Option<AudioDevice>, DeviceError> {
    blocking(move || Ok(find_by_name(&name, &list_audio(), 0, |d| &d.name).cloned())).await
}

/// The MIDI port a project's name hint refers to, or `null` when it is not connected. A system
/// without MIDI support has no ports, so that also resolves to `null` instead of an error.
#[tauri::command]
pub async fn devices_find_midi(
    name: String,
    index: usize,
) -> Result<Option<MidiPort>, DeviceError> {
    blocking(move || {
        let ports = list_midi().unwrap_or_default();
        Ok(find_by_name(&name, &ports, index, |p| &p.name).cloned())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(names: &[&str]) -> Vec<MidiPort> {
        names
            .iter()
            .map(|&name| MidiPort { name: name.into() })
            .collect()
    }

    fn find<'a>(hint: &str, items: &'a [MidiPort]) -> Option<&'a str> {
        find_by_name(hint, items, 0, |p| &p.name).map(|p| p.name.as_str())
    }

    #[test]
    fn is_listed_should_keep_one_entry_per_physical_alsa_output() {
        for pcm in ["plughw:CARD=PCH,DEV=0", "plughw:CARD=1,DEV=0"] {
            assert!(is_listed(Some(pcm)), "{pcm}");
        }
        for pcm in [
            "hw:CARD=PCH,DEV=0",
            "sysdefault:CARD=PCH",
            "front:CARD=PCH,DEV=0",
            "surround51:CARD=PCH,DEV=0",
            "hdmi:CARD=HDMI,DEV=0",
            "iec958:CARD=PCH,DEV=0",
            "default",
            "pipewire",
            "pulse",
            "null",
            "samplerate",
        ] {
            assert!(!is_listed(Some(pcm)), "{pcm}");
        }
    }

    #[test]
    fn is_listed_should_drop_an_alsa_device_without_a_pcm_id() {
        assert!(!is_listed(None));
    }

    #[test]
    fn find_by_name_should_match_exactly() {
        let items = ports(&["IAC Driver Bus 1", "Other"]);
        assert_eq!(find("IAC Driver Bus 1", &items), Some("IAC Driver Bus 1"));
    }

    #[test]
    fn find_by_name_should_ignore_case_and_whitespace() {
        let items = ports(&["Main Interface"]);
        assert_eq!(find("  main interface ", &items), Some("Main Interface"));
    }

    #[test]
    fn find_by_name_should_ignore_alsa_client_and_port_numbers() {
        let items = ports(&[
            "Midi Through:Midi Through Port-0 14:0",
            "UM-ONE:UM-ONE MIDI 1 28:0",
        ]);
        assert_eq!(
            find("UM-ONE:UM-ONE MIDI 1 20:0", &items),
            Some("UM-ONE:UM-ONE MIDI 1 28:0")
        );
        assert_eq!(
            find("Midi Through:Midi Through Port-0", &items),
            Some("Midi Through:Midi Through Port-0 14:0")
        );
    }

    #[test]
    fn find_by_name_should_prefer_an_exact_match() {
        let items = ports(&["synth", "Synth"]);
        assert_eq!(find("Synth", &items), Some("Synth"));
    }

    #[test]
    fn find_by_name_should_tell_ports_with_the_same_name_apart_by_index() {
        let items = ports(&["Hub", "Other", "Hub"]);
        let at = |index| find_by_name("Hub", &items, index, |p| &p.name);
        assert!(at(0).is_some_and(|p| std::ptr::eq(p, &items[0])));
        assert!(at(1).is_some_and(|p| std::ptr::eq(p, &items[2])));
        assert!(at(2).is_none());
    }

    #[test]
    fn find_by_name_should_count_loose_matches_when_the_hint_is_not_exact() {
        let items = ports(&["hub 20:0", "Hub 28:0"]);
        let found = find_by_name("Hub 14:0", &items, 1, |p| &p.name);
        assert!(found.is_some_and(|p| std::ptr::eq(p, &items[1])));
    }

    #[test]
    fn find_by_name_should_report_a_missing_device() {
        assert_eq!(find("Gone", &ports(&["Main Interface"])), None);
        assert_eq!(find("Gone", &[]), None);
    }

    #[test]
    fn find_by_name_should_not_treat_other_numbers_as_alsa_addresses() {
        let items = ports(&["Port 2", "Port 3"]);
        assert_eq!(find("Port 3", &items), Some("Port 3"));
        assert_eq!(find("Port 4", &items), None);
    }
}
