use super::*;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Change {
    Microphone {
        microphone: Microphone,
    },
    LtcIn {
        input: Option<Microphone>,
    },
    MidiClock {
        port: Option<String>,
        #[serde(default)]
        index: usize,
    },
    MtcIn {
        port: Option<String>,
    },
    Analysis {
        source: Option<AnalysisSource>,
    },
    AudioRoute {
        id: String,
        field: String,
        text: String,
    },
    Voice {
        voice: Option<String>,
    },
    VideoDisplay {
        #[serde(default = "main_screen")]
        screen: String,
        display: Option<String>,
    },
    SnapshotDir {
        dir: Option<String>,
    },
    SnapshotMedia {
        on: bool,
    },
    RecordingsDir {
        dir: Option<String>,
    },
    Locale {
        locale: Locale,
        #[serde(default)]
        only_if_unset: bool,
    },
    Offset {
        kind: String,
        slot: u32,
        text: String,
    },
    Import {
        routes: Box<OutputConfig>,
    },
    Route {
        kind: String,
        field: String,
        text: String,
        #[serde(default)]
        slot: u32,
    },
}

impl Preferences {
    /// The display the screen `screen` of the video output opens on.
    pub fn video_display_for(&self, screen: &str) -> Option<String> {
        self.video_displays.get(screen).cloned().or_else(|| {
            (screen == flux_project::mapping::MAIN_SCREEN)
                .then(|| self.video_display.clone())
                .flatten()
        })
    }

    pub(super) fn apply(&mut self, change: Change) -> Result<(), String> {
        match change {
            Change::Microphone { microphone } => self.microphone = Some(microphone),
            Change::LtcIn { input } => self.ltc_in = input,
            Change::MtcIn { port } => self.mtc_in = port,
            Change::MidiClock { port, index } => {
                self.midi_clock = port
                    .filter(|port| !port.is_empty())
                    .map(|port_name| MtcOutput {
                        port_name,
                        port_index: index,
                        offset_ms: None,
                    });
            }
            Change::Analysis { source } => self.analysis = source,
            Change::AudioRoute { id, field, text } => {
                if !flux_project::routing::valid_id(&id) {
                    return Err("invalid audio output id".into());
                }
                let mut local = self.clone();
                local.audio = self.audio_routes.get(&id).cloned();
                local.edit_route("audio", &field, &text, 0)?;
                if let Some(route) = local.audio {
                    self.audio_routes.insert(id, route);
                } else {
                    self.audio_routes.remove(&id);
                }
            }
            Change::Voice { voice } => self.voice = voice,
            Change::SnapshotDir { dir } => {
                self.snapshot_dir = dir.filter(|dir| !dir.trim().is_empty());
            }
            Change::SnapshotMedia { on } => self.snapshot_media = on,
            Change::RecordingsDir { dir } => {
                self.recordings_dir = dir.filter(|dir| !dir.trim().is_empty());
            }
            Change::VideoDisplay { screen, display } => {
                if screen == flux_project::mapping::MAIN_SCREEN {
                    self.video_display = None;
                }
                match display {
                    Some(display) => self.video_displays.insert(screen, display),
                    None => self.video_displays.remove(&screen),
                };
            }
            Change::Route {
                kind,
                field,
                text,
                slot,
            } => return self.edit_route(&kind, &field, &text, slot),
            Change::Locale {
                locale,
                only_if_unset,
            } => {
                if !only_if_unset || self.locale.is_none() {
                    self.locale = Some(locale);
                }
            }
            Change::Offset { kind, slot, text } => {
                let offset: i32 = text.trim().parse().map_err(|_| "invalid latency offset")?;
                if offset.unsigned_abs() > flux_project::MAX_OFFSET_MS as u32 {
                    return Err("latency must be between -1000 and 1000 ms".into());
                }
                let target = match kind.as_str() {
                    "ltc" => self.ltc.get_mut(&slot).map(|r| &mut r.offset_ms),
                    "mtc" => self.mtc.get_mut(&slot).map(|r| &mut r.offset_ms),
                    _ => None,
                }
                .ok_or("assign an output before setting its latency")?;
                *target = (offset != 0).then_some(offset);
            }
            Change::Import { routes } => {
                routes.validate().map_err(|e| e.to_string())?;
                if routes.driver.is_some() {
                    self.driver = routes.driver;
                }
                if routes.audio.is_some() {
                    self.audio = routes.audio;
                }
                if routes.preview.is_some() {
                    self.preview = routes.preview;
                }
                if let Some(m) = routes.metronome {
                    self.metronome = Some(AudioOutput {
                        device_name: m.device_name,
                        channels: m.channels,
                    });
                }
                for (i, mut r) in routes.ltc.into_iter().enumerate() {
                    r.level = None;
                    self.ltc.insert(i as u32 + 1, r);
                }
                for (i, r) in routes.mtc.into_iter().enumerate() {
                    self.mtc.insert(i as u32 + 1, r);
                }
            }
        }
        validate(self)
    }

