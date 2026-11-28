use super::*;

/// The audio API and device name hint the programme's stream was opened with.
#[derive(Debug, Clone)]
pub struct ProgrammeDevice {
    driver: Option<String>,
    hint: String,
}

impl ProgrammeDevice {
    /// Whether the device is there to be opened.
    pub fn is_present(&self) -> bool {
        devices::open_audio(&self.hint, self.driver.as_deref()).is_some()
    }
}

/// A running output. Dropping it stops the threads and closes the devices.
pub struct Output {
    messages: Sender<Message>,
    /// Every clip set ever sent. The audio callback swaps sets in and must never be the one to free
    /// the old one, so this side keeps them alive until the output stops.
    clip_sets: Mutex<Vec<Arc<ClipSet>>>,
    /// The faders and peak meters of the clip set being played.
    levels: SharedLevels,
    reader: PositionReader,
    /// The tempo the metronome clicks to, when the project has a metronome output.
    metronome: Option<Arc<SharedMetronome>>,
    /// Stops the programme's stream and the cue threads. The other streams and MTC threads each
    /// have a flag of their own, so one can be closed while the rest keep running.
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// The LTC and click streams on other devices and the MTC threads.
    branches: Branches,
    /// What the programme's stream and the metronome were opened with: a project that asks for
    /// anything else needs the output opened again.
    master: MasterKey,
    click: Option<ClickKey>,
    /// Why the programme could not be built fully, and why the cue tracks could not be opened.
    programme_warnings: Vec<String>,
    cue_warnings: Vec<String>,
    /// The threads that fire the MIDI and OSC cue tracks.
    cues: CueRoutes,
    /// Whether the cue tracks fire at all. Auditioning in Edit on a preview device does not.
    fire_cues: bool,
    info: OutputInfo,
    programme_device: ProgrammeDevice,
}

impl Output {
    /// Opens the devices a project's `output` section names and starts the stream, stopped at
    /// the timeline start. The audio device is required because its clock drives everything; a
    /// missing MIDI port only produces a warning. With `fire_cues` off (an audition) the MIDI and OSC
    /// tracks stay silent.
    pub fn start(
        output: &OutputConfig,
        timeline: &Timeline,
        programme: Programme,
        fire_cues: bool,
    ) -> Result<Self, OutputError> {
        let driver = output.driver.as_deref();
        let plan = Plan::build(output, timeline, None, |a, b| same_device(driver, a, b));
        let Plan {
            master,
            click,
            programme_ltc,
            programme_click,
            metronome,
            ..
        } = &plan;

        let ctx = BranchCtx::new(output.driver.clone(), timeline);
        let BranchCtx {
            ltc_config,
            clock,
            rate,
            start,
            ..
        } = ctx;
        let mut shared_clock = SharedClock::new();
        shared_clock.operator_route = output.operator.clone();
        shared_clock.audio_config = output.clone();
        *shared_clock
            .audio_health
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner) = output
            .audio_routes
            .iter()
            .map(|b| {
                use crate::audio_routing::{AudioRouteState, AudioRouteStatus, RouteHealth};
                RouteHealth {
                    state: AudioRouteState {
                        id: b.id.clone(),
                        device: b
                            .route
                            .as_ref()
                            .map_or_else(String::new, |r| r.device_name.clone()),
                        channels: b
                            .route
                            .as_ref()
                            .map_or_else(Vec::new, |r| r.channels.clone()),
                        status: if !crate::audio_routing::used(output, b) {
                            AudioRouteStatus::Unused
                        } else if crate::audio_routing::conflict(output, b, |a, b| {
                            same_device(driver, a, b)
                        }) {
                            AudioRouteStatus::Conflict
                        } else {
                            AudioRouteStatus::Missing
                        },
                    },
                    lost: None,
                }
            })
            .collect();
        let shared = Arc::new(shared_clock);
        let stop = Arc::new(AtomicBool::new(false));
        let (messages, message_rx) = mpsc::channel();
        let Programme { set, warnings } = programme;
        let clips = Arc::new(set);
        shared.update_operator(Arc::clone(&clips));
        let levels = SharedLevels::new(clips.levels());

