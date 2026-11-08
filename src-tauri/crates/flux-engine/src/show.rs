//! Sample-clock show execution. Plans are prepared off-thread and retained for the run.
use crate::{
    mixer::ClipSet,
    transport::{Command, Transport, TransportState},
};
use flux_core::Clock;
use flux_project::{ControlKind, SetlistEntry, Timeline, Transition};
use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub struct PreparedEntry {
    pub cues: crate::cues::CueList,
    pub entry: SetlistEntry,
    pub timeline: Timeline,
    pub clips: Arc<ClipSet>,
    pub end: u64,
}

impl PreparedEntry {
    pub fn duration_samples(&self) -> Option<u64> {
        let clock = Clock::new(48_000, self.timeline.timecode.frame_rate);
        let mut total = self.end;
        for control in &self.timeline.controls {
            match control.kind {
                ControlKind::Wait | ControlKind::Loop { turns: None, .. } => return None,
                ControlKind::Loop {
                    end,
                    turns: Some(turns),
                } => {
                    let length = clock.frames_to_samples(end) - clock.frames_to_samples(control.at);
                    total = total
                        .checked_add(length.checked_mul(u64::from(turns.saturating_sub(1)))?)?;
                }
            }
        }
        Some(total)
    }
}

#[derive(Debug)]
pub struct ShowPlan {
    pub overview: flux_project::show::SetlistOverview,
    pub mix: std::sync::Mutex<std::collections::HashMap<String, (f32, bool)>>,
    pub execution_lock: Arc<AtomicBool>,
    pub setlist_id: String,
    pub entries: Vec<PreparedEntry>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ShowStatus {
    #[default]
    Ready,
    Playing,
    Paused,
    Waiting,
    Devamping,
    Delay,
    Completed,
    Blocked,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GoAction {
    #[default]
    Start,
    Resume,
    Devamp,
    None,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowTick {
    pub entry: usize,
    pub status: ShowStatus,
    pub go: GoAction,
    pub turn: u32,
    pub devamp_pending: bool,
    pub control: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub enum ShowCommand {
    Go,
    Devamp,
    CancelDevamp,
    Prepare(usize),
}

/// A rendered interval, queued to cue workers so short loops cannot lose events.
#[derive(Debug, Clone, Copy)]
pub struct ShowSpan {
    pub generation: u64,
    pub include_end: bool,
    pub entry: usize,
    pub from: u64,
    pub to: u64,
    pub epoch: u64,
    pub turn: u32,
    pub release: bool,
}

#[derive(Debug)]
pub struct ShowRunner {
    pub plan: Arc<ShowPlan>,
    pub tick: ShowTick,
    next_control: usize,
    loop_control: Option<usize>,
    waiting: Option<usize>,
    delay: u64,
    delay_paused: bool,
}

impl ShowRunner {
    pub fn new(plan: Arc<ShowPlan>, transport: &mut Transport) -> Self {
        let mut runner = Self {
            plan,
            tick: ShowTick::default(),
            next_control: 0,
            loop_control: None,
            waiting: None,
            delay: 0,
            delay_paused: false,
        };
        runner.prepare(0, transport);
        runner
    }

    pub fn current(&self) -> &PreparedEntry {
        &self.plan.entries[self.tick.entry]
    }

    fn prepare(&mut self, index: usize, transport: &mut Transport) {
        self.tick = ShowTick {
            entry: index,
            ..ShowTick::default()
        };
        self.next_control = 0;
        self.loop_control = None;
        self.waiting = None;
        self.delay = 0;
        self.delay_paused = false;
        let entry = self.current();
        transport.configure(
            Clock::new(48_000, entry.timeline.timecode.frame_rate),
            entry.timeline.timecode.start,
        );
        transport.set_end(None);
    }

    pub fn command(&mut self, command: ShowCommand, transport: &mut Transport) {
        match command {
            ShowCommand::Prepare(index)
                if transport.state() == TransportState::Stopped
                    && index < self.plan.entries.len() =>
            {
                self.prepare(index, transport)
            }
            ShowCommand::Go if self.tick.status == ShowStatus::Delay && self.delay_paused => {
                self.delay_paused = false;
                self.tick.go = GoAction::None;
            }
            ShowCommand::Go => match self.tick.go {
                GoAction::Start | GoAction::Resume => {
                    self.waiting = None;
                    transport.apply(Command::Play);
                    self.tick.status = ShowStatus::Playing;
                }
                GoAction::Devamp => self.tick.devamp_pending = true,
                GoAction::None => {}
            },
            ShowCommand::Devamp if self.loop_control.is_some() => self.tick.devamp_pending = true,
            ShowCommand::CancelDevamp => self.tick.devamp_pending = false,
            _ => {}
        }
        self.refresh(transport);
    }

    pub fn transport(&mut self, command: Command, transport: &mut Transport) {
        if self.tick.status == ShowStatus::Delay
            && matches!(command, Command::Pause | Command::Play)
        {
            self.delay_paused = matches!(command, Command::Pause);
            self.tick.go = if self.delay_paused {
                GoAction::Resume
            } else {
                GoAction::None
            };
            return;
        }
        if matches!(command, Command::Stop) {
            self.prepare(self.tick.entry, transport);
        } else if matches!(command, Command::Play)
            && (self.waiting.is_some()
                || matches!(
                    self.tick.status,
                    ShowStatus::Completed | ShowStatus::Blocked | ShowStatus::Delay
                ))
        {
            return;
        } else {
            transport.apply(command);
            if matches!(command, Command::Seek(_) | Command::Restore { .. }) {
                self.waiting = None;
                self.loop_control = None;
                self.tick.turn = 0;
                self.tick.devamp_pending = false;
                let frame = transport
                    .clock()
                    .samples_to_frames(transport.position_samples());
                self.next_control = self
                    .current()
                    .timeline
                    .controls
                    .partition_point(|c| c.at < frame);
                self.loop_control = self
                    .current()
                    .timeline
                    .controls
                    .iter()
                    .enumerate()
                    .find_map(|(i, c)| {
                        matches!(c.kind, ControlKind::Loop {end, ..} if c.at < frame && frame < end)
                            .then_some(i)
                    });
                if self.loop_control.is_some() {
                    self.tick.turn = 1;
                }
            }
        }
        self.refresh(transport);
    }

    /// Returns a nonempty slice up to the next boundary, or consumes a stationary slice.
    /// Boundary changes happen before rendering, including when they coincide with buffer ends.
    pub fn slice(&mut self, transport: &mut Transport, available: u64) -> u64 {
        if self.tick.status == ShowStatus::Delay {
            if self.delay_paused {
                return available;
            }
            if self.delay == 0 {
                self.next(transport, true);
            } else {
                let n = available.min(self.delay);
                self.delay -= n;
                return n;
            }
        }
        if transport.state() != TransportState::Playing {
            self.refresh(transport);
            return available;
        }
        loop {
            let position = transport.position_samples();
            if let Some(i) = self.loop_control {
                let control = &self.current().timeline.controls[i];
                if let ControlKind::Loop { end, turns } = control.kind {
                    let end = transport.clock().frames_to_samples(end);
                    if position >= end {
                        if self.tick.devamp_pending || turns.is_some_and(|n| self.tick.turn >= n) {
                            self.loop_control = None;
                            self.tick.devamp_pending = false;
                            self.tick.turn = 0;
                        } else {
                            let frame = self.current().timeline.timecode.start + control.at;
                            transport.apply(Command::Seek(frame));
                            self.tick.turn += 1;
                            continue;
                        }
                    } else {
                        self.refresh(transport);
                        return available.min(end - position);
                    }
                }
            }
            if let Some(control) = self.current().timeline.controls.get(self.next_control) {
                let at = transport.clock().frames_to_samples(control.at);
                if position >= at {
                    let index = self.next_control;
                    let kind = control.kind.clone();
                    self.next_control += 1;
                    match kind {
                        ControlKind::Wait => {
                            self.waiting = Some(index);
                            transport.apply(Command::Pause);
                            self.refresh(transport);
                            return available;
                        }
                        ControlKind::Loop { .. } => {
                            self.loop_control = Some(index);
                            self.tick.turn = 1;
                            continue;
                        }
                    }
                }
                self.refresh(transport);
                return available
                    .min(at - position)
                    .min(self.current().end.saturating_sub(position).max(1));
            }
            if position >= self.current().end {
                match self.current().entry.transition {
                    Transition::Wait => self.next(transport, false),
                    Transition::Continue => self.next(transport, true),
                    Transition::After { milliseconds }
                        if self.tick.entry + 1 < self.plan.entries.len() =>
                    {
                        transport.apply(Command::Pause);
                        self.delay = u64::from(milliseconds) * 48;
                        self.plan.execution_lock.store(true, Ordering::Release);
                        self.tick.status = ShowStatus::Delay;
                        self.tick.go = GoAction::None;
                        return self.slice(transport, available);
                    }
                    Transition::After { .. } => self.next(transport, true),
                }
                if transport.state() != TransportState::Playing {
                    return available;
                }
                continue;
            }
            self.refresh(transport);
            return available.min(self.current().end - position);
        }
    }

    fn next(&mut self, transport: &mut Transport, play: bool) {
        let index = self.tick.entry + 1;
        if index >= self.plan.entries.len() {
            transport.apply(Command::Pause);
            self.tick.status = ShowStatus::Completed;
            self.tick.go = GoAction::None;
        } else {
            self.prepare(index, transport);
            if play {
                transport.apply(Command::Play);
            }
            self.refresh(transport);
        }
    }

    fn refresh(&mut self, transport: &Transport) {
        self.plan.execution_lock.store(
            !matches!(self.tick.status, ShowStatus::Ready | ShowStatus::Completed)
                && transport.state() != TransportState::Stopped,
            Ordering::Release,
        );
        if matches!(
            self.tick.status,
            ShowStatus::Completed | ShowStatus::Blocked | ShowStatus::Delay
        ) {
            return;
        }
        self.tick.control = self.waiting.or(self.loop_control);
        self.tick.status = if self.waiting.is_some() {
            ShowStatus::Waiting
        } else if transport.state() == TransportState::Paused {
            ShowStatus::Paused
        } else if transport.state() == TransportState::Stopped {
            ShowStatus::Ready
        } else if self.tick.devamp_pending {
            ShowStatus::Devamping
        } else {
            ShowStatus::Playing
        };
        self.tick.go = match self.tick.status {
            ShowStatus::Ready => GoAction::Start,
            ShowStatus::Paused | ShowStatus::Waiting => GoAction::Resume,
            ShowStatus::Playing if self.loop_control.is_some() => GoAction::Devamp,
            _ => GoAction::None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::{FrameRate, TimelineControl};

    fn runner(controls: Vec<TimelineControl>) -> (ShowRunner, Transport) {
        let mut timeline = Timeline::new("Song", FrameRate::Fps25);
        timeline.controls = controls;
        let project = flux_project::Project::new("Test", FrameRate::Fps25);
        let cues = crate::cues::CueList::build(&project, &timeline, |_| None);
        let entry = PreparedEntry {
            cues,
            timeline,
            entry: SetlistEntry {
                id: "e".into(),
                timeline_id: "t".into(),
                name: String::new(),
                notes: String::new(),
                omitted: false,
                transition: Transition::Wait,
            },
            clips: Arc::new(ClipSet::default()),
            end: 19200,
        };
        let mut transport = Transport::new(Clock::new(48_000, FrameRate::Fps25), 0);
        (
            ShowRunner::new(
                Arc::new(ShowPlan {
                    overview: flux_project::show::SetlistOverview {
                        id: "s".into(),
                        name: "Show".into(),
                        entries: vec![],
                    },
                    mix: Default::default(),
                    execution_lock: Arc::new(AtomicBool::new(false)),
                    setlist_id: "s".into(),
                    entries: vec![entry],
                }),
                &mut transport,
            ),
            transport,
        )
    }

    #[test]
    fn devamp_finishes_current_turn_without_a_position_jump() {
        let (mut r, mut t) = runner(vec![TimelineControl {
            id: "l".into(),
            name: "Vamp".into(),
            at: 0,
            kind: ControlKind::Loop {
                end: 1,
                turns: None,
            },
        }]);
        r.command(ShowCommand::Go, &mut t);
        assert_eq!(r.slice(&mut t, 4000), 1920);
        t.advance(1920);
        assert_eq!(r.slice(&mut t, 4000), 1920);
        assert_eq!(t.position_samples(), 0);
        assert_eq!(r.tick.turn, 2);
        r.command(ShowCommand::Go, &mut t);
        t.advance(1920);
        assert_eq!(r.slice(&mut t, 4000), 4000);
        assert_eq!(t.position_samples(), 1920);
        assert!(!r.tick.devamp_pending);
    }

    #[test]
    fn wait_consumes_no_timeline_samples_until_go() {
        let (mut r, mut t) = runner(vec![TimelineControl {
            id: "w".into(),
            name: "Stand by".into(),
            at: 1,
            kind: ControlKind::Wait,
        }]);
        r.command(ShowCommand::Go, &mut t);
        assert_eq!(r.slice(&mut t, 4096), 1920);
        t.advance(1920);
        r.slice(&mut t, 100);
        assert_eq!(r.tick.status, ShowStatus::Waiting);
        t.advance(100);
        assert_eq!(t.position_samples(), 1920);
        r.command(ShowCommand::Go, &mut t);
        assert_eq!(r.slice(&mut t, 100), 100);
        assert_eq!(t.state(), TransportState::Playing);
    }
}
