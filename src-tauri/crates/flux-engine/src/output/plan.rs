use super::*;

/// What the programme's stream is opened with. A project that asks for anything else needs the
/// stream reopened. The positions of the LTC outputs in the project are not part of it: they only
/// say which row of the UI a stream belongs to.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MasterKey {
    driver: Option<String>,
    pub(super) device_hint: String,
    /// 1-based interface channels of the programme.
    pub(super) programme: Vec<u16>,
    /// How many channels the stream has to open.
    pub(super) needed: u16,
    /// The LTC outputs written into the programme's stream.
    ltc: Vec<LtcSend>,
    rate: FrameRate,
    start: u64,
}

/// Where the metronome plays and how, which is part of every stream it can play on.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ClickKey {
    pub(super) device: String,
    channels: Vec<u16>,
    gain_db: f32,
    pub(super) place: ClickPlaceKey,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ClickPlaceKey {
    Programme,
    /// The stream of the LTC device the group was opened by.
    Group(String),
    Own,
}

/// An LTC device that is not the programme's, with the metronome when it plays there too.
pub(super) struct FollowerWant {
    pub(super) group: LtcGroup,
    pub(super) click: Option<ClickSpec>,
}

/// What a project asks for, split into what needs the programme's stream and what does not.
pub(super) struct Plan {
    pub(super) master: MasterKey,
    pub(super) click: Option<ClickKey>,
    /// The LTC outputs on the programme's stream.
    pub(super) programme_ltc: Vec<LtcSlot>,
    pub(super) programme_click: Option<ClickSpec>,
    pub(super) followers: Vec<FollowerWant>,
    /// The metronome's device, when it carries nothing else.
    pub(super) own_click: Option<(String, ClickSpec)>,
    /// The metronome, shared with every stream that plays it.
    pub(super) metronome: Option<Arc<SharedMetronome>>,
    pub(super) mtc: Vec<MtcOutput>,
    pub(super) midi_clock: Option<MidiClockOutput>,
    /// How many LTC outputs the project has.
    pub(super) ltc_outputs: usize,
    pub(super) unassigned_ltc: Vec<(usize, u16)>,
}

