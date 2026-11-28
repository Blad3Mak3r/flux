//! Per-installation routing and language. Nothing here belongs to an undo history.
use flux_project::{
    AudioOutput, LtcOutput, MetronomeOutput, MidiClockOutput, MtcOutput, OutputConfig, ShowOutputs,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Mutex, PoisonError},
};
use tauri::{AppHandle, Emitter, Manager, State};

pub use flux_project::UNASSIGNED_DEVICE as UNASSIGNED;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub audio_routes: BTreeMap<String, AudioOutput>,
    pub locale: Option<Locale>,
    pub driver: Option<String>,
    pub audio: Option<AudioOutput>,
    pub preview: Option<AudioOutput>,
    pub operator: Option<AudioOutput>,
    pub voice: Option<String>,
    pub microphone: Option<Microphone>,
    /// The audio input and channel external LTC comes in on, for the VJ clock.
    pub ltc_in: Option<Microphone>,
    /// The MIDI input external MTC comes in on, for the VJ clock.
    pub mtc_in: Option<String>,
    /// What the VJ grid's audio modulation listens to; absent is the grid's own sound.
    pub analysis: Option<AnalysisSource>,
    pub metronome: Option<AudioOutput>,
    pub ltc: BTreeMap<u32, LtcOutput>,
    pub mtc: BTreeMap<u32, MtcOutput>,
    /// The MIDI port MIDI clock goes to, for a cue show and a VJ show alike; the show decides
    /// whether it is sent.
    pub midi_clock: Option<MtcOutput>,
    /// The display the main screen of the video output opened on before screens had ids of
    /// their own; read as the main screen's when `video_displays` does not name it.
    #[serde(default)]
    pub video_display: Option<String>,
    /// The display each screen of the video output opens on, by screen id and display name; a
    /// screen not listed stays closed.
    #[serde(default)]
    pub video_displays: BTreeMap<String, String>,
    /// The folder Output > Snapshot saves into; absent is `Pictures/Flux Snapshots`.
    pub snapshot_dir: Option<String>,
    /// A snapshot is also added to the project's media as an image clip.
    pub snapshot_media: bool,
    /// The folder the VJ recording saves into; absent is `Videos/FLUX`.
    pub recordings_dir: Option<String>,
    /// The keys the user changed in a cue show; see `flux_project::keymap`.
    pub shortcuts: flux_project::keymap::Overrides,
    /// The keys every action has now: what the window looks key presses up in. Worked out when sent.
    #[serde(skip_deserializing, skip_serializing_if = "Vec::is_empty")]
    pub keymap: Vec<flux_project::keymap::ActionView>,
    #[serde(skip_deserializing)]
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Microphone {
    pub driver: Option<String>,
    pub device: String,
    pub channel: u16,
}

/// What the audio analysis of the VJ grid listens to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AnalysisSource {
    /// Nothing: audio modulation stays still.
    Off,
    /// The sound of the grid's own clips.
    #[default]
    Grid,
    /// One channel of an audio input.
    Input {
        driver: Option<String>,
        device: String,
        channel: u16,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    Es,
}

#[derive(Default)]
pub struct Store(pub Mutex<Option<Preferences>>);

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
    /// Gives an action one key, or none. Refused when another action has the key.
    Shortcut {
        action: String,
        key: Option<String>,
    },
    /// Back to the default keys of an action, or of all.
    ShortcutReset {
        action: Option<String>,
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

fn file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("preferences.json"))
        .map_err(|e| e.to_string())
}

fn load(path: &Path) -> Preferences {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
            Ok(prefs) if validate(&prefs).is_ok() => prefs,
            _ => {
                log::warn!("invalid local preferences; using defaults");
                Preferences {
                    warning: Some("invalid local preferences; using defaults".into()),
                    ..Default::default()
                }
            }
        },
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("cannot read local preferences: {e}");
            }
            Preferences {
                warning: (e.kind() != std::io::ErrorKind::NotFound).then(|| e.to_string()),
                ..Default::default()
            }
        }
    }
}