        let programme_device = ProgrammeDevice {
            driver: output.driver.clone(),
            hint: master.device_hint.clone(),
        };
        let spec = AudioSpec {
            driver: output.driver.clone(),
            device_hint: master.device_hint.clone(),
            needed: master.needed,
            programme: master.programme.clone(),
            clips: Arc::clone(&clips),
            ltc: sends_of(programme_ltc),
            click: programme_click.clone(),
            ltc_config,
            rate,
            start,
            clock,
        };
        // The programme's stream cannot be reopened in place: the renderer owns the receiver of
        // the transport's commands. A new output takes over when its device is back.
        let beat: Arc<AtomicU64> = Arc::default();
        let watch = Watch {
            beat: Arc::clone(&beat),
            lost: shared.lost_flag(),
            retry: false,
        };
        let (audio_thread, audio) = spawn_stream("flux-audio", Arc::clone(&stop), watch, {
            let shared = Arc::clone(&shared);
            let mut once = Some((spec, message_rx));
            move || {
                let (spec, messages) = once.take().ok_or(OutputError::NotRunning)?;
                open_stream(spec, messages, Arc::clone(&shared), Arc::clone(&beat))
            }
        })?;

        let mut branches = Branches::default();
        let mut secondary_warnings = Vec::new();
        let opened = branches.apply(&plan, &ctx, &shared, &audio.device, &mut secondary_warnings);

        // The cue tracks: one thread per MIDI port and per OSC destination.
        let mut cues = CueRoutes::new(clock, Arc::clone(&shared), Arc::clone(&stop));
        let mut cue_warnings = Vec::new();
        let cue_info = if fire_cues {
            cues.apply(timeline, &output.osc, &mut cue_warnings)
        } else {
            Vec::new()
        };