    pub(super) fn edit_route(
        &mut self,
        kind: &str,
        field: &str,
        text: &str,
        slot: u32,
    ) -> Result<(), String> {
        if kind == "driver" {
            self.driver = (!text.is_empty()).then(|| text.to_owned());
            // A host switch must not silently reroute timecode to its default device.
            self.audio = None;
            self.preview = None;
            self.operator = None;
            self.metronome = None;
            self.audio_routes.clear();
            self.ltc.clear();
            return Ok(());
        }
        if field == "offset" {
            return self.apply(Change::Offset {
                kind: kind.into(),
                slot,
                text: text.into(),
            });
        }
        if kind == "mtc" {
            if field != "port" {
                return Err("unknown route field".into());
            }
            if slot == 0 {
                return Err("invalid output slot".into());
            }
            if text == UNASSIGNED {
                self.mtc.remove(&slot);
            } else {
                let (index, name) = text.split_once('|').ok_or("invalid MIDI port")?;
                let port_index = index.parse().map_err(|_| "invalid MIDI port index")?;
                let offset_ms = self.mtc.get(&slot).and_then(|r| r.offset_ms);
                self.mtc.insert(
                    slot,
                    MtcOutput {
                        port_name: name.into(),
                        port_index,
                        offset_ms,
                    },
                );
            }
        } else if kind == "ltc" {
            if slot == 0 {
                return Err("invalid output slot".into());
            }
            if field == "device" && text == UNASSIGNED {
                self.ltc.remove(&slot);
            } else {
                let taken: Vec<u16> = self
                    .ltc
                    .iter()
                    .filter(|(key, r)| **key != slot && r.device_name == text)
                    .map(|(_, r)| r.channel)
                    .chain(
                        self.audio
                            .iter()
                            .filter(|r| r.device_name == text)
                            .flat_map(|r| r.channels.iter().copied()),
                    )
                    .collect();
                let r = self.ltc.entry(slot).or_insert(LtcOutput {
                    device_name: UNASSIGNED.into(),
                    channel: 1,
                    level: None,
                    offset_ms: None,
                });
                match field {
                    "device" => {
                        r.device_name = text.into();
                        let count = flux_engine::devices::list_audio(self.driver.as_deref())
                            .iter()
                            .find(|d| d.name == text || (text.is_empty() && d.is_default))
                            .map_or(2, |d| d.output_channels);
                        let top = count.clamp(1, 8);
                        r.channel = (1..=top).rev().find(|c| !taken.contains(c)).unwrap_or(top);
                    }
                    "channel" => r.channel = text.parse().map_err(|_| "invalid audio channel")?,
                    _ => return Err("unknown route field".into()),
                }
            }
        } else {
            let r = match kind {
                "audio" => &mut self.audio,
                "preview" => &mut self.preview,
                "metronome" => &mut self.metronome,
                "operator" => &mut self.operator,
                _ => return Err("unknown route".into()),
            };
            if field == "device" {
                *r = if text == UNASSIGNED {
                    None
                } else {
                    let count = flux_engine::devices::list_audio(self.driver.as_deref())
                        .iter()
                        .find(|d| d.name == text || (text.is_empty() && d.is_default))
                        .map_or(2, |d| d.output_channels);
                    Some(AudioOutput {
                        device_name: text.into(),
                        channels: if count >= 2 { vec![1, 2] } else { vec![1] },
                    })
                };
            } else if field == "channels" {
                let channels = text
                    .split('-')
                    .map(str::parse)
                    .collect::<Result<Vec<u16>, _>>()
                    .map_err(|_| "invalid audio channels")?;
                r.as_mut().ok_or("assign a device first")?.channels = channels;
            } else {
                return Err("unknown route field".into());
            }
        }
        validate(self)
    }

