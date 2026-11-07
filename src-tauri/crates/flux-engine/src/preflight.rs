//! Read-only checks before running a show. No network requests or audio streams are started.
use cpal::traits::DeviceTrait;
use flux_core::{Clock, ENGINE_RATE};
use flux_project::{AudioAsset, OutputConfig, Project, TrackKind, VoiceStatus};
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: IssueCode,
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
    report.errors = report.issues.iter().filter(|i| i.blocking).count();
    report.warnings = report.issues.len() - report.errors;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::{Alignment, FrameRate, GeneratedVoice, Track, VoiceOrder};

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
}
