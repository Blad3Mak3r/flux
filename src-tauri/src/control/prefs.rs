use super::*;

/// Whether an action makes sense: a number is a place from 1 and a step moves.
fn valid(action: ControlAction) -> bool {
    match action {
        ControlAction::GotoCue(n) | ControlAction::SelectTimeline(n) => n >= 1,
        ControlAction::StepFrames(n) | ControlAction::StepSeconds(n) => n != 0,
        _ => true,
    }
}

fn valid_trigger(trigger: MidiTrigger) -> bool {
    (1..=16).contains(&trigger.channel) && trigger.number < 128
}

/// `prefs` as they can be kept: a file edited by hand or by another version may hold anything.
pub fn normalize(mut prefs: ControlPrefs) -> ControlPrefs {
    prefs
        .midi
        .bindings
        .retain(|binding| valid(binding.action) && valid_trigger(binding.trigger));
    for binding in &mut prefs.midi.bindings {
        binding.feedback = binding.feedback.filter(|feedback| feedback.is_valid());
    }
    prefs.midi.bindings = dedup(prefs.midi.bindings);
    prefs.midi.port_name = prefs.midi.port_name.filter(|name| !name.trim().is_empty());
    if prefs.osc.port == 0 {
        prefs.osc.port = DEFAULT_OSC_PORT;
    }
    prefs.artnet.universe = prefs
        .artnet
        .universe
        .filter(|universe| *universe <= flux_project::vj_mapping::MAX_UNIVERSE);
    prefs.artnet.bind_address = prefs.artnet.bind_address.trim().to_owned();
    if prefs.artnet.bind_address.is_empty() {
        prefs.artnet.bind_address = ArtnetPrefs::default().bind_address;
    }
    prefs
}

pub(super) fn prefs_file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(IDENTIFIER).join(FILE_NAME))
}

/// The preferences in `file`, or the defaults when there are none or they cannot be read.
pub(super) fn load(file: &Path) -> ControlPrefs {
    let prefs = fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    normalize(prefs)
}

/// Writes through a temporary file, so a failed write leaves the old preferences as they were.
pub(super) fn save(file: &Path, prefs: &ControlPrefs) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let tmp = file.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(prefs).map_err(io::Error::other)?;
    fs::write(&tmp, json)?;
    fs::rename(&tmp, file).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}
