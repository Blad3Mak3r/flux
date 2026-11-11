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
    LtcUnavailable,
    MtcUnavailable,
    LtcCollision,
    MtcCollision,
    EmptyTrack,
}

/// The part of the show an issue belongs to; the dialog groups its list by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Area {
    Operator,
    Midi,
    Osc,
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
mod tests {
    use super::*;
    use flux_project::{
        Alignment, FrameRate, GeneratedVoice, LtcOutput, MidiEvent, MidiMessage, MidiPortRef,
        MtcOutput, OscEvent, Track, VoiceOrder,
    };

    fn project() -> Project {
        let mut p = Project::new("Show", FrameRate::Fps25);
        p.voice = Some("voice".into());
        p.assets.push(AudioAsset {
            id: "audio".into(),
            name: "Go".into(),
            file: "audio/go.mp3".into(),
            bpm: None,
        });
        p.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "operator".into(),
            name: "Operator".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Operator {
                voice: None,
                orders: vec![VoiceOrder {
                    mode: Default::default(),
                    synthesis_text: None,
                    trim_start: 0,
                    trim_end: 0,
                    id: "first".into(),
                    at: 25,
                    dialogue: "Go".into(),
                    alignment: Alignment::Start,
                    generated: Some(GeneratedVoice {
                        origin: flux_project::operator::RecordingOrigin::Generated,
                        synthesis_text: None,
                        asset: "audio".into(),
                        dialogue: "Go".into(),
                        voice: "voice".into(),
                        model: flux_project::operator::VOICE_MODEL.into(),
                        samples: 48_000,
                    }),
                }],
            },
        });
        p
    }

    const READY: RouteCheck = RouteCheck {
        available: true,
        ltc_conflict: false,
    };
    fn orders(p: &mut Project) -> &mut Vec<VoiceOrder> {
        let TrackKind::Operator { orders, .. } = &mut p.timelines[0].tracks[0].kind else {
            panic!()
        };
        orders
    }
    fn codes(r: &Report) -> Vec<IssueCode> {
        r.issues.iter().map(|i| i.code).collect()
    }

    #[test]
    fn valid_show_is_clear_and_muted_tracks_are_excluded() {
        let mut p = project();
        let report = check(&p, None, READY, |_| Some(48_000));
        assert_eq!(report.orders, 1);
        assert!(report.issues.is_empty());
        p.timelines[0].tracks[0].muted = true;
        let report = check(
            &p,
            None,
            RouteCheck {
                available: false,
                ltc_conflict: true,
            },
            |_| None,
        );
        assert_eq!(report.orders, 0);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn content_audio_position_and_route_problems_are_independent() {
        let mut p = project();
        let order = &mut orders(&mut p)[0];
        order.dialogue = "A new draft".into();
        order.alignment = Alignment::End;
        order.at = 1;
        let report = check(
            &p,
            None,
            RouteCheck {
                available: false,
                ltc_conflict: true,
            },
            |_| Some(47_999),
        );
        assert_eq!(
            codes(&report),
            [
                IssueCode::Stale,
                IssueCode::MissingAudio,
                IssueCode::BeforeOrigin,
                IssueCode::MissingRoute,
                IssueCode::LtcConflict
            ]
        );
        assert_eq!((report.errors, report.warnings), (4, 1));
        assert_eq!(report.issues[0].name, "Go");
        orders(&mut p)[0].generated = None;
        let report = check(&p, None, READY, |_| unreachable!());
        assert_eq!(codes(&report), [IssueCode::Ungenerated]);
    }

    #[test]
    fn overlaps_mark_each_order_once_and_adjacent_audio_is_allowed() {
        let mut p = project();
        let mut second = orders(&mut p)[0].clone();
        second.id = "second".into();
        second.at = 50;
        orders(&mut p).push(second.clone());
        assert!(check(&p, None, READY, |_| Some(48_000)).issues.is_empty());
        second.id = "third".into();
        second.at = 49;
        orders(&mut p).push(second);
        let report = check(&p, None, READY, |_| Some(48_000));
        assert_eq!(codes(&report), [IssueCode::Overlap; 3]);
        assert_eq!((report.errors, report.warnings), (0, 3));
    }

    #[test]
    fn each_operator_track_uses_its_own_route_check_and_silent_is_explicit() {
        let mut p = project();
        let mut other = p.timelines[0].tracks[0].clone();
        other.id = "other".into();
        other.name = "Stage".into();
        if let TrackKind::Operator { orders, .. } = &mut other.kind {
            orders[0].id = "second".into();
            orders[0].at = 100;
        }
        p.timelines[0].tracks.push(other);
        let report = check_routes(
            &p,
            None,
            |_, track| {
                if track.id == "other" {
                    RouteCheck {
                        available: false,
                        ltc_conflict: false,
                    }
                } else {
                    READY
                }
            },
            |_| Some(48_000),
        );
        assert_eq!((report.orders, report.ready, report.errors), (2, 1, 1));
        assert_eq!(report.issues[0].name, "Stage");
        p.timelines[0].tracks[1].audio_output = flux_project::routing::AudioAssignment::Silent;
        let report = check_routes(
            &p,
            None,
            |_, track| {
                if track.id == "other" {
                    RouteCheck {
                        available: false,
                        ltc_conflict: false,
                    }
                } else {
                    READY
                }
            },
            |_| Some(48_000),
        );
        assert_eq!(codes(&report), [IssueCode::SilentOutput]);
        assert_eq!((report.errors, report.warnings), (0, 1));
    }

    fn fake_hardware() -> Hardware {
        let audio = |name: &str, output_channels, sample_rates: &[u32], is_default| AudioDevice {
            name: name.into(),
            output_channels,
            sample_rates: sample_rates.to_vec(),
            is_default,
        };
        Hardware {
            audio: vec![
                audio("Main", 8, &[44_100, 48_000], true),
                audio("Old card", 2, &[44_100], false),
            ],
            midi: ["Hub", "Hub", "Lights"]
                .map(|name| MidiPort { name: name.into() })
                .to_vec(),
        }
    }

    fn cue_project(kind: TrackKind) -> Project {
        let mut p = Project::new("Show", FrameRate::Fps25);
        p.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "cues".into(),
            name: "Cues".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind,
        });
        p
    }

    fn midi_track(port: Option<(&str, usize)>, events: usize) -> TrackKind {
        TrackKind::Midi {
            port: port.map(|(name, index)| MidiPortRef {
                port_name: name.into(),
                port_index: index,
            }),
            events: (0..events)
                .map(|i| MidiEvent {
                    first_turn_only: false,
                    id: format!("m{i}"),
                    at: 0,
                    message: MidiMessage::ProgramChange {
                        channel: 1,
                        program: 1,
                    },
                })
                .collect(),
        }
    }

    fn osc_track(destination: Option<&str>, events: usize) -> TrackKind {
        TrackKind::Osc {
            destination: destination.map(Into::into),
            events: (0..events)
                .map(|i| OscEvent {
                    first_turn_only: false,
                    id: format!("o{i}"),
                    at: 0,
                    address: "/go".into(),
                    args: vec![],
                })
                .collect(),
        }
    }

    fn destination(host: &str, port: u16) -> OscDestination {
        OscDestination {
            id: "qlab".into(),
            name: "QLab".into(),
            host: host.into(),
            port,
        }
    }

    fn ltc(device: &str, channel: u16) -> LtcOutput {
        LtcOutput {
            device_name: device.into(),
            channel,
            level: None,
            offset_ms: None,
        }
    }

    fn mtc(port: &str, index: usize) -> MtcOutput {
        MtcOutput {
            port_name: port.into(),
            port_index: index,
            offset_ms: None,
        }
    }

    fn hardware_codes(p: &Project, output: &OutputConfig, hw: &Hardware) -> Vec<(IssueCode, bool)> {
        check_hardware(p, output, hw)
            .iter()
            .map(|i| (i.code, i.blocking))
            .collect()
    }

    #[test]
    fn a_midi_track_needs_a_port_that_is_present() {
        let hw = fake_hardware();
        let out = OutputConfig::default();
        let check = |port| hardware_codes(&cue_project(midi_track(port, 1)), &out, &hw);
        assert!(check(Some(("Hub", 0))).is_empty());
        assert!(check(Some(("hub 24:1", 1))).is_empty());
        let missing = [(IssueCode::MidiPortMissing, true)];
        assert_eq!(check(Some(("Hub", 2))), missing);
        assert_eq!(check(Some(("Gone", 0))), missing);
        assert_eq!(check(None), missing);
        assert_eq!(
            hardware_codes(
                &cue_project(midi_track(Some(("Gone", 0)), 1)),
                &out,
                &Hardware::default()
            ),
            missing
        );
    }

    #[test]
    fn an_osc_track_needs_a_defined_destination_with_a_valid_address() {
        let hw = fake_hardware();
        let check = |chosen: Option<&str>, host: &str, port| {
            let out = OutputConfig {
                osc: vec![destination(host, port)],
                ..Default::default()
            };
            hardware_codes(&cue_project(osc_track(chosen, 1)), &out, &hw)
        };
        assert!(check(Some("qlab"), "192.168.1.20", 53000).is_empty());
        assert!(check(Some("qlab"), "qlab.local", 53000).is_empty());
        assert!(check(Some("qlab"), "::1", 53000).is_empty());
        let missing = [(IssueCode::OscDestinationMissing, true)];
        assert_eq!(check(Some("other"), "192.168.1.20", 53000), missing);
        assert_eq!(check(None, "192.168.1.20", 53000), missing);
        let invalid = [(IssueCode::OscDestinationInvalid, true)];
        assert_eq!(check(Some("qlab"), "300.1.1.1", 53000), invalid);
        assert_eq!(check(Some("qlab"), "bad host!", 53000), invalid);
        assert_eq!(check(Some("qlab"), "-x.local", 53000), invalid);
        assert_eq!(check(Some("qlab"), "192.168.1.20", 0), invalid);
    }

    #[test]
    fn a_track_with_no_events_is_a_warning_and_a_muted_track_is_skipped() {
        let hw = fake_hardware();
        let out = OutputConfig::default();
        let warned = [(IssueCode::EmptyTrack, false)];
        let mut p = cue_project(midi_track(Some(("Gone", 0)), 0));
        assert_eq!(hardware_codes(&p, &out, &hw), warned);
        p = cue_project(osc_track(None, 0));
        assert_eq!(hardware_codes(&p, &out, &hw), warned);
        p.timelines[0].tracks[0].muted = true;
        assert!(hardware_codes(&p, &out, &hw).is_empty());
    }

    #[test]
    fn ltc_needs_a_present_device_channel_and_engine_rate() {
        let hw = fake_hardware();
        let p = Project::new("Show", FrameRate::Fps25);
        let check = |ltc: LtcOutput| {
            let out = OutputConfig {
                ltc: vec![ltc],
                ..Default::default()
            };
            hardware_codes(&p, &out, &hw)
        };
        assert!(check(ltc("Main", 8)).is_empty());
        assert!(check(ltc("", 1)).is_empty());
        let down = [(IssueCode::LtcUnavailable, true)];
        assert_eq!(check(ltc("Gone", 1)), down);
        assert_eq!(check(ltc("Main", 9)), down);
        assert_eq!(check(ltc("Main", 0)), down);
        assert_eq!(check(ltc("Old card", 1)), down);
        assert_eq!(check(ltc(UNASSIGNED_DEVICE, 1)), down);
    }

    #[test]
    fn mtc_needs_a_present_port() {
        let hw = fake_hardware();
        let p = Project::new("Show", FrameRate::Fps25);
        let check = |mtc: MtcOutput| {
            let out = OutputConfig {
                mtc: vec![mtc],
                ..Default::default()
            };
            hardware_codes(&p, &out, &hw)
        };
        assert!(check(mtc("Lights", 0)).is_empty());
        assert!(check(mtc("Hub", 1)).is_empty());
        let down = [(IssueCode::MtcUnavailable, true)];
        assert_eq!(check(mtc("Hub", 2)), down);
        assert_eq!(check(mtc("", 0)), down);
        assert_eq!(check(mtc("Gone", 0)), down);
    }

    #[test]
    fn outputs_on_the_same_channel_or_port_collide() {
        let hw = fake_hardware();
        let p = Project::new("Show", FrameRate::Fps25);
        let out = OutputConfig {
            ltc: vec![ltc("Main", 3), ltc("", 3), ltc("Main", 4)],
            mtc: vec![mtc("Hub", 0), mtc("hub", 0), mtc("Hub", 1)],
            ..Default::default()
        };
        assert_eq!(
            hardware_codes(&p, &out, &hw),
            [
                (IssueCode::LtcCollision, true),
                (IssueCode::MtcCollision, true)
            ]
        );
    }

    #[test]
    fn with_no_devices_attached_every_configured_output_is_reported_and_nothing_fails() {
        let p = cue_project(midi_track(Some(("Hub", 0)), 1));
        let out = OutputConfig {
            ltc: vec![ltc("Main", 1)],
            mtc: vec![mtc("Hub", 0)],
            ..Default::default()
        };
        let mut report = check(&p, None, READY, |_| None);
        report.extend(check_hardware(&p, &out, &Hardware::default()));
        assert_eq!(
            codes(&report),
            [
                IssueCode::MidiPortMissing,
                IssueCode::LtcUnavailable,
                IssueCode::MtcUnavailable
            ]
        );
        assert_eq!((report.errors, report.warnings), (3, 0));
        report.extend(check_hardware(
            &cue_project(midi_track(None, 0)),
            &OutputConfig::default(),
            &Hardware::default(),
        ));
        assert_eq!((report.errors, report.warnings), (3, 1));
        assert_eq!(report.issues[3].area, Area::Midi);
    }
}
