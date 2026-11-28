//! Read-only checks before running a show. No network requests, audio streams or MIDI are started.
use crate::devices::{AudioDevice, MidiPort, find_by_name};
use cpal::traits::DeviceTrait;
use flux_core::{Clock, ENGINE_RATE};
use flux_project::{
    AudioAsset, OscDestination, OutputConfig, Project, TrackKind, UNASSIGNED_DEVICE, VoiceStatus,
};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueCode {
    Ungenerated,
    Stale,
    BeforeOrigin,
    MissingAudio,
    Overlap,
    MissingRoute,
    LtcConflict,
    SilentOutput,
    MidiPortMissing,
    OscDestinationMissing,
    OscDestinationInvalid,
    DmxDestinationMissing,
    DmxDestinationInvalid,
    LtcUnavailable,
    MtcUnavailable,
    LtcCollision,
    MtcCollision,
    EmptyTrack,
    /// The display chosen for the video output is not connected.
    VideoDisplayMissing,
    /// The show has video clips but the video output is off.
    VideoOutputOff,
}

/// The part of the show an issue belongs to; the dialog groups its list by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Area {
    Operator,
    Midi,
    Osc,
    Dmx,
    Outputs,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: IssueCode,
    pub area: Area,
    pub blocking: bool,
    pub timeline_id: Option<String>,
    pub timeline_name: Option<String>,
    pub order_id: Option<String>,
    pub name: String,
    pub frame: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub orders: usize,
    pub ready: usize,
    pub errors: usize,
    pub warnings: usize,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Copy)]
pub struct RouteCheck {
    pub available: bool,
    pub ltc_conflict: bool,
}

/// Query capabilities only; an available device still needs a real sound check.
pub fn probe(output: &OutputConfig) -> RouteCheck {
    let Some(route) = &output.operator else {
        return RouteCheck {
            available: false,
            ltc_conflict: false,
        };
    };
    let device = crate::devices::open_audio(&route.device_name, output.driver.as_deref());
    let needed = route.channels.iter().copied().max().unwrap_or(0);
    let available = needed > 0
        && device.as_ref().is_some_and(|device| {
            device.supported_output_configs().is_ok_and(|mut ranges| {
                ranges.any(|r| {
                    r.channels() >= needed
                        && r.min_sample_rate() <= ENGINE_RATE
                        && r.max_sample_rate() >= ENGINE_RATE
                })
            })
        });
    let name = device.as_ref().and_then(crate::devices::device_name);
    let ltc_conflict = output.ltc.iter().any(|ltc| {
        route.channels.contains(&ltc.channel)
            && (ltc.device_name == route.device_name
                || name.as_ref().is_some_and(|name| {
                    crate::devices::open_audio(&ltc.device_name, output.driver.as_deref())
                        .as_ref()
                        .and_then(crate::devices::device_name)
                        .as_ref()
                        == Some(name)
                }))
    });
    RouteCheck {
        available,
        ltc_conflict,
    }
}

pub fn check(
    project: &Project,
    local_voice: Option<&str>,
    route: RouteCheck,
    samples: impl FnMut(&AudioAsset) -> Option<u64>,
) -> Report {
    check_routes(project, local_voice, |_, _| route, samples)
}