    /// Language and persistence metadata never require an output change.
    pub fn same_routing(&self, other: &Self) -> bool {
        self.audio_routes == other.audio_routes
            && self.driver == other.driver
            && self.audio == other.audio
            && self.operator == other.operator
            && self.preview == other.preview
            && self.metronome == other.metronome
            && self.ltc == other.ltc
            && self.mtc == other.mtc
            && self.midi_clock == other.midi_clock
    }

    pub fn resolve_project(
        &self,
        project: &flux_project::Project,
        timeline: &flux_project::Timeline,
        audition: bool,
    ) -> OutputConfig {
        use flux_project::routing::{
            OPERATOR, PROGRAMME, ResolvedAudioBus, TrackRoute, audible, target,
        };
        let mut config = self.resolve(&project.output, audition);
        // A VJ show's clock runs all the time; a cue show's runs with its transport.
        if let Some(clock) = &mut config.midi_clock {
            clock.free_run = project.mode == flux_project::ProjectMode::Vj;
        }
        config.audio_routes = vec![
            ResolvedAudioBus {
                id: PROGRAMME.into(),
                route: config.audio.clone(),
            },
            ResolvedAudioBus {
                id: OPERATOR.into(),
                route: config.operator.clone(),
            },
        ];
        config
            .audio_routes
            .extend(project.output.audio.iter().map(|b| ResolvedAudioBus {
                id: b.id.clone(),
                route: if audition {
                    self.preview.clone()
                } else {
                    self.audio_routes.get(&b.id).cloned()
                },
            }));
        config.track_routes = timeline
            .tracks
            .iter()
            .filter(|t| audible(t))
            .map(|t| TrackRoute {
                track_id: t.id.clone(),
                output: target(t).map(str::to_owned),
            })
            .collect();
        config
            .track_routes
            .sort_by(|a, b| a.track_id.cmp(&b.track_id));
        config
    }

    pub fn resolve(&self, show: &ShowOutputs, audition: bool) -> OutputConfig {
        OutputConfig {
            audio_routes: Vec::new(),
            track_routes: Vec::new(),
            driver: self.driver.clone(),
            operator: if audition {
                self.preview.clone()
            } else {
                self.operator.clone()
            },
            audio: if audition {
                self.preview.clone()
            } else {
                self.audio.clone().or_else(|| {
                    Some(AudioOutput {
                        device_name: String::new(),
                        channels: vec![1, 2],
                    })
                })
            },
            preview: if audition { None } else { self.preview.clone() },
            metronome: show.metronome.as_ref().zip(self.metronome.as_ref()).map(
                |(signal, route)| MetronomeOutput {
                    device_name: route.device_name.clone(),
                    channels: route.channels.clone(),
                    gain_db: signal.gain_db,
                },
            ),
            ltc: if audition {
                vec![]
            } else {
                show.ltc
                    .iter()
                    .map(|s| {
                        let mut r = self.ltc.get(&s.slot).cloned().unwrap_or(LtcOutput {
                            device_name: UNASSIGNED.into(),
                            channel: 1,
                            level: None,
                            offset_ms: None,
                        });
                        r.level = s.level;
                        r
                    })
                    .collect()
            },
            mtc: if audition {
                vec![]
            } else {
                show.mtc
                    .iter()
                    .map(|s| {
                        self.mtc.get(&s.slot).cloned().unwrap_or(MtcOutput {
                            port_name: String::new(),
                            port_index: 0,
                            offset_ms: None,
                        })
                    })
                    .collect()
            },
            osc: show.osc.clone(),
            midi_clock: if audition || !show.midi_clock {
                None
            } else {
                self.midi_clock.as_ref().map(|port| MidiClockOutput {
                    port_name: port.port_name.clone(),
                    port_index: port.port_index,
                    free_run: false,
                })
            },
        }
    }
}
