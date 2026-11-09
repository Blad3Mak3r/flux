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

const MIDI_CLIENT_NAME: &str = "FLUX";
const MIDI_INPUT_CLIENT_NAME: &str = "FLUX control";

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

/// An audio API (driver) the system offers: ALSA, PipeWire, JACK, WASAPI, CoreAudio, ...
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioHost {
    pub name: String,
    /// Whether it is the one the system uses when a project names none.
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiPort {
    pub name: String,
}

/// Makes the calling thread a multithreaded COM apartment before cpal touches it.
///
/// cpal puts every thread it meets in a single-threaded apartment. Some WASAPI drivers initialise
/// COM as multithreaded inside `IMMDevice::Activate`, which then fails with `RPC_E_CHANGED_MODE`
/// ("cannot change the thread mode after it is set") and the device cannot be opened. cpal accepts
/// a thread that is already multithreaded, so a thread that owns a stream calls this first.
pub(crate) fn join_mta() {
    #[cfg(windows)]
    {
        const COINIT_MULTITHREADED: u32 = 0;
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoInitializeEx(reserved: *mut core::ffi::c_void, flags: u32) -> i32;
        }
        // A thread that is already in an apartment answers with an error; it keeps that one.
        // SAFETY: a null reserved pointer and a documented flag; the apartment ends with the thread.
        let _ = unsafe { CoInitializeEx(core::ptr::null_mut(), COINIT_MULTITHREADED) };
    }
}

/// The audio APIs available here, the default one first.
pub fn list_hosts() -> Vec<AudioHost> {
    let default = cpal::default_host().id();
    let mut hosts: Vec<AudioHost> = cpal::available_hosts()
        .into_iter()
        .map(|id| AudioHost {
            name: id.name().to_owned(),
            is_default: id == default,
        })
        .collect();
    hosts.sort_by_key(|host| !host.is_default);
    hosts
}

/// The host a project's driver names. A driver this machine does not have (a project written on
/// another system) falls back to the default one, like a missing device does not fail the project.
fn host_for(driver: Option<&str>) -> cpal::Host {
    driver
        .and_then(|name| {
            cpal::available_hosts()
                .into_iter()
                .find(|id| id.name() == name)
        })
        .and_then(|id| cpal::host_from_id(id).ok())
        .unwrap_or_else(cpal::default_host)
}

/// Opens the output device a project's name hint refers to, on the host `driver` names. An empty
/// hint is that host's default output, which the audio picker offers as "System default".
pub fn open_audio(hint: &str, driver: Option<&str>) -> Option<cpal::Device> {
    let host = host_for(driver);
    if hint.is_empty() {
        return host.default_output_device();
    }
    let devices = output_devices(&host);
    find_by_name(hint, &devices, 0, |(name, _)| name).map(|(_, device)| device.clone())
}

/// The output devices of the host `driver` names worth offering, one per physical output.
pub fn list_audio(driver: Option<&str>) -> Vec<AudioDevice> {
    let host = host_for(driver);
    let default_name = host
        .default_output_device()
        .and_then(|device| device_name(&device));
    output_devices(&host)
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

/// Named outputs of `host`, each name once, without the entries [`is_listed`] drops.
fn output_devices(host: &cpal::Host) -> Vec<(String, cpal::Device)> {
    let driver = host.id().name();
    let Ok(outputs) = host.output_devices() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    outputs
        .filter_map(|device| {
            let description = device.description().ok()?;
            let listed = is_listed(driver, description.driver(), description.name());
            listed.then(|| (description.name().to_owned(), device))
        })
        // ALSA names one card twice (`plughw:CARD=PCH,...` and `plughw:CARD=0,...`).
        .filter(|(name, _)| seen.insert(name.clone()))
        .collect()
}

/// Whether a device is a choice a person would make. WASAPI, CoreAudio and the like already give one
/// entry per output. ALSA lists every plugin of every card (`hw:`, `front:`, `surround51:`, `hdmi:`,
/// `null`, ...) under the same name: only `plughw:` stays, one per physical device that converts
/// whatever format or rate the project needs. PipeWire adds two aliases of the default sink. The
/// sound servers and aliases are left out because the picker's "System default" already is them.
fn is_listed(host: &str, pcm_id: Option<&str>, name: &str) -> bool {
    match host {
        "ALSA" => pcm_id.is_some_and(|pcm| pcm.starts_with("plughw:")),
        "PipeWire" => !matches!(name, "default_sink" | "default_output"),
        _ => true,
    }
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

/// The MIDI ports that send to Flux (controllers, pads), as [`list_midi`] lists those it sends to.
pub fn list_midi_inputs() -> Result<Vec<MidiPort>, DeviceError> {
    let input = midir::MidiInput::new(MIDI_INPUT_CLIENT_NAME)
        .map_err(|err| DeviceError::Midi(err.to_string()))?;
    Ok(input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
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
        let alsa = |pcm| is_listed("ALSA", Some(pcm), "HDA Intel PCH");
        for pcm in ["plughw:CARD=PCH,DEV=0", "plughw:CARD=1,DEV=0"] {
            assert!(alsa(pcm), "{pcm}");
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
            assert!(!alsa(pcm), "{pcm}");
        }
        assert!(!is_listed("ALSA", None, "Unknown"));
    }

    #[test]
    fn is_listed_should_hide_the_aliases_of_the_default_pipewire_sink() {
        assert!(!is_listed("PipeWire", None, "default_sink"));
        assert!(!is_listed("PipeWire", None, "default_output"));
        assert!(is_listed(
            "PipeWire",
            Some("api.alsa.pcm.sink"),
            "Built-in Audio Analog Stereo"
        ));
    }

    #[test]
    fn is_listed_should_keep_every_device_of_other_hosts() {
        for host in ["WASAPI", "ASIO", "CoreAudio", "JACK"] {
            assert!(is_listed(host, None, "Speakers (Realtek)"), "{host}");
        }
    }

    #[test]
    fn host_for_should_fall_back_to_the_default_host_for_an_unknown_driver() {
        let default = cpal::default_host().id();
        assert_eq!(host_for(None).id(), default);
        assert_eq!(host_for(Some("No Such Driver")).id(), default);
    }

    #[test]
    fn host_for_should_pick_the_driver_by_name() {
        let default = cpal::default_host().id();
        assert_eq!(host_for(Some(default.name())).id(), default);
    }

    #[test]
    fn list_hosts_should_put_the_default_host_first() {
        let hosts = list_hosts();
        assert!(hosts.first().is_some_and(|host| host.is_default));
        assert_eq!(hosts.iter().filter(|host| host.is_default).count(), 1);
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
