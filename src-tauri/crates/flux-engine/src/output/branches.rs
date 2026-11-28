use super::*;

/// What the streams and threads besides the programme's are opened with.
pub(super) struct BranchCtx {
    pub(super) driver: Option<String>,
    /// The generator runs at full scale and every channel applies its own level.
    pub(super) ltc_config: LtcConfig,
    pub(super) rate: FrameRate,
    pub(super) start: u64,
    pub(super) clock: Clock,
    /// The tempo map of the timeline, which the MIDI clock follows.
    pub(super) tempo: flux_project::TempoMap,
}

impl BranchCtx {
    pub(super) fn new(driver: Option<String>, timeline: &Timeline) -> Self {
        let ltc_config = LtcConfig {
            level: 1.0,
            ..LtcConfig::default()
        };
        let rate = timeline.timecode.frame_rate;
        Self {
            driver,
            clock: Clock::new(ltc_config.sample_rate, rate),
            ltc_config,
            rate,
            start: timeline.timecode.start,
            tempo: flux_project::TempoMap::of(timeline),
        }
    }
}

/// A stream (LTC and/or the click) on a device that is not the programme's.
pub(super) struct Follower {
    /// The device name hint it was opened by.
    device: String,
    sends: Vec<LtcSend>,
    with_click: bool,
    /// The name of the device that opened.
    opened: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// A thread sending MTC to a MIDI port.
pub(super) struct MtcBranch {
    hint: String,
    index: usize,
    offset_ms: i32,
    /// The system's own position of the port.
    at: usize,
    name: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// The thread sending MIDI clock to a port.
pub(super) struct ClockBranch {
    want: MidiClockOutput,
    /// The name of the port that opened.
    name: String,
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Opens the MIDI port `want` names and starts the thread that sends clock to it.
fn open_midi_clock(
    want: &MidiClockOutput,
    clock: Clock,
    shared: &Arc<SharedClock>,
) -> Result<ClockBranch, String> {
    let (_, name, connection) = connect_midi(&want.port_name, want.port_index, MIDI_CLIENT_NAME)?;
    let (stop, lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let link = PortLink {
        connection,
        hint: want.port_name.clone(),
        index: want.port_index,
    };
    let mode = if want.free_run {
        crate::midi_clock::Mode::Free
    } else {
        crate::midi_clock::Mode::Follow
    };
    let (shared, flags) = (Arc::clone(shared), (Arc::clone(&stop), Arc::clone(&lost)));
    let handle = thread::Builder::new()
        .name("flux-midi-clock".into())
        .spawn(move || run_midi_clock(mode, clock, link, shared, flags.0, flags.1))
        .map_err(|err| format!("{name}: {err}"))?;
    Ok(ClockBranch {
        want: want.clone(),
        name,
        stop,
        lost,
        handle,
    })
}

/// Stops a stream or thread of its own and waits for it.
pub(super) fn shut(stop: &AtomicBool, handle: JoinHandle<()>) {
    stop.store(true, Ordering::Relaxed);
    handle.thread().unpark();
    let _ = handle.join();
}

impl Follower {
    fn shut(self) {
        shut(&self.stop, self.handle);
    }
}

impl MtcBranch {
    fn shut(self) {
        shut(&self.stop, self.handle);
    }
}

/// For each wanted item, the running one that already does it (each used once), and the running
/// ones nothing wants any more.
pub(super) fn pair<W, R>(
    wanted: &[W],
    running: &[R],
    same: impl Fn(&W, &R) -> bool,
) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut taken = vec![false; running.len()];
    let paired = wanted
        .iter()
        .map(|want| {
            let at = (0..running.len()).find(|&at| !taken[at] && same(want, &running[at]))?;
            taken[at] = true;
            Some(at)
        })
        .collect();
    let unused = (0..running.len()).filter(|&at| !taken[at]).collect();
    (paired, unused)
}

/// What the branches ended up as, for the UI.
pub(super) struct Opened {
    pub(super) ltc: Vec<LtcInfo>,
    pub(super) mtc: Vec<MtcInfo>,
    pub(super) metronome: Option<MetronomeInfo>,
    pub(super) operator: Option<MetronomeInfo>,
    pub(super) midi_clock: Option<MtcInfo>,
}

/// The LTC and click streams on other devices and the MTC threads of a running output. Applying a
/// plan keeps what already does what the plan asks, and only closes and opens the rest.
#[derive(Default)]
pub(super) struct Branches {
    pub(super) followers: Vec<Follower>,
    /// The metronome's stream, when its device carries nothing else.
    click: Option<Follower>,
    pub(super) mtc: Vec<MtcBranch>,
    /// The thread sending MIDI clock.
    clock: Option<ClockBranch>,
}

impl Branches {
    pub(super) fn shut_all(&mut self) {
        for follower in self.followers.drain(..).chain(self.click.take()) {
            follower.shut();
        }
        for port in self.mtc.drain(..) {
            port.shut();
        }
        if let Some(clock) = self.clock.take() {
            shut(&clock.stop, clock.handle);
        }
    }