        let info = OutputInfo {
            device: audio.device,
            sample_rate: ltc_config.sample_rate,
            channels: audio.channels,
            ltc: opened.ltc,
            mtc: opened.mtc,
            metronome: opened.metronome,
            operator: opened.operator,
            cues: cue_info,
            warnings: [&warnings, &secondary_warnings, &cue_warnings]
                .into_iter()
                .flatten()
                .cloned()
                .collect(),
        };
        Ok(Self {
            messages,
            clip_sets: Mutex::new(vec![clips]),
            levels,
            reader: PositionReader {
                shared,
                clock,
                start,
            },
            metronome: metronome.clone(),
            stop,
            threads: vec![audio_thread],
            branches,
            master: master.clone(),
            click: click.clone(),
            programme_warnings: warnings,
            cue_warnings,
            cues,
            fire_cues,
            info,
            programme_device,
        })
    }

    pub fn install_show(
        &mut self,
        plan: Arc<crate::show::ShowPlan>,
        destinations: &[OscDestination],
    ) -> Result<(), OutputError> {
        if !self.info.warnings.is_empty()
            || self.reader.audio_routes().iter().any(|r| {
                matches!(
                    r.status,
                    crate::audio_routing::AudioRouteStatus::Missing
                        | crate::audio_routing::AudioRouteStatus::Conflict
                        | crate::audio_routing::AudioRouteStatus::Lost
                )
            })
        {
            return Err(OutputError::Stream("show.error.outputs".into()));
        }
        self.info.cues.clear();
        for entry in &plan.entries {
            let clock = Clock::new(ENGINE_RATE, entry.timeline.timecode.frame_rate);
            for midi in crate::midi_cue::plan(&entry.timeline, &clock) {
                let info = CueInfo {
                    kind: crate::cue_routes::CueDestination::Midi,
                    target: midi.port.port_name,
                    ok: true,
                };
                if !self.info.cues.contains(&info) {
                    self.info.cues.push(info);
                }
            }
            for osc in crate::osc::plan(&entry.timeline, destinations, &clock) {
                let info = CueInfo {
                    kind: crate::cue_routes::CueDestination::Osc,
                    target: osc.destination.name,
                    ok: true,
                };
                if !self.info.cues.contains(&info) {
                    self.info.cues.push(info);
                }
            }
        }
        let sender = crate::show_events::start(
            Arc::clone(&plan),
            destinations,
            Arc::clone(&self.stop),
            Arc::clone(&self.reader.shared),
        )?;
        self.reader
            .shared
            .show_spans
            .set(sender)
            .map_err(|_| OutputError::Stream("show.error.alreadyPrepared".into()))?;
        self.reader
            .shared
            .show_plan
            .set(plan)
            .map_err(|_| OutputError::Stream("show.error.alreadyPrepared".into()))?;
        Ok(())
    }

    pub fn show_command(&self, command: crate::show::ShowCommand) -> Result<(), OutputError> {
        self.messages
            .send(Message::Show(command))
            .map_err(|_| OutputError::NotRunning)
    }

    /// Follows a change of the project's output settings while the output runs, without touching
    /// the programme's stream: the LTC and click streams on other devices and the MTC threads
    /// that are no longer wanted are closed, the new ones opened, and the rest keep running.
    /// When the change reaches the programme's stream (its device, channels, the LTC on it, the
    /// metronome, the frame rate or start) nothing is touched and the output must be opened again.
    pub fn reconfigure(&mut self, output: &OutputConfig, timeline: &Timeline) -> Reconfigured {
        let driver = output.driver.as_deref();
        let plan = Plan::build(output, timeline, self.metronome.clone(), |a, b| {
            same_device(driver, a, b)
        });
        let ctx = BranchCtx::new(output.driver.clone(), timeline);
        if plan.master != self.master
            || plan.click != self.click
            || output.operator != self.reader.shared.operator_route
            || output.audio_routes != self.reader.shared.audio_config.audio_routes
            || output.track_routes != self.reader.shared.audio_config.track_routes
            || (!output.audio_routes.is_empty()
                && output.ltc != self.reader.shared.audio_config.ltc)
        {
            return Reconfigured::NeedsRestart;
        }
        let shared = Arc::clone(&self.reader.shared);
        let mut secondary_warnings = Vec::new();
        let device = self.info.device.clone();
        let opened = self
            .branches
            .apply(&plan, &ctx, &shared, &device, &mut secondary_warnings);
        self.info.ltc = opened.ltc;
        self.info.mtc = opened.mtc;
        self.info.metronome = opened.metronome;
        self.info.operator = opened.operator;
        self.info.warnings = [
            &self.programme_warnings,
            &secondary_warnings,
            &self.cue_warnings,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        Reconfigured::Applied {
            info: self.info.clone(),
        }
    }

    /// Follows an edit of the cue tracks while the show runs: the schedules are replaced, carrying
    /// on from the position, and a port or destination that is new is opened now. Returns the
    /// reasons for those that could not be.
    pub fn set_events(&mut self, timeline: &Timeline, osc: &[OscDestination]) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.fire_cues {
            self.cues.apply(timeline, osc, &mut warnings);
        }
        warnings
    }

    /// Follows an edit of the active timeline's tempo or click switch while the show runs.
    /// Does nothing when the project has no metronome output.
    pub fn set_tempo(&self, tempo: Tempo) {
        if let Some(metronome) = &self.metronome {
            metronome.set(tempo);
        }
    }

    /// The device the programme plays on, to find out whether it is back after it was unplugged.
    pub fn programme_device(&self) -> ProgrammeDevice {
        self.programme_device.clone()
    }

    pub fn info(&self) -> &OutputInfo {
        &self.info
    }

    pub fn send(&self, command: Command) -> Result<(), OutputError> {
        let play = matches!(command, Command::Play);
        if play {
            self.reader
                .shared
                .pending_plays
                .fetch_add(1, Ordering::AcqRel);
        }
        self.messages
            .send(Message::Transport(command))
            .map_err(|_| {
                if play {
                    self.reader
                        .shared
                        .pending_plays
                        .fetch_sub(1, Ordering::AcqRel);
                }
                OutputError::NotRunning
            })
    }

    pub fn playing_or_pending(&self) -> bool {
        self.reader.shared.pending_plays.load(Ordering::Acquire) > 0
            || self.reader.shared.load().state == TransportState::Playing
    }

    /// Replaces the audio being played, after the project's clips changed.
    pub fn set_clips(&self, set: ClipSet) -> Result<(), OutputError> {
        let set = Arc::new(set);
        self.reader.shared.update_operator(Arc::clone(&set));
        self.levels.replace(set.levels());
        let mut sets = self
            .clip_sets
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        prune_clip_sets(&mut sets);
        sets.push(Arc::clone(&set));
        drop(sets);
        self.messages
            .send(Message::Clips(set))
            .map_err(|_| OutputError::NotRunning)
    }

    pub fn reader(&self) -> PositionReader {
        self.reader.clone()
    }

    /// Replaces the sound of the VJ grid, which plays on the programme whether the transport plays
    /// or not. The callback takes it up on its next buffer, without waiting.
    pub fn set_grid_audio(&self, mix: crate::grid_audio::GridMix) {
        self.reader.shared.grid_audio.set(mix);
    }

    /// Where to move a track's fader and read its peak, whichever clip set is playing.
    pub fn levels(&self) -> SharedLevels {
        self.levels.clone()
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.branches.shut_all();
        for handle in self.threads.drain(..).chain(self.cues.threads.drain(..)) {
            handle.thread().unpark();
            let _ = handle.join();
        }
        if let Some(plan) = self.reader.shared.show_plan.get() {
            plan.execution_lock.store(false, Ordering::Release);
        }
    }
}
