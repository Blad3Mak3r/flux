use super::*;

pub(super) fn file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("preferences.json"))
        .map_err(|e| e.to_string())
}

pub(super) fn load(path: &Path) -> Preferences {
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

pub(super) fn save(path: &Path, prefs: &Preferences) -> Result<(), String> {
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

pub(super) fn validate(p: &Preferences) -> Result<(), String> {
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

pub(super) fn main_screen() -> String {
    flux_project::mapping::MAIN_SCREEN.into()
}