    /// The thread that sends MIDI clock: kept while the port and mode are the same, replaced
    /// otherwise, closed when the project sends none. Returns how it ended up.
    fn apply_midi_clock(
        &mut self,
        plan: &Plan,
        ctx: &BranchCtx,
        shared: &Arc<SharedClock>,
        warnings: &mut Vec<String>,
    ) -> Option<MtcInfo> {
        let want = plan.midi_clock.as_ref();
        if let Some(running) = &self.clock
            && want != Some(&running.want)
            && let Some(old) = self.clock.take()
        {
            shut(&old.stop, old.handle);
        }
        let want = want?;
        if let Some(running) = &self.clock {
            return Some(MtcInfo {
                port: running.name.clone(),
                ok: !running.lost.load(Ordering::Relaxed),
            });
        }
        if want.port_name.is_empty() {
            return None;
        }
        match open_midi_clock(want, ctx.clock, shared) {
            Ok(branch) => {
                let info = MtcInfo {
                    port: branch.name.clone(),
                    ok: true,
                };
                self.clock = Some(branch);
                Some(info)
            }
            Err(reason) => {
                warnings.push(format!("MIDI clock is off, {reason}"));
                Some(MtcInfo {
                    port: want.port_name.clone(),
                    ok: false,
                })
            }
        }
    }