impl Plan {
    /// `metronome` is the one a running output already uses, so a stream opened later keeps
    /// following its tempo. `same` tells whether two device name hints open the same device.
    pub(super) fn build(
        output: &OutputConfig,
        timeline: &Timeline,
        metronome: Option<Arc<SharedMetronome>>,
        same: impl Fn(&str, &str) -> bool,
    ) -> Self {
        // An empty device name is the system's default output, so a project without any device
        // chosen still plays. A project written before `output.audio` existed carries the
        // programme on the first LTC device.
        let device_hint = match (&output.audio, output.ltc.first()) {
            (Some(audio), _) => audio.device_name.clone(),
            (None, Some(ltc)) => ltc.device_name.clone(),
            (None, None) => String::new(),
        };
        let programme: Vec<u16> = output
            .audio
            .as_ref()
            .map(|audio| audio.channels.clone())
            .filter(|channels| !channels.is_empty())
            .unwrap_or_else(|| vec![1, 2]);
        // LTC shares the programme's stream unless it names another device, which then gets a
        // stream of its own.
        let LtcPlan {
            shared: programme_ltc,
            mut own,
        } = plan_ltc(&output.ltc, &device_hint, &same);
        // The metronome has an output of its own. It plays on the programme's stream when it names
        // the programme's device, on the stream of an LTC device it shares, or else on a stream
        // of its own: no device is opened twice.
        let operator_channels = output
            .operator
            .as_ref()
            .filter(|_| output.audio_routes.is_empty())
            .filter(|r| same(&r.device_name, &device_hint))
            .map_or_else(Vec::new, |r| r.channels.clone());
        if let Some(route) = &output.operator
            && output.audio_routes.is_empty()
            && !same(&route.device_name, &device_hint)
            && !own.iter().any(|g| same(&g.device, &route.device_name))
        {
            own.push(LtcGroup {
                device: route.device_name.clone(),
                slots: Vec::new(),
            });
        }
        for bus in output
            .audio_routes
            .iter()
            .filter(|b| crate::audio_routing::used(output, b))
        {
            if let Some(route) = &bus.route
                && route.device_name != flux_project::UNASSIGNED_DEVICE
                && !same(&route.device_name, &device_hint)
                && !own.iter().any(|g| same(&g.device, &route.device_name))
            {
                own.push(LtcGroup {
                    device: route.device_name.clone(),
                    slots: Vec::new(),
                });
            }
        }
        let shared_metronome = output.metronome.as_ref().map(|metronome_output| {
            metronome.unwrap_or_else(|| {
                Arc::new(SharedMetronome::new(
                    timeline.tempo,
                    metronome_output.gain_db,
                ))
            })
        });
        let click = output
            .metronome
            .as_ref()
            .zip(shared_metronome.as_ref())
            .map(|(metronome, shared)| {
                let channels = if metronome.channels.is_empty() {
                    vec![1, 2]
                } else {
                    metronome.channels.clone()
                };
                let place = if same(&metronome.device_name, &device_hint) {
                    ClickPlace::Programme
                } else if let Some(group) = own
                    .iter()
                    .position(|group| same(&group.device, &metronome.device_name))
                {
                    ClickPlace::Group(group)
                } else {
                    ClickPlace::Own
                };
                let spec = ClickSpec {
                    shared: Arc::clone(shared),
                    channels,
                };
                (
                    metronome.device_name.clone(),
                    metronome.gain_db,
                    place,
                    spec,
                )
            });
        let click_key = click
            .as_ref()
            .map(|(device, gain_db, place, spec)| ClickKey {
                device: device.clone(),
                channels: spec.channels.clone(),
                gain_db: *gain_db,
                place: match place {
                    ClickPlace::Programme => ClickPlaceKey::Programme,
                    ClickPlace::Group(at) => ClickPlaceKey::Group(own[*at].device.clone()),
                    ClickPlace::Own => ClickPlaceKey::Own,
                },
            });
        let click_for = |wanted: ClickPlace| {
            click
                .as_ref()
                .filter(|(_, _, place, _)| *place == wanted)
                .map(|(_, _, _, spec)| spec.clone())
        };
        let programme_click = click_for(ClickPlace::Programme);
        let own_click = click_for(ClickPlace::Own)
            .zip(click.as_ref())
            .map(|(spec, (device, ..))| (device.clone(), spec));
        let needed = programme_ltc
            .iter()
            .map(|slot| slot.channel)
            .chain(programme.iter().copied())
            .chain(operator_channels.iter().copied())
            .chain(
                output
                    .audio_routes
                    .iter()
                    .filter(|b| crate::audio_routing::used(output, b))
                    .filter_map(|b| b.route.as_ref())
                    .filter(|r| same(&r.device_name, &device_hint))
                    .flat_map(|r| r.channels.iter().copied()),
            )
            .chain(
                programme_click
                    .iter()
                    .flat_map(|spec| spec.channels.iter().copied()),
            )
            .max()
            .unwrap_or(2);
        let followers = own
            .into_iter()
            .enumerate()
            .map(|(index, group)| FollowerWant {
                group,
                click: click_for(ClickPlace::Group(index)),
            })
            .collect();
        Self {
            master: MasterKey {
                driver: output.driver.clone(),
                device_hint,
                programme,
                needed,
                ltc: sends_of(&programme_ltc),
                rate: timeline.timecode.frame_rate,
                start: timeline.timecode.start,
            },
            click: click_key,
            programme_ltc,
            programme_click,
            followers,
            own_click,
            metronome: shared_metronome,
            mtc: output.mtc.clone(),
            midi_clock: output.midi_clock.clone(),
            ltc_outputs: output.ltc.len(),
            unassigned_ltc: output
                .ltc
                .iter()
                .enumerate()
                .filter(|(_, r)| r.device_name == flux_project::UNASSIGNED_DEVICE)
                .map(|(i, r)| (i, r.channel))
                .collect(),
        }
    }
}
