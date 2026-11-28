use super::*;

/// What only the window can do, told to it with [`CONTROL_ACTION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum WindowAction {
    /// Play with no output open: the window opens it, as its own play button does.
    Play,
    ViewEdit,
    ViewLive,
}

/// How a timeline action picks the timeline.
#[derive(Debug, Clone, Copy)]
enum Pick {
    Step(isize),
    Place(usize),
}

pub(super) struct Dispatcher {
    pub(super) app: AppHandle,
    pub(super) bindings: Vec<Binding>,
    pub(super) learning: Arc<AtomicBool>,
    pub(super) vj: VjRunner,
}

impl Dispatcher {
    /// Handles what arrives until the inputs are closed. Messages that come together (a knob
    /// turned fast) are taken as one batch, so only where a control ended is applied; while a
    /// moving control is being followed the wait is cut short to send its state on time.
    pub(super) fn run(mut self, inputs: &Receiver<Input>) {
        loop {
            let first = match self.vj.wait() {
                Some(wait) => match inputs.recv_timeout(wait) {
                    Ok(input) => Some(input),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match inputs.recv() {
                    Ok(input) => Some(input),
                    Err(_) => break,
                },
            };
            if let Some(first) = first {
                let mut commands = Vec::new();
                self.handle(first, &mut commands);
                while let Ok(input) = inputs.try_recv() {
                    self.handle(input, &mut commands);
                }
                self.vj.run(commands);
            }
            self.vj.tick();
        }
        // The gesture the last message opened is still the user's.
        self.vj.finish();
    }

    /// A VJ show takes what its mappings are the source of; anything else goes to the cue
    /// bindings and the fixed `/flux/...` addresses, as before.
    fn handle(&mut self, input: Input, commands: &mut Vec<(Target, VjCommand)>) {
        match input {
            Input::Midi(message) => {
                if self.vj.midi(&message, commands) {
                    return;
                }
                let Some(trigger) = message.trigger() else {
                    return;
                };
                if self.learning.swap(false, Ordering::Relaxed) {
                    log::info!("remote control: learned {trigger:?}");
                    let _ = self.app.emit(CONTROL_LEARNED, trigger);
                } else if let Some(action) = action_for(&self.bindings, trigger) {
                    self.execute(action);
                } else {
                    log::debug!("remote control: {trigger:?} has no action");
                }
            }
            Input::Dmx(frame) => self.vj.dmx(&frame, commands),
            Input::Key(message) => self.vj.key(&message, commands),
            Input::Osc(message) => {
                if self.vj.osc(&message, commands) {
                    return;
                }
                if let Some(action) = osc_action(&message) {
                    self.execute(action);
                }
            }
        }
    }

    fn execute(&self, action: ControlAction) {
        log::debug!("remote control: {action:?}");
        let output = self.app.state::<SharedOutput>();
        match action {
            ControlAction::Go | ControlAction::Devamp | ControlAction::CancelDevamp => {
                let action = match action {
                    ControlAction::Go => crate::show_commands::Action::Go,
                    ControlAction::Devamp => crate::show_commands::Action::Devamp,
                    _ => crate::show_commands::Action::CancelDevamp,
                };
                if let Err(e) = crate::show_commands::execute(&self.app, &output, action) {
                    log::warn!("show control failed: {e}");
                }
            }
            ControlAction::Play => self.transport(&output, Command::Play, true),
            ControlAction::Pause => self.transport(&output, Command::Pause, false),
            ControlAction::Stop => self.transport(&output, Command::Stop, false),
            ControlAction::Toggle => {
                let playing = snapshot(&output)
                    .is_some_and(|(position, _)| position.state == TransportState::Playing);
                if playing {
                    self.transport(&output, Command::Pause, false);
                } else {
                    self.transport(&output, Command::Play, true);
                }
            }
            ControlAction::NextCue
            | ControlAction::PrevCue
            | ControlAction::GotoCue(_)
            | ControlAction::StepFrames(_)
            | ControlAction::StepSeconds(_) => self.seek(&output, action),
            ControlAction::NextTimeline => self.timeline(&output, Pick::Step(1)),
            ControlAction::PrevTimeline => self.timeline(&output, Pick::Step(-1)),
            ControlAction::SelectTimeline(place) => {
                self.timeline(&output, Pick::Place(place as usize));
            }
            ControlAction::Panic => {
                self.transport(&output, Command::Stop, false);
                if let Err(err) = crate::live_commands::panic_from_app(&self.app) {
                    log::warn!("remote control: panic failed: {err}");
                }
            }
            ControlAction::ViewEdit => self.tell(WindowAction::ViewEdit),
            ControlAction::ViewLive => self.tell(WindowAction::ViewLive),
        }
    }

    fn tell(&self, action: WindowAction) {
        if let Err(err) = self.app.emit(CONTROL_ACTION, action) {
            log::warn!("remote control: the window could not be told: {err}");
        }
    }

    /// Sends `command` to the running output. With none running, only play does something: the
    /// window opens the output, which it owns, and plays.
    fn transport(&self, output: &SharedOutput, command: Command, open: bool) {
        match crate::voice_commands::send_transport(
            &self.app.state::<Arc<crate::voice_commands::VoiceState>>(),
            output,
            command,
        ) {
            Ok(()) => {}
            Err(OutputError::NotRunning) if open => self.tell(WindowAction::Play),
            Err(OutputError::NotRunning) => log::debug!("remote control: no output is open"),
            Err(err) => log::warn!("remote control: {command:?} failed: {err}"),
        }
    }

    fn seek(&self, output: &SharedOutput, action: ControlAction) {
        let Some((position, cues)) = snapshot(output) else {
            log::debug!("remote control: no output is open");
            return;
        };
        let timecode = self
            .app
            .state::<SharedSession>()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active_timecode();
        let Some(timecode) = timecode else {
            log::debug!("remote control: no project is open");
            return;
        };
        let fps = timecode.frame_rate.nominal_fps();
        match seek_target(action, position.frame, cues.cues(), timecode.start, fps) {
            Some(frame) => {
                if let Err(err) = crate::voice_commands::send_transport(
                    &self.app.state::<Arc<crate::voice_commands::VoiceState>>(),
                    output,
                    Command::Seek(frame),
                ) {
                    log::warn!("remote control: seek failed: {err}");
                }
            }
            None => log::info!("remote control: {action:?} has nowhere to go"),
        }
    }

    /// Changes the active timeline as the window's own command does: not while the show plays, and
    /// the output is released because it was opened with the old timecode. The window is told with
    /// the event it already hears when a show ends and the next timeline takes over.
    fn timeline(&self, output: &SharedOutput, pick: Pick) {
        if is_playing(output) {
            log::info!("remote control: the timelines cannot change while the show plays");
            return;
        }
        let changed = {
            let session = self.app.state::<SharedSession>();
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            match pick {
                Pick::Step(delta) => session.step_timeline(delta),
                Pick::Place(place) => session.select_timeline_at(place),
            }
        };
        match changed {
            Ok(Some(state)) => {
                tauri::async_runtime::block_on(release(output));
                let _ = self.app.emit(TIMELINE_ADVANCED, state);
            }
            Ok(None) => log::info!("remote control: there is no such timeline"),
            Err(err) => log::warn!("remote control: the timeline could not be changed: {err}"),
        }
    }
}