pub fn check_routes(
    project: &Project,
    local_voice: Option<&str>,
    mut route: impl FnMut(&flux_project::Timeline, &flux_project::Track) -> RouteCheck,
    mut samples: impl FnMut(&AudioAsset) -> Option<u64>,
) -> Report {
    let mut report = Report::default();
    for timeline in &project.timelines {
        let clock = Clock::new(ENGINE_RATE, timeline.timecode.frame_rate);
        let mut intervals = Vec::new();
        for track in timeline.tracks.iter().filter(|track| !track.muted) {
            let TrackKind::Operator { orders, .. } = &track.kind else {
                continue;
            };
            let route = route(timeline, track);
            let voice = flux_project::operator::effective_voice(project, track, local_voice).0;
            for order in orders {
                report.orders += 1;
                let name = order
                    .generated
                    .as_ref()
                    .map_or(order.dialogue.as_str(), |g| g.dialogue.as_str());
                let issue = |code, blocking| Issue {
                    code,
                    area: Area::Operator,
                    blocking,
                    timeline_id: Some(timeline.id.clone()),
                    timeline_name: Some(timeline.name.clone()),
                    order_id: Some(order.id.clone()),
                    name: name.to_owned(),
                    frame: Some(timeline.timecode.start + order.at),
                };
                match order.status(voice) {
                    VoiceStatus::Ungenerated => {
                        report.issues.push(issue(IssueCode::Ungenerated, true))
                    }
                    VoiceStatus::Stale => report.issues.push(issue(IssueCode::Stale, false)),
                    VoiceStatus::Synced => {}
                }
                if let Some(g) = &order.generated {
                    let readable = project
                        .assets
                        .iter()
                        .find(|a| a.id == g.asset)
                        .is_some_and(|a| samples(a) == Some(g.samples));
                    if !readable {
                        report.issues.push(issue(IssueCode::MissingAudio, true));
                    }
                    if let Some((from, to)) = order.interval(clock) {
                        if readable
                            && order.status(voice) == VoiceStatus::Synced
                            && route.available
                            && !route.ltc_conflict
                        {
                            report.ready += 1;
                        }
                        intervals.push((from, to, order));
                    } else {
                        report.issues.push(issue(IssueCode::BeforeOrigin, true));
                    }
                }
            }
            if !orders.is_empty() {
                let silent = flux_project::routing::target(track).is_none();
                for (code, present, blocking) in [
                    (IssueCode::SilentOutput, silent, false),
                    (IssueCode::MissingRoute, !route.available && !silent, true),
                    (IssueCode::LtcConflict, route.ltc_conflict, true),
                ] {
                    if present {
                        report.issues.push(Issue {
                            code,
                            area: Area::Operator,
                            blocking,
                            timeline_id: Some(timeline.id.clone()),
                            timeline_name: Some(timeline.name.clone()),
                            order_id: None,
                            name: track.name.clone(),
                            frame: None,
                        });
                    }
                }
            }
        }
        intervals.sort_by_key(|(from, _, _)| *from);
        let mut longest: Option<(u64, u64, &flux_project::VoiceOrder)> = None;
        let mut overlaps = HashSet::new();
        for item in &intervals {
            if let Some((_, end, order)) = longest
                && item.0 < end
            {
                overlaps.insert(order.id.as_str());
                overlaps.insert(item.2.id.as_str());
            }
            if longest.is_none_or(|(_, end, _)| item.1 > end) {
                longest = Some(*item);
            }
        }
        for (_, _, order) in intervals {
            if overlaps.contains(order.id.as_str()) {
                report.issues.push(Issue {
                    code: IssueCode::Overlap,
                    area: Area::Operator,
                    blocking: false,
                    timeline_id: Some(timeline.id.clone()),
                    timeline_name: Some(timeline.name.clone()),
                    order_id: Some(order.id.clone()),
                    name: order
                        .generated
                        .as_ref()
                        .map_or_else(|| order.dialogue.clone(), |g| g.dialogue.clone()),
                    frame: Some(timeline.timecode.start + order.at),
                });
            }
        }
    }
    report.tally();
    report
}

impl Report {
    /// Adds issues found by another check and recounts errors (blocking) and warnings.
    /// What the video output needs: the show has video clips, and the display chosen for them is
    /// `chosen` (`None` when the output is off), connected or not.
    pub fn check_video(&mut self, uses_video: bool, chosen: Option<&str>, connected: bool) {
        let code = match chosen {
            _ if !uses_video => return,
            None => IssueCode::VideoOutputOff,
            Some(_) if !connected => IssueCode::VideoDisplayMissing,
            Some(_) => return,
        };
        self.extend(vec![Issue {
            code,
            area: Area::Outputs,
            blocking: code == IssueCode::VideoDisplayMissing,
            timeline_id: None,
            timeline_name: None,
            order_id: None,
            name: chosen.unwrap_or_default().to_owned(),
            frame: None,
        }]);
    }

    pub fn extend(&mut self, issues: Vec<Issue>) {
        self.issues.extend(issues);
        self.tally();
    }

    fn tally(&mut self) {
        self.errors = self.issues.iter().filter(|i| i.blocking).count();
        self.warnings = self.issues.len() - self.errors;
    }
}

/// What this computer offers: audio outputs and MIDI ports, listed once for a whole check.
#[derive(Debug, Default)]
pub struct Hardware {
    pub audio: Vec<AudioDevice>,
    pub midi: Vec<MidiPort>,
}

impl Hardware {
    /// Enumerates only. Nothing attached, or a MIDI system that cannot start, is an empty list and
    /// never an error, so the check runs with no devices.
    pub fn scan(driver: Option<&str>) -> Self {
        Self {
            audio: crate::devices::list_audio(driver),
            midi: crate::devices::list_midi().unwrap_or_default(),
        }
    }

    /// The device a name hint refers to; an empty hint is the host's default output.
    fn audio_device(&self, hint: &str) -> Option<&AudioDevice> {
        if hint.is_empty() {
            return self.audio.iter().find(|d| d.is_default);
        }
        find_by_name(hint, &self.audio, 0, |d| &d.name)
    }

    /// Where in the list the port a name hint and index refer to is.
    fn midi_port(&self, name: &str, index: usize) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let found = find_by_name(name, &self.midi, index, |p| &p.name)?;
        self.midi.iter().position(|p| std::ptr::eq(p, found))
    }
}

