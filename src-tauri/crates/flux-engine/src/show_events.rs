//! Bounded render journal for show cues, including loops shorter than a device buffer.
use crate::{
    fire::{Schedule, Sink},
    midi_cue::{self, MidiSink},
    osc::{self, OscSink},
    output::{OutputError, SharedClock, connect_midi},
    show::{ShowPlan, ShowSpan},
};
use flux_core::Clock;
use flux_project::{OscDestination, TrackKind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread;
use std::time::Duration;

type TurnSchedules = (Schedule<Vec<u8>>, Schedule<Vec<u8>>);

struct Destination {
    sink: Box<dyn Sink<Vec<u8>> + Send>,
    schedules: Vec<TurnSchedules>,
}

pub(crate) fn start(
    plan: Arc<ShowPlan>,
    destinations: &[OscDestination],
    stop: Arc<AtomicBool>,
    shared: Arc<SharedClock>,
) -> Result<SyncSender<ShowSpan>, OutputError> {
    let mut ports = Vec::new();
    for entry in &plan.entries {
        let clock = Clock::new(48_000, entry.timeline.timecode.frame_rate);
        for midi in midi_cue::plan(&entry.timeline, &clock) {
            if !ports.contains(&midi.port) {
                ports.push(midi.port);
            }
        }
    }
    let mut routes = Vec::new();
    for port in ports {
        let (_, _, link) = connect_midi(&port.port_name, port.port_index, "FLUX show cues")
            .map_err(OutputError::Stream)?;
        let schedules = plan
            .entries
            .iter()
            .map(|entry| {
                let clock = Clock::new(48_000, entry.timeline.timecode.frame_rate);
                let get = |timeline: &flux_project::Timeline| {
                    midi_cue::plan(timeline, &clock)
                        .into_iter()
                        .find(|p| p.port == port)
                        .map_or_else(Schedule::default, |p| p.schedule)
                };
                (get(&entry.timeline), get(&repeated(&entry.timeline)))
            })
            .collect();
        routes.push(Destination {
            sink: Box::new(MidiSink::new(link)),
            schedules,
        });
    }
    for destination in destinations {
        if !plan.entries.iter().any(|e| e.timeline.tracks.iter().any(|t| matches!(&t.kind, TrackKind::Osc { destination: Some(id), .. } if id == &destination.id))) { continue; }
        let sink = OscSink::connect(destination).map_err(|e| OutputError::Stream(e.to_string()))?;
        let schedules = plan
            .entries
            .iter()
            .map(|entry| {
                let clock = Clock::new(48_000, entry.timeline.timecode.frame_rate);
                let get = |timeline: &flux_project::Timeline| {
                    osc::plan(timeline, std::slice::from_ref(destination), &clock)
                        .into_iter()
                        .next()
                        .map_or_else(Schedule::default, |p| p.schedule)
                };
                (get(&entry.timeline), get(&repeated(&entry.timeline)))
            })
            .collect();
        routes.push(Destination {
            sink: Box::new(sink),
            schedules,
        });
    }
    let (tx, rx) = mpsc::sync_channel::<ShowSpan>(4096);
    thread::Builder::new()
        .name("flux-show-cues".into())
        .spawn(move || {
            let mut epoch = None;
            while !stop.load(Ordering::Relaxed) {
                let span = match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(span) => span,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if span.generation != shared.show_generation.load(Ordering::Acquire) {
                    for destination in &mut routes {
                        destination.sink.release();
                    }
                    continue;
                }
                let reset = epoch != Some(span.epoch) || span.release;
                epoch = Some(span.epoch);
                for destination in &mut routes {
                    if reset {
                        destination.sink.release();
                    }
                    let (first, repeat) = &destination.schedules[span.entry];
                    let schedule = if span.turn > 1 { repeat } else { first };
                    let items = schedule.items();
                    let from = items.partition_point(|e| e.at < span.from);
                    for event in items[from..]
                        .iter()
                        .take_while(|e| e.at < span.to || (span.include_end && e.at == span.to))
                    {
                        if !destination.sink.send(&event.payload) {
                            shared.cue_failed();
                        }
                    }
                }
            }
            for destination in &mut routes {
                destination.sink.release();
            }
        })
        .map_err(|e| OutputError::Stream(e.to_string()))?;
    Ok(tx)
}

fn repeated(timeline: &flux_project::Timeline) -> flux_project::Timeline {
    let mut timeline = timeline.clone();
    for track in &mut timeline.tracks {
        match &mut track.kind {
            TrackKind::Midi { events, .. } => events.retain(|e| !e.first_turn_only),
            TrackKind::Osc { events, .. } => events.retain(|e| !e.first_turn_only),
            _ => {}
        }
    }
    timeline
}