fn save(path: &Path, prefs: &Preferences) -> Result<(), String> {
    let parent = path.parent().ok_or("missing preferences directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let mut stored = prefs.clone();
    stored.warning = None;
    stored.keymap.clear();
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn validate(p: &Preferences) -> Result<(), String> {
    if p.microphone.iter().chain(&p.ltc_in).any(|m| {
        m.device.len() > 1024
            || m.channel == 0
            || m.channel > 256
            || m.driver.as_ref().is_some_and(|d| d.len() > 128)
    }) || p.mtc_in.as_ref().is_some_and(|port| port.len() > 1024)
        || p.midi_clock
            .as_ref()
            .is_some_and(|port| port.port_name.len() > 1024)
    {
        return Err("invalid microphone input".into());
    }
    if let Some(AnalysisSource::Input {
        driver,
        device,
        channel,
    }) = &p.analysis
        && (device.len() > 1024
            || *channel == 0
            || *channel > 256
            || driver.as_ref().is_some_and(|d| d.len() > 128))
    {
        return Err("invalid analysis input".into());
    }
    flux_project::operator::validate_voice(p.voice.as_deref()).map_err(|e| e.to_string())?;
    flux_project::keymap::validate(&p.shortcuts).map_err(|e| e.to_string())?;
    if p.ltc.keys().chain(p.mtc.keys()).any(|s| *s == 0) {
        return Err("invalid output slot".into());
    }
    let output = OutputConfig {
        audio_routes: Vec::new(),
        track_routes: Vec::new(),
        driver: p.driver.clone(),
        audio: p.audio.clone(),
        preview: p.preview.clone(),
        operator: p.operator.clone(),
        metronome: p.metronome.as_ref().map(|m| MetronomeOutput {
            device_name: m.device_name.clone(),
            channels: m.channels.clone(),
            gain_db: 0.0,
        }),
        ltc: p.ltc.values().cloned().collect(),
        mtc: p.mtc.values().cloned().collect(),
        osc: vec![],
        dmx: vec![],
        midi_clock: None,
    };
    output.validate().map_err(|e| e.to_string())?;
    for (id, route) in &p.audio_routes {
        if !flux_project::routing::valid_id(id) {
            return Err("invalid audio output id".into());
        }
        let config = OutputConfig {
            audio: Some(route.clone()),
            ..Default::default()
        };
        if !(1..=2).contains(&route.channels.len()) {
            return Err("choose mono or stereo channels".into());
        }
        config.validate().map_err(|e| e.to_string())?;
    }
    if [&p.audio, &p.preview, &p.metronome, &p.operator]
        .into_iter()
        .flatten()
        .any(|a| a.channels.is_empty())
    {
        return Err("select at least one audio channel".into());
    }
    Ok(())
}

fn main_screen() -> String {
    flux_project::mapping::MAIN_SCREEN.into()
}

impl Preferences {
    /// What the window is sent: these preferences with the keys every action has now.
    fn sent(mut self) -> Self {
        self.keymap = flux_project::keymap::view(&self.shortcuts);
        self
    }

    /// The display the screen `screen` of the video output opens on.
    pub fn video_display_for(&self, screen: &str) -> Option<String> {
        self.video_displays.get(screen).cloned().or_else(|| {
            (screen == flux_project::mapping::MAIN_SCREEN)
                .then(|| self.video_display.clone())
                .flatten()
        })
    }

    fn apply(&mut self, change: Change) -> Result<(), String> {
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
            Change::Shortcut { action, key } => {
                flux_project::keymap::set(&mut self.shortcuts, &action, key.as_deref())
                    .map_err(|e| e.to_string())?;
            }
            Change::ShortcutReset { action } => {
                flux_project::keymap::reset(&mut self.shortcuts, action.as_deref())
                    .map_err(|e| e.to_string())?;
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

    fn edit_route(&mut self, kind: &str, field: &str, text: &str, slot: u32) -> Result<(), String> {
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
            dmx: show.dmx.clone(),
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

/// What the audio analysis listens to now, loading the preferences if they are not yet.
pub fn analysis_source(app: &AppHandle) -> AnalysisSource {
    let state = app.state::<Store>();
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .get_or_insert_with(|| {
            file(app)
                .map(|p| load(&p))
                .unwrap_or_else(|error| Preferences {
                    warning: Some(error),
                    ..Default::default()
                })
        })
        .analysis
        .clone()
        .unwrap_or_default()
}

#[tauri::command]
pub fn preferences_get(app: AppHandle, state: State<'_, Store>) -> Result<Preferences, String> {
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    Ok(guard
        .get_or_insert_with(|| {
            file(&app)
                .map(|p| load(&p))
                .unwrap_or_else(|error| Preferences {
                    warning: Some(error),
                    ..Default::default()
                })
        })
        .clone()
        .sent())
}

#[tauri::command]
pub fn preferences_change(
    app: AppHandle,
    state: State<'_, Store>,
    change: Change,
) -> Result<Preferences, String> {
    let path = file(&app)?;
    let mut guard = state.0.lock().unwrap_or_else(PoisonError::into_inner);
    let current = guard.get_or_insert_with(|| load(&path));
    let mut next = current.clone();
    next.apply(change)?;
    next.warning = None;
    save(&path, &next)?;
    next.revision = current.revision + 1;
    *current = next.clone();
    let next = next.sent();
    let _ = app.emit("preferences-changed", &next);
    Ok(next)
}

#[derive(Serialize)]
pub struct ResolvedOutputs {
    choices: Vec<flux_project::routing::AudioChoice>,
    live: OutputConfig,
    edit: OutputConfig,
    audition: bool,
}

#[tauri::command]
pub fn preferences_resolve(
    app: AppHandle,
    state: State<'_, Store>,
    project: flux_project::Project,
    session: State<'_, crate::commands::SharedSession>,
) -> Result<ResolvedOutputs, String> {
    project.validate().map_err(|e| e.to_string())?;
    let timeline = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .active_timeline(&project)
        .map_err(|e| e.to_string())?
        .clone();
    let prefs = preferences_get(app, state)?;
    let audition = prefs.preview.is_some();
    Ok(ResolvedOutputs {
        choices: flux_project::routing::choices(&project),
        live: prefs.resolve_project(&project, &timeline, false),
        edit: prefs.resolve_project(&project, &timeline, audition),
        audition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_destinations_resolve_locally_and_preview_preserves_silent_tracks() {
        use flux_project::{
            Track, TrackKind,
            routing::{AudioAssignment, AudioBus},
        };
        let mut project = flux_project::Project::new("Show", flux_project::FrameRate::Fps25);
        let id = "5fda6c22-87d3-40e9-9710-589f8c1ef8f0".to_owned();
        project.output.audio.push(AudioBus {
            id: id.clone(),
            name: "Monitor".into(),
        });
        for (name, output) in [
            ("Inherit", AudioAssignment::Inherit),
            ("Custom", AudioAssignment::Output { id: id.clone() }),
            ("Silent", AudioAssignment::Silent),
        ] {
            project.timelines[0].tracks.push(Track {
                id: name.into(),
                name: name.into(),
                muted: false,
                color: None,
                gain_db: None,
                audio_output: output,
                kind: TrackKind::Operator {
                    voice: None,
                    orders: vec![],
                },
            });
        }
        let route = |name: &str| AudioOutput {
            device_name: name.into(),
            channels: vec![1, 2],
        };
        let mut prefs = Preferences {
            operator: Some(route("Headphones")),
            preview: Some(route("Preview")),
            ..Default::default()
        };
        let live = prefs.resolve_project(&project, &project.timelines[0], false);
        assert!(
            live.audio_routes
                .iter()
                .find(|b| b.id == id)
                .unwrap()
                .route
                .is_none()
        );
        prefs
            .audio_routes
            .insert(id.clone(), route("Monitor device"));
        let live = prefs.resolve_project(&project, &project.timelines[0], false);
        assert_eq!(
            live.audio_routes[2].route.as_ref().unwrap().device_name,
            "Monitor device"
        );
        let edit = prefs.resolve_project(&project, &project.timelines[0], true);
        assert!(
            edit.audio_routes
                .iter()
                .all(|b| b.route.as_ref().unwrap().device_name == "Preview")
        );
        assert_eq!(
            edit.track_routes
                .iter()
                .find(|r| r.track_id == "Silent")
                .unwrap()
                .output,
            None
        );
        assert_eq!(
            edit.track_routes
                .iter()
                .find(|r| r.track_id == "Inherit")
                .unwrap()
                .output
                .as_deref(),
            Some("operator")
        );
        let json = serde_json::to_string(&project).unwrap();
        assert!(!json.contains("Monitor device") && !json.contains("Headphones"));
    }
    #[test]
    fn named_hardware_routes_survive_preferences_roundtrip_and_driver_switch_clears_them() {
        let id = "5fda6c22-87d3-40e9-9710-589f8c1ef8f0".to_owned();
        let mut prefs = Preferences::default();
        prefs.audio_routes.insert(
            id.clone(),
            AudioOutput {
                device_name: "Monitor".into(),
                channels: vec![5],
            },
        );
        prefs
            .apply(Change::AudioRoute {
                id: id.clone(),
                field: "channels".into(),
                text: "7".into(),
            })
            .unwrap();
        let loaded: Preferences =
            serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
        assert_eq!(loaded.audio_routes[&id].channels, vec![7]);
        prefs
            .apply(Change::Route {
                kind: "driver".into(),
                field: "driver".into(),
                text: "Other".into(),
                slot: 0,
            })
            .unwrap();
        assert!(prefs.audio_routes.is_empty());
    }
    #[test]
    fn locale_changes_do_not_count_as_hardware_changes() {
        let before = Preferences::default();
        let mut after = before.clone();
        after.locale = Some(Locale::Es);
        assert!(before.same_routing(&after));
        after.audio = Some(AudioOutput {
            device_name: "Main".into(),
            channels: vec![1, 2],
        });
        assert!(!before.same_routing(&after));
    }

    #[test]
    fn saving_replaces_previous_preferences_and_failure_keeps_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let mut p = Preferences {
            locale: Some(Locale::Es),
            ..Default::default()
        };
        save(&path, &p).unwrap();
        p.locale = Some(Locale::En);
        save(&path, &p).unwrap();
        assert!(matches!(load(&path).locale, Some(Locale::En)));
        let before = fs::read(&path).unwrap();
        fs::create_dir(path.with_extension("json.tmp")).unwrap();
        assert!(save(&path, &p).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn corrupt_preferences_have_a_visible_warning_and_safe_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        fs::write(&path, "broken json").unwrap();
        let p = load(&path);
        assert!(p.warning.is_some());
        assert!(p.ltc.is_empty() && p.mtc.is_empty());
    }

    #[test]
    fn two_shows_use_shared_routes_and_independent_levels() {
        let mut p = Preferences::default();
        p.ltc.insert(
            2,
            LtcOutput {
                device_name: "Sync".into(),
                channel: 8,
                level: None,
                offset_ms: Some(15),
            },
        );
        let show = |level| ShowOutputs {
            audio: Vec::new(),
            ltc: vec![flux_project::LtcSignal {
                slot: 2,
                level: Some(level),
            }],
            ..Default::default()
        };
        let first = p.resolve(&show(0.2), false);
        let second = p.resolve(&show(0.7), false);
        assert_eq!(first.ltc[0].device_name, second.ltc[0].device_name);
        assert_eq!(
            (first.ltc[0].level, second.ltc[0].level),
            (Some(0.2), Some(0.7))
        );
        assert_eq!(p.ltc[&2].offset_ms, Some(15));
    }

    #[test]
    fn import_preserves_unmentioned_routes_and_language() {
        let mut p = Preferences {
            locale: Some(Locale::Es),
            ..Default::default()
        };
        p.mtc.insert(
            2,
            MtcOutput {
                port_name: "Keep".into(),
                port_index: 1,
                offset_ms: Some(-10),
            },
        );
        let imported = OutputConfig {
            audio_routes: Vec::new(),
            track_routes: Vec::new(),
            mtc: vec![MtcOutput {
                port_name: "New".into(),
                port_index: 2,
                offset_ms: Some(12),
            }],
            ..Default::default()
        };
        p.apply(Change::Import {
            routes: Box::new(imported),
        })
        .unwrap();
        assert_eq!(p.mtc[&1].port_index, 2);
        assert_eq!(p.mtc[&2].port_name, "Keep");
        assert!(matches!(p.locale, Some(Locale::Es)));
    }

    #[test]
    fn route_edits_parse_and_validate_channel_and_offset_text() {
        let mut p = Preferences::default();
        p.mtc.insert(
            1,
            MtcOutput {
                port_name: "Bus".into(),
                port_index: 1,
                offset_ms: None,
            },
        );
        for text in ["abc", "1.5", "1001", "-1001", ""] {
            assert!(
                p.apply(Change::Offset {
                    kind: "mtc".into(),
                    slot: 1,
                    text: text.into()
                })
                .is_err()
            );
        }
        p.apply(Change::Offset {
            kind: "mtc".into(),
            slot: 1,
            text: " -12 ".into(),
        })
        .unwrap();
        assert_eq!(p.mtc[&1].offset_ms, Some(-12));
        p.apply(Change::Route {
            kind: "mtc".into(),
            field: "port".into(),
            text: "2|Other".into(),
            slot: 1,
        })
        .unwrap();
        assert_eq!((p.mtc[&1].port_index, p.mtc[&1].offset_ms), (2, Some(-12)));
        p.audio = Some(AudioOutput {
            device_name: "Main".into(),
            channels: vec![1, 2],
        });
        assert!(p.edit_route("audio", "channels", "1-1", 0).is_err());
    }

    #[test]
    fn midi_clock_should_go_out_only_when_the_show_and_this_computer_both_ask() {
        let mut p = Preferences::default();
        let mut show = ShowOutputs::default();
        assert!(p.resolve(&show, false).midi_clock.is_none());
        p.apply(Change::MidiClock {
            port: Some("Drum machine".into()),
            index: 1,
        })
        .unwrap();
        assert!(
            p.resolve(&show, false).midi_clock.is_none(),
            "the show is off"
        );
        show.midi_clock = true;
        let clock = p.resolve(&show, false).midi_clock.unwrap();
        assert_eq!(
            (clock.port_name.as_str(), clock.port_index),
            ("Drum machine", 1)
        );
        assert!(!clock.free_run, "a cue show unless the project says VJ");
        assert!(
            p.resolve(&show, true).midi_clock.is_none(),
            "no clock while auditioning"
        );
        p.apply(Change::MidiClock {
            port: None,
            index: 0,
        })
        .unwrap();
        assert!(p.resolve(&show, false).midi_clock.is_none());
    }

    #[test]
    fn a_vj_show_should_run_its_midi_clock_free() {
        let mut p = Preferences::default();
        p.apply(Change::MidiClock {
            port: Some("Port".into()),
            index: 0,
        })
        .unwrap();
        let mut project = flux_project::Project::new("Show", flux_project::FrameRate::Fps25);
        project.output.midi_clock = true;
        let timeline = project.timelines[0].clone();
        let cue = p.resolve_project(&project, &timeline, false);
        assert!(!cue.midi_clock.unwrap().free_run);
        project.mode = flux_project::ProjectMode::Vj;
        let vj = p.resolve_project(&project, &timeline, false);
        assert!(vj.midi_clock.unwrap().free_run);
    }

    #[test]
    fn local_routes_resolve_without_importing_legacy_hints() {
        let legacy = OutputConfig {
            audio_routes: Vec::new(),
            track_routes: Vec::new(),
            ltc: vec![LtcOutput {
                device_name: "Old".into(),
                channel: 3,
                level: Some(0.2),
                offset_ms: Some(20),
            }],
            ..Default::default()
        };
        let show = ShowOutputs::from(legacy.clone());
        let mut p = Preferences::default();
        assert_eq!(p.resolve(&show, false).ltc[0].device_name, UNASSIGNED);
        p.apply(Change::Import {
            routes: Box::new(legacy),
        })
        .unwrap();
        let resolved = p.resolve(&show, false);
        assert_eq!(
            (
                resolved.ltc[0].channel,
                resolved.ltc[0].level,
                resolved.ltc[0].offset_ms
            ),
            (3, Some(0.2), Some(20))
        );
        assert!(p.resolve(&show, true).ltc.is_empty());
    }
    #[test]
    fn offsets_and_locale_migration_are_checked() {
        let mut p = Preferences::default();
        p.apply(Change::Locale {
            locale: Locale::Es,
            only_if_unset: true,
        })
        .unwrap();
        p.apply(Change::Locale {
            locale: Locale::En,
            only_if_unset: true,
        })
        .unwrap();
        assert!(matches!(p.locale, Some(Locale::Es)));
        assert!(
            p.apply(Change::Offset {
                kind: "ltc".into(),
                slot: 1,
                text: "1001".into()
            })
            .is_err()
        );
    }

    #[test]
    fn the_analysis_source_should_default_to_the_grid_and_round_trip() {
        assert_eq!(AnalysisSource::default(), AnalysisSource::Grid);
        assert!(Preferences::default().analysis.is_none());
        let input = AnalysisSource::Input {
            driver: None,
            device: "Interface".into(),
            channel: 2,
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["kind"], "input");
        assert_eq!(
            serde_json::from_value::<AnalysisSource>(json).unwrap(),
            input
        );
        let change: Change =
            serde_json::from_str(r#"{"op":"analysis","source":{"kind":"off"}}"#).unwrap();
        let mut prefs = Preferences::default();
        prefs.apply(change).unwrap();
        assert_eq!(prefs.analysis, Some(AnalysisSource::Off));
        prefs.analysis = Some(AnalysisSource::Input {
            driver: None,
            device: "x".into(),
            channel: 0,
        });
        assert!(validate(&prefs).is_err());
    }

    #[test]
    fn shortcuts_round_trip_and_refuse_a_key_in_use() {
        let mut prefs = Preferences::default();
        let change = |json: &str| serde_json::from_str::<Change>(json).unwrap();
        prefs
            .apply(change(
                r#"{"op":"shortcut","action":"addMarker","key":"k"}"#,
            ))
            .unwrap();
        let error = prefs
            .apply(change(
                r#"{"op":"shortcut","action":"addRegion","key":"K"}"#,
            ))
            .unwrap_err();
        assert_eq!(error, "conflict:addMarker");
        let dir = std::env::temp_dir().join(format!("flux-keys-{}", std::process::id()));
        let path = dir.join("preferences.json");
        save(&path, &prefs.clone().sent()).unwrap();
        let loaded = load(&path);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(loaded.warning, None);
        assert_eq!(loaded.shortcuts, prefs.shortcuts);
        let keys = |p: Preferences| {
            p.sent()
                .keymap
                .into_iter()
                .find(|a| a.id == "addMarker")
                .unwrap()
                .keys
        };
        assert_eq!(keys(loaded), ["K"]);
        prefs
            .apply(change(r#"{"op":"shortcutReset","action":null}"#))
            .unwrap();
        assert_eq!(keys(prefs), ["M"]);
    }

    #[test]
    fn every_action_the_window_uses_is_in_the_table() {
        let ts = include_str!("../../src/keymap/actions.ts");
        let used: std::collections::BTreeSet<&str> = ts.split('"').skip(1).step_by(2).collect();
        let table: std::collections::BTreeSet<&str> =
            flux_project::keymap::ACTIONS.iter().map(|a| a.id).collect();
        assert_eq!(used, table);
    }
}