/// A host name or an IP address, judged by its spelling alone: resolving it would use the network.
fn valid_host(host: &str) -> bool {
    if host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .is_ok()
    {
        return true;
    }
    let host = host.strip_suffix('.').unwrap_or(host);
    // A name ending in digits only is a malformed IPv4 address (`300.1.1.1`), not a name.
    host.len() <= 253
        && host
            .rsplit('.')
            .next()
            .is_some_and(|tld| !tld.bytes().all(|b| b.is_ascii_digit()))
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

fn valid_destination(destination: &OscDestination) -> bool {
    destination.port != 0 && valid_host(&destination.host)
}

/// Checks the MIDI and OSC tracks and the configured LTC / MTC outputs against `hardware`. `output`
/// is the project's outputs as this computer's preferences resolve them. Nothing is opened or sent.
pub fn check_hardware(project: &Project, output: &OutputConfig, hardware: &Hardware) -> Vec<Issue> {
    let mut issues = Vec::new();
    for timeline in &project.timelines {
        for track in timeline.tracks.iter().filter(|track| !track.muted) {
            let (area, empty, problem) = match &track.kind {
                TrackKind::Midi { port, events } => {
                    let present = port
                        .as_ref()
                        .is_some_and(|p| hardware.midi_port(&p.port_name, p.port_index).is_some());
                    (
                        Area::Midi,
                        events.is_empty(),
                        (!present).then_some(IssueCode::MidiPortMissing),
                    )
                }
                TrackKind::Osc {
                    destination,
                    events,
                } => {
                    let found = destination
                        .as_deref()
                        .and_then(|id| output.osc.iter().find(|d| d.id == id));
                    let problem = match found {
                        None => Some(IssueCode::OscDestinationMissing),
                        Some(d) if !valid_destination(d) => Some(IssueCode::OscDestinationInvalid),
                        Some(_) => None,
                    };
                    (Area::Osc, events.is_empty(), problem)
                }
                TrackKind::Dmx {
                    destination,
                    scenes,
                } => {
                    let found = destination
                        .as_deref()
                        .and_then(|id| output.dmx.iter().find(|d| d.id == id));
                    let problem = match found {
                        None => Some(IssueCode::DmxDestinationMissing),
                        Some(d) if !valid_host(&d.host) => Some(IssueCode::DmxDestinationInvalid),
                        Some(_) => None,
                    };
                    (Area::Dmx, scenes.is_empty(), problem)
                }
                _ => continue,
            };
            // A track with nothing to fire cannot fail to fire it: only its emptiness is worth a word.
            let (code, blocking) = if empty {
                (IssueCode::EmptyTrack, false)
            } else if let Some(code) = problem {
                (code, true)
            } else {
                continue;
            };
            issues.push(Issue {
                code,
                area,
                blocking,
                timeline_id: Some(timeline.id.clone()),
                timeline_name: Some(timeline.name.clone()),
                order_id: None,
                name: track.name.clone(),
                frame: None,
            });
        }
    }

    let output_issue = |code, name: &str| Issue {
        code,
        area: Area::Outputs,
        blocking: true,
        timeline_id: None,
        timeline_name: None,
        order_id: None,
        name: name.to_owned(),
        frame: None,
    };
    let mut ltc_taken = HashSet::new();
    for ltc in &output.ltc {
        let device = (ltc.device_name != UNASSIGNED_DEVICE)
            .then(|| hardware.audio_device(&ltc.device_name))
            .flatten()
            .filter(|d| {
                (1..=d.output_channels).contains(&ltc.channel)
                    && d.sample_rates.contains(&ENGINE_RATE)
            });
        let name = if ltc.device_name == UNASSIGNED_DEVICE {
            ""
        } else {
            &ltc.device_name
        };
        match device {
            None => issues.push(output_issue(IssueCode::LtcUnavailable, name)),
            Some(d) if !ltc_taken.insert((d.name.as_str(), ltc.channel)) => {
                issues.push(output_issue(IssueCode::LtcCollision, name));
            }
            Some(_) => {}
        }
    }
    let mut mtc_taken = HashSet::new();
    for mtc in &output.mtc {
        match hardware.midi_port(&mtc.port_name, mtc.port_index) {
            None => issues.push(output_issue(IssueCode::MtcUnavailable, &mtc.port_name)),
            Some(at) if !mtc_taken.insert(at) => {
                issues.push(output_issue(IssueCode::MtcCollision, &mtc.port_name));
            }
            Some(_) => {}
        }
    }
    // Stable, so each area keeps its tracks in timeline order and the dialog can group by area.
    issues.sort_by_key(|issue| issue.area);
    issues
}

#[cfg(test)]
mod tests;
