//! Opens the MIDI ports and OSC sockets the cue tracks send to and keeps one firing thread on each.
//!
//! A port or destination that cannot be opened is a warning, never an error: the project still
//! plays. A schedule can be replaced while the show runs (the project was edited); a port or
//! destination that is new at that moment is opened then.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};

use flux_core::Clock;
use flux_project::{DmxDestination, MidiPortRef, OscDestination, Timeline};
use serde::Serialize;

use super::dmx::{self, DmxSink, SharedPlan, run_dmx};
use super::fire::{Schedule, SharedSchedule, Sink, run_fire};
use super::midi_cue::{self, MidiSink};
use super::midi_panic::Served;
use super::osc::{self, OscSink};
use super::output::{SharedClock, connect_midi};

type Payload = Vec<u8>;

/// Which kind of cue destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CueDestination {
    Midi,
    Osc,
    Dmx,
}

/// How one destination of the cue tracks ended up. When it did not open, `target` is the name the
/// project holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueInfo {
    pub kind: CueDestination,
    pub target: String,
    pub ok: bool,
}

const MIDI_CLIENT_NAME: &str = "FLUX cues";

/// The firing threads of an output and what they are sent.
pub(crate) struct CueRoutes {
    clock: Clock,
    shared: Arc<SharedClock>,
    stop: Arc<AtomicBool>,
    midi: Vec<(MidiPortRef, Arc<SharedSchedule<Payload>>)>,
    osc: Vec<(String, Arc<SharedSchedule<Payload>>)>,
    dmx: Vec<(String, Arc<SharedPlan>)>,
    pub(crate) threads: Vec<JoinHandle<()>>,
}

impl CueRoutes {
    pub(crate) fn new(clock: Clock, shared: Arc<SharedClock>, stop: Arc<AtomicBool>) -> Self {
        Self {
            clock,
            shared,
            stop,
            midi: Vec::new(),
            osc: Vec::new(),
            dmx: Vec::new(),
            threads: Vec::new(),
        }
    }

    /// Points every route at the schedule `timeline` makes of it now, opening what is new. A route
    /// the timeline no longer sends to goes quiet. Returns how each destination of the timeline
    /// ended up, and pushes the reasons of those that did not onto `warnings`.
    pub(crate) fn apply(
        &mut self,
        timeline: &Timeline,
        osc_destinations: &[OscDestination],
        dmx_destinations: &[DmxDestination],
        warnings: &mut Vec<String>,
    ) -> Vec<CueInfo> {
        let mut info = Vec::new();
        let midi_plans = midi_cue::plan(timeline, &self.clock);
        let osc_plans = osc::plan(timeline, osc_destinations, &self.clock);

        for (port, schedule) in &self.midi {
            if !midi_plans.iter().any(|plan| &plan.port == port) {
                schedule.replace(Schedule::default());
            }
        }
        for plan in midi_plans {
            let target = plan.port.port_name.clone();
            let ok = if let Some((_, schedule)) = self.midi.iter().find(|(p, _)| *p == plan.port) {
                schedule.replace(plan.schedule);
                true
            } else {
                match connect_midi(&plan.port.port_name, plan.port.port_index, MIDI_CLIENT_NAME) {
                    Ok((_, _, connection)) => {
                        let schedule = Arc::new(SharedSchedule::new(plan.schedule));
                        let served = Served::new(plan.port.clone());
                        self.spawn("flux-midi", &schedule, MidiSink::new(connection), served)
                            .map(|()| self.midi.push((plan.port.clone(), schedule)))
                            .map_err(|err| {
                                warnings.push(format!("MIDI cues are off on {target}: {err}"))
                            })
                            .is_ok()
                    }
                    Err(reason) => {
                        warnings.push(format!("MIDI cues are off, {reason}"));
                        false
                    }
                }
            };
            info.push(CueInfo {
                kind: CueDestination::Midi,
                target,
                ok,
            });
        }

        for (id, schedule) in &self.osc {
            if !osc_plans.iter().any(|plan| &plan.destination.id == id) {
                schedule.replace(Schedule::default());
            }
        }
        for plan in osc_plans {
            let id = plan.destination.id.clone();
            let target = plan.destination.name.clone();
            let ok = if let Some((_, schedule)) = self.osc.iter().find(|(known, _)| *known == id) {
                schedule.replace(plan.schedule);
                true
            } else {
                match OscSink::connect(&plan.destination) {
                    Ok(sink) => {
                        let schedule = Arc::new(SharedSchedule::new(plan.schedule));
                        self.spawn("flux-osc", &schedule, sink, ())
                            .map(|()| self.osc.push((id, schedule)))
                            .map_err(|err| warnings.push(format!("OSC is off on {target}: {err}")))
                            .is_ok()
                    }
                    Err(reason) => {
                        warnings.push(format!("OSC is off on {target}, {reason}"));
                        false
                    }
                }
            };
            info.push(CueInfo {
                kind: CueDestination::Osc,
                target,
                ok,
            });
        }

        let dmx_plans = dmx::plan(timeline, dmx_destinations, &self.clock);
        for (id, shared) in &self.dmx {
            if !dmx_plans.iter().any(|plan| &plan.destination.id == id) {
                shared.clear();
            }
        }
        for plan in dmx_plans {
            let id = plan.destination.id.clone();
            let target = plan.destination.name.clone();
            let ok = if let Some((_, shared)) = self.dmx.iter().find(|(known, _)| *known == id) {
                shared.replace(plan);
                true
            } else {
                match DmxSink::connect(&plan.destination) {
                    Ok(sink) => {
                        let shared = Arc::new(SharedPlan::new(plan));
                        self.spawn_dmx(&shared, sink)
                            .map(|()| self.dmx.push((id, shared)))
                            .map_err(|err| warnings.push(format!("DMX is off on {target}: {err}")))
                            .is_ok()
                    }
                    Err(reason) => {
                        warnings.push(format!("DMX is off on {target}, {reason}"));
                        false
                    }
                }
            };
            info.push(CueInfo {
                kind: CueDestination::Dmx,
                target,
                ok,
            });
        }
        info
    }

    fn spawn_dmx(&mut self, plan: &Arc<SharedPlan>, sink: DmxSink) -> std::io::Result<()> {
        let (plan, shared, stop, clock) = (
            Arc::clone(plan),
            Arc::clone(&self.shared),
            Arc::clone(&self.stop),
            self.clock,
        );
        let handle = thread::Builder::new()
            .name("flux-dmx".into())
            .spawn(move || run_dmx(&plan, clock, sink, &shared, &stop))?;
        self.threads.push(handle);
        Ok(())
    }

    /// `held` lives as long as the thread does (the mark of a port it holds open, or nothing).
    fn spawn<S: Sink<Payload> + Send + 'static, H: Send + 'static>(
        &mut self,
        name: &str,
        schedule: &Arc<SharedSchedule<Payload>>,
        sink: S,
        held: H,
    ) -> std::io::Result<()> {
        let (schedule, shared, stop, clock) = (
            Arc::clone(schedule),
            Arc::clone(&self.shared),
            Arc::clone(&self.stop),
            self.clock,
        );
        let handle = thread::Builder::new().name(name.into()).spawn(move || {
            let _held = held;
            run_fire(&schedule, clock, sink, &shared, &stop);
        })?;
        self.threads.push(handle);
        Ok(())
    }
}