    pub(super) fn apply(
        &mut self,
        plan: &Plan,
        ctx: &BranchCtx,
        shared: &Arc<SharedClock>,
        programme: &str,
        warnings: &mut Vec<String>,
    ) -> Opened {
        let mut lamps = Lamps {
            programme_ltc: mask(
                plan.programme_ltc
                    .iter()
                    .flat_map(|slot| slot.outputs.iter().copied()),
            ),
            programme_click: plan.programme_click.is_some(),
            ..Lamps::default()
        };
        // One entry per project output, in the project's order, whatever stream it landed on.
        let mut ltc_info: Vec<Option<LtcInfo>> = vec![None; plan.ltc_outputs];
        for &(index, channel) in &plan.unassigned_ltc {
            ltc_info[index] = Some(LtcInfo {
                device: String::new(),
                channel,
                ok: false,
            });
        }
        let mut place = |slots: &[LtcSlot], device: &str, ok: bool| {
            for slot in slots {
                for &index in &slot.outputs {
                    ltc_info[index] = Some(LtcInfo {
                        device: device.to_owned(),
                        channel: slot.channel,
                        ok,
                    });
                }
            }
        };
        place(&plan.programme_ltc, programme, true);
        let mut metronome_info = plan.click.as_ref().map(|key| MetronomeInfo {
            device: if key.place == ClickPlaceKey::Programme {
                programme.to_owned()
            } else {
                key.device.clone()
            },
            ok: key.place == ClickPlaceKey::Programme,
        });

        // What each stream is asked to do, as it is compared with the ones running.
        let asked: Vec<(&str, Vec<LtcSend>, bool)> = plan
            .followers
            .iter()
            .map(|want| {
                (
                    want.group.device.as_str(),
                    sends_of(&want.group.slots),
                    want.click.is_some(),
                )
            })
            .collect();
        let (paired, unused) = pair(
            &asked,
            &self.followers,
            |(device, sends, click), running| {
                running.device == *device && running.sends == *sends && running.with_click == *click
            },
        );
        let mut old: Vec<Option<Follower>> = std::mem::take(&mut self.followers)
            .into_iter()
            .map(Some)
            .collect();
        // A device the new streams need may be one a closing stream holds: close first.
        for at in unused {
            if let Some(follower) = old[at].take() {
                follower.shut();
            }
        }
        for (want, paired) in plan.followers.iter().zip(paired) {
            let group = &want.group;
            let opened = match paired.and_then(|at| old[at].take()) {
                Some(running) => Ok(running),
                None => open_follower(
                    "flux-ltc",
                    &group.device,
                    sends_of(&group.slots),
                    want.click.clone(),
                    ctx,
                    shared,
                ),
            };
            match opened {
                Ok(follower) => {
                    place(&group.slots, &follower.opened, true);
                    let outputs = group
                        .slots
                        .iter()
                        .flat_map(|slot| slot.outputs.iter().copied());
                    lamps.ltc.push(Lamp::new(outputs, &follower.lost));
                    if let (true, Some(info)) = (want.click.is_some(), &mut metronome_info) {
                        *info = MetronomeInfo {
                            device: follower.opened.clone(),
                            ok: true,
                        };
                        lamps.click = Some(Arc::clone(&follower.lost));
                    }
                    self.followers.push(follower);
                }
                Err(err) => {
                    warnings.push(format!("LTC is off on {}, {err}", group.device));
                    place(&group.slots, &group.device, false);
                    if want.click.is_some() {
                        warnings.push(format!("The metronome is off on {}, {err}", group.device));
                    }
                }
            }
        }

        // A device that carries nothing but the metronome gets a stream that plays only the click.
        let running_click = match self.click.take() {
            Some(running)
                if plan
                    .own_click
                    .as_ref()
                    .is_some_and(|(device, _)| running.device == *device) =>
            {
                Some(running)
            }
            Some(running) => {
                running.shut();
                None
            }
            None => None,
        };
        if let Some((device, spec)) = &plan.own_click {
            let opened = match running_click {
                Some(running) => Ok(running),
                None => open_follower(
                    "flux-click",
                    device,
                    Vec::new(),
                    Some(spec.clone()),
                    ctx,
                    shared,
                ),
            };
            match opened {
                Ok(follower) => {
                    lamps.click = Some(Arc::clone(&follower.lost));
                    metronome_info = Some(MetronomeInfo {
                        device: follower.opened.clone(),
                        ok: true,
                    });
                    self.click = Some(follower);
                }
                Err(err) => warnings.push(format!("The metronome is off on {device}, {err}")),
            }
        }

        let operator = shared.operator_route.as_ref().map(|route| {
            let primary = same_device(
                ctx.driver.as_deref(),
                &route.device_name,
                &plan.master.device_hint,
            );
            let follower = self
                .followers
                .iter()
                .find(|f| same_device(ctx.driver.as_deref(), &route.device_name, &f.device));
            let flag = if primary && shared.operator_invalid.load(Ordering::Relaxed) {
                Arc::new(AtomicBool::new(true))
            } else if primary {
                Arc::clone(&shared.lost)
            } else {
                follower.map_or_else(|| Arc::new(AtomicBool::new(true)), |f| Arc::clone(&f.lost))
            };
            let ok = !flag.load(Ordering::Relaxed);
            *shared
                .operator_health
                .write()
                .unwrap_or_else(PoisonError::into_inner) = Some(flag);
            MetronomeInfo {
                device: if primary {
                    programme.into()
                } else {
                    route.device_name.clone()
                },
                ok,
            }
        });
        let mtc = self.apply_mtc(plan, ctx, shared, warnings, &mut lamps);
        shared.set_clock_map(ctx.tempo.clone());
        let midi_clock = self.apply_midi_clock(plan, ctx, shared, warnings);
        shared.health().replace(lamps);
        Opened {
            ltc: ltc_info.into_iter().flatten().collect(),
            mtc,
            metronome: metronome_info,
            midi_clock,
            operator,
        }
    }

