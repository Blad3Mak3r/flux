//! Per-installation routing and language. Nothing here belongs to an undo history.
use flux_project::{AudioOutput, LtcOutput, MetronomeOutput, MtcOutput, OutputConfig, ShowOutputs};
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
    pub locale: Option<Locale>,
    pub driver: Option<String>,
    pub audio: Option<AudioOutput>,
    pub preview: Option<AudioOutput>,
    pub operator: Option<AudioOutput>,
    pub voice: Option<String>,
    pub metronome: Option<AudioOutput>,
    pub ltc: BTreeMap<u32, LtcOutput>,
    pub mtc: BTreeMap<u32, MtcOutput>,
    #[serde(skip_deserializing)]
    pub revision: u64,
    #[serde(skip_deserializing)]
    pub warning: Option<String>,
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
    Voice {
        voice: Option<String>,
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
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string())?;
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn validate(p: &Preferences) -> Result<(), String> {
    flux_project::operator::validate_voice(p.voice.as_deref()).map_err(|e| e.to_string())?;
    if p.ltc.keys().chain(p.mtc.keys()).any(|s| *s == 0) {
        return Err("invalid output slot".into());
    }
    let output = OutputConfig {
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
    };
    output.validate().map_err(|e| e.to_string())?;
    if [&p.audio, &p.preview, &p.metronome, &p.operator]
        .into_iter()
        .flatten()
        .any(|a| a.channels.is_empty())
    {
        return Err("select at least one audio channel".into());
    }
    Ok(())
}

impl Preferences {
    fn apply(&mut self, change: Change) -> Result<(), String> {
        match change {
            Change::Voice { voice } => self.voice = voice,
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

    fn edit_route(&mut self, kind: &str, field: &str, text: &str, slot: u32) -> Result<(), String> {
        if kind == "driver" {
            self.driver = (!text.is_empty()).then(|| text.to_owned());
            // A host switch must not silently reroute timecode to its default device.
            self.audio = None;
            self.preview = None;
            self.operator = None;
            self.metronome = None;
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
        self.driver == other.driver
            && self.audio == other.audio
            && self.operator == other.operator
            && self.preview == other.preview
            && self.metronome == other.metronome
            && self.ltc == other.ltc
            && self.mtc == other.mtc
    }

    pub fn resolve(&self, show: &ShowOutputs, audition: bool) -> OutputConfig {
        OutputConfig {
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
        }
    }
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
        .clone())
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
    let _ = app.emit("preferences-changed", &next);
    Ok(next)
}

#[derive(Serialize)]
pub struct ResolvedOutputs {
    live: OutputConfig,
    edit: OutputConfig,
    audition: bool,
}

#[tauri::command]
pub fn preferences_resolve(
    app: AppHandle,
    state: State<'_, Store>,
    show: ShowOutputs,
) -> Result<ResolvedOutputs, String> {
    show.validate().map_err(|e| e.to_string())?;
    let prefs = preferences_get(app, state)?;
    let audition = prefs.preview.is_some();
    Ok(ResolvedOutputs {
        live: prefs.resolve(&show, false),
        edit: prefs.resolve(&show, audition),
        audition,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn local_routes_resolve_without_importing_legacy_hints() {
        let legacy = OutputConfig {
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
}