    /// One thread per port, and a port listed twice (or named two ways) shares the first one's
    /// result. Returns one entry per MTC output of the project.
    fn apply_mtc(
        &mut self,
        plan: &Plan,
        ctx: &BranchCtx,
        shared: &Arc<SharedClock>,
        warnings: &mut Vec<String>,
        lamps: &mut Lamps,
    ) -> Vec<MtcInfo> {
        // The outputs naming the same port the same way are one.
        let mut first_of: HashMap<(&str, usize), usize> = HashMap::new();
        let mut distinct: Vec<&MtcOutput> = Vec::new();
        let group_of: Vec<usize> = plan
            .mtc
            .iter()
            .map(|mtc| {
                *first_of
                    .entry((mtc.port_name.as_str(), mtc.port_index))
                    .or_insert_with(|| {
                        distinct.push(mtc);
                        distinct.len() - 1
                    })
            })
            .collect();

        let (paired, unused) = pair(&distinct, &self.mtc, |want, running| {
            running.hint == want.port_name
                && running.index == want.port_index
                && running.offset_ms == want.offset_ms.unwrap_or(0)
        });
        let mut old: Vec<Option<MtcBranch>> = std::mem::take(&mut self.mtc)
            .into_iter()
            .map(Some)
            .collect();
        for at in unused {
            if let Some(port) = old[at].take() {
                port.shut();
            }
        }
        // What each distinct port ended up as: the branch that sends to it, or why it is off.
        let mut resolved: Vec<Option<Result<usize, MtcInfo>>> =
            distinct.iter().map(|_| None).collect();
        let mut running: Vec<MtcBranch> = Vec::new();
        // The ports kept come first, so a port that is new and turns out to be one of them
        // shares it instead of opening it twice.
        for (at, paired) in paired.into_iter().enumerate() {
            if let Some(port) = paired.and_then(|from| old[from].take()) {
                running.push(port);
                resolved[at] = Some(Ok(running.len() - 1));
            }
        }
        for (at, want) in distinct.iter().enumerate() {
            if resolved[at].is_some() {
                continue;
            }
            let result = open_mtc(want, ctx, shared, &mut running, warnings);
            resolved[at] = Some(result);
        }

        let branch_of = |output: usize| match &resolved[group_of[output]] {
            Some(Ok(branch)) => Some(*branch),
            _ => None,
        };
        for (branch, port) in running.iter().enumerate() {
            let outputs = (0..plan.mtc.len()).filter(|&output| branch_of(output) == Some(branch));
            lamps.mtc.push(Lamp::new(outputs, &port.lost));
        }
        let info = (0..plan.mtc.len())
            .map(|output| match &resolved[group_of[output]] {
                Some(Ok(branch)) => MtcInfo {
                    port: running[*branch].name.clone(),
                    ok: true,
                },
                Some(Err(info)) => info.clone(),
                None => MtcInfo {
                    port: plan.mtc[output].port_name.clone(),
                    ok: false,
                },
            })
            .collect();
        self.mtc = running;
        info
    }
}

/// Opens the MIDI port `want` names and starts its MTC thread, pushing it onto `running`. A port
/// that turns out to be one `running` already sends to is not opened twice. Returns the position
/// in `running` of the branch that sends to it, or why it is off.
pub(super) fn open_mtc(
    want: &MtcOutput,
    ctx: &BranchCtx,
    shared: &Arc<SharedClock>,
    running: &mut Vec<MtcBranch>,
    warnings: &mut Vec<String>,
) -> Result<usize, MtcInfo> {
    let hint = want.port_name.as_str();
    if hint.is_empty() {
        return Err(MtcInfo {
            port: String::new(),
            ok: false,
        });
    }
    let (position, name, connection) = match connect_midi(hint, want.port_index, MIDI_CLIENT_NAME) {
        Ok(opened) => opened,
        Err(reason) => {
            warnings.push(format!("MTC is off, {reason}"));
            return Err(MtcInfo {
                port: hint.to_owned(),
                ok: false,
            });
        }
    };
    if let Some(shared_with) = running.iter().position(|port| port.at == position) {
        return Ok(shared_with);
    }
    let offset_ms = want.offset_ms.unwrap_or(0);
    let scheduler = MtcScheduler::new(ctx.clock, ctx.rate, ctx.start)
        .with_offset(ctx.clock.ms_to_samples(offset_ms));
    let (stop, lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let link = PortLink {
        connection,
        hint: hint.to_owned(),
        index: want.port_index,
    };
    let (clock, shared, flags) = (
        ctx.clock,
        Arc::clone(shared),
        (Arc::clone(&stop), Arc::clone(&lost)),
    );
    let spawned = thread::Builder::new()
        .name("flux-mtc".into())
        .spawn(move || run_mtc(scheduler, clock, link, shared, flags.0, flags.1));
    match spawned {
        Ok(handle) => {
            running.push(MtcBranch {
                hint: hint.to_owned(),
                index: want.port_index,
                offset_ms,
                at: position,
                name,
                stop,
                lost,
                handle,
            });
            Ok(running.len() - 1)
        }
        Err(err) => {
            warnings.push(format!("MTC is off on {name}: {err}"));
            Err(MtcInfo {
                port: name,
                ok: false,
            })
        }
    }
}

/// Opens the stream of an LTC (or click) device that is not the programme's.
pub(super) fn open_follower(
    name: &str,
    device: &str,
    sends: Vec<LtcSend>,
    click: Option<ClickSpec>,
    ctx: &BranchCtx,
    shared: &Arc<SharedClock>,
) -> Result<Follower, OutputError> {
    let with_click = click.is_some();
    let spec = LtcSpec {
        driver: ctx.driver.clone(),
        device_hint: device.to_owned(),
        ltc: sends.clone(),
        click,
        config: ctx.ltc_config,
        rate: ctx.rate,
        start: ctx.start,
        clock: ctx.clock,
    };
    let (stop, lost) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );
    let (handle, opened) = spawn_follower(name, &stop, shared, &lost, spec)?;
    Ok(Follower {
        device: device.to_owned(),
        sends,
        with_click,
        opened,
        stop,
        lost,
        handle,
    })
}
