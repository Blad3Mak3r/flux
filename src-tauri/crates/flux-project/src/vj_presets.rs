//! Presets of the VJ grid, kept per user like Resolume keeps them in the user's Documents folder:
//! **effect presets** (the settings of one effect, or of a whole stack, under a name) and **show
//! templates** (the structure of a VJ show without its clips).
//!
//! Both live outside any show, in the app data folder, as versioned JSON; a preset is copied into
//! the show when it is used and the show never refers back to it, so a show stays portable. The
//! files are read and written here; the app crate only gives them a folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::Project;
use crate::vj::ProjectMode;
use crate::vj::{Effect, EffectKind, EffectTarget, MAX_EFFECTS, Modulation, Vj};
use crate::vj_file::VjFile;
use crate::{ProjectError, Result};

/// The version of the preset library and of a template file this build writes; a file with a
/// higher one was made by a newer Flux and is refused, never read half.
pub const VERSION: u32 = 1;
/// The most presets the library holds.
pub const MAX_PRESETS: usize = 500;
/// The most templates the folder holds.
pub const MAX_TEMPLATES: usize = 100;
/// The longest name of a preset or a template, in characters.
pub const MAX_NAME: usize = 64;

fn name_of(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name is 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One effect of a preset: its kind and settings as the map the kind is stored as, so a preset
/// written when a kind had fewer settings still applies (the settings it lacks keep their value).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetEffect {
    /// The settings, with `effect` naming the kind.
    pub settings: Map<String, Value>,
    /// What drives a numeric setting by itself, by the setting's key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modulation: BTreeMap<String, Modulation>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
}

impl PresetEffect {
    fn of(effect: &Effect) -> Self {
        Self {
            settings: effect.kind.settings(),
            modulation: effect.modulation.clone(),
            bypass: effect.bypass,
        }
    }

    /// The kind's name, empty if the preset lacks it.
    pub fn name(&self) -> &str {
        self.settings
            .get("effect")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// A new effect with the preset's settings over the ones the kind starts with.
    fn effect(&self, id: String) -> Result<Effect> {
        let kind = EffectKind::default_of(self.name())?.with_settings(&self.settings)?;
        Ok(Effect {
            id,
            bypass: self.bypass,
            modulation: self.modulation_for(&kind),
            kind,
        })
    }

    /// The modulation the kind can take: a setting that is no longer modulatable is left out.
    fn modulation_for(&self, kind: &EffectKind) -> BTreeMap<String, Modulation> {
        self.modulation
            .iter()
            .filter(|(key, _)| kind.modulatable_range(key).is_ok())
            .map(|(key, modulation)| (key.clone(), *modulation))
            .collect()
    }
}

/// A named set of effect settings: one effect, or a whole stack when it holds several.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectPreset {
    pub id: String,
    pub name: String,
    /// At least one, at most [`MAX_EFFECTS`].
    pub effects: Vec<PresetEffect>,
}

impl EffectPreset {
    /// The preset of one effect, or of the whole stack of a clip, a layer, a group or the
    /// composition.
    pub fn of_effect(vj: &Vj, effect: &str, id: String, name: &str) -> Result<Self> {
        let found = stacks(vj)
            .flatten()
            .find(|e| e.id == effect)
            .ok_or_else(|| ProjectError::Invalid(format!("no effect {effect}")))?;
        Self::new(id, name, vec![PresetEffect::of(found)])
    }

    pub fn of_stack(vj: &Vj, target: &EffectTarget, id: String, name: &str) -> Result<Self> {
        let effects = stack(vj, target)?.iter().map(PresetEffect::of).collect();
        Self::new(id, name, effects)
    }

    fn new(id: String, name: &str, effects: Vec<PresetEffect>) -> Result<Self> {
        if effects.is_empty() || effects.len() > MAX_EFFECTS {
            return Err(ProjectError::Invalid(format!(
                "a preset holds 1 to {MAX_EFFECTS} effects"
            )));
        }
        Ok(Self {
            id,
            name: name_of(name)?,
            effects,
        })
    }

    /// The kind of its first effect when it is the preset of one effect.
    pub fn single(&self) -> Option<&str> {
        (self.effects.len() == 1).then(|| self.effects[0].name())
    }
}

fn stack<'a>(vj: &'a Vj, target: &EffectTarget) -> Result<&'a Vec<Effect>> {
    let missing = |what: &str, id: &str| ProjectError::Invalid(format!("no {what} {id}"));
    Ok(match target {
        EffectTarget::Composition => &vj.composition_effects,
        EffectTarget::Layer(id) => {
            &vj.layers
                .iter()
                .find(|l| l.id == *id)
                .ok_or_else(|| missing("layer", id))?
                .effects
        }
        EffectTarget::Group(id) => {
            &vj.groups
                .iter()
                .find(|g| g.id == *id)
                .ok_or_else(|| missing("group", id))?
                .effects
        }
        EffectTarget::Clip(id) => {
            &vj.layers
                .iter()
                .flat_map(|l| &l.clips)
                .find(|c| c.id == *id)
                .ok_or_else(|| missing("clip", id))?
                .effects
        }
    })
}

fn stacks(vj: &Vj) -> impl Iterator<Item = &Vec<Effect>> {
    std::iter::once(&vj.composition_effects)
        .chain(vj.groups.iter().map(|g| &g.effects))
        .chain(vj.layers.iter().flat_map(|layer| {
            std::iter::once(&layer.effects).chain(layer.clips.iter().map(|c| &c.effects))
        }))
}

/// Gives `effect` the settings of the one-effect `preset`: the settings the preset has, in the
/// range of the kind; the ones it lacks keep their value. The effect and the preset must be the
/// same kind, and the effect keeps its place, id and bypass.
pub(crate) fn apply_to_effect(vj: &mut Vj, effect: &str, preset: &EffectPreset) -> Result<()> {
    let [source] = preset.effects.as_slice() else {
        return Err(ProjectError::Invalid(
            "a stack preset is added to a stack, not applied to an effect".into(),
        ));
    };
    let (stack, at) = vj.effect_mut(effect)?;
    let target = &mut stack[at];
    if target.kind.name() != source.name() {
        return Err(ProjectError::Invalid(format!(
            "the preset {} is for {}, not {}",
            preset.name,
            source.name(),
            target.kind.name()
        )));
    }
    target.kind = target.kind.with_settings(&source.settings)?;
    target.modulation = source.modulation_for(&target.kind);
    Ok(())
}

/// Puts the effects of `preset` at the end of the stack of `target`, with new ids.
pub(crate) fn add_to_stack(
    vj: &mut Vj,
    target: &EffectTarget,
    preset: &EffectPreset,
    mut new_id: impl FnMut() -> String,
) -> Result<()> {
    let effects = preset
        .effects
        .iter()
        .map(|e| e.effect(new_id()))
        .collect::<Result<Vec<_>>>()?;
    if effects
        .iter()
        .any(|e| e.kind.composition_only() && *target != EffectTarget::Composition)
    {
        return Err(ProjectError::Invalid(
            "an effect of the preset only works on the composition".into(),
        ));
    }
    let stack = match target {
        EffectTarget::Clip(id) => &mut vj.clip_mut(id)?.effects,
        EffectTarget::Layer(id) => &mut vj.layer_mut(id)?.effects,
        EffectTarget::Group(id) => &mut vj.group_mut(id)?.effects,
        EffectTarget::Composition => &mut vj.composition_effects,
    };
    if stack.len() + effects.len() > MAX_EFFECTS {
        return Err(ProjectError::Invalid(format!(
            "a stack has at most {MAX_EFFECTS} effects"
        )));
    }
    stack.extend(effects);
    Ok(())
}

/// The effect presets of the user: the file `vj-presets.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub version: u32,
    pub presets: Vec<EffectPreset>,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: VERSION,
            presets: Vec::new(),
        }
    }
}

impl Library {
    /// The library in `text`; refused when a newer Flux wrote it.
    pub fn parse(text: &str) -> Result<Self> {
        let library: Self = serde_json::from_str(text).map_err(|err| {
            ProjectError::Invalid(format!("the preset library is not valid: {err}"))
        })?;
        check_version(library.version, "preset library")?;
        Ok(library)
    }

    /// The library at `path`; none yet is an empty one, and one that cannot be read is an error
    /// (it is never overwritten by an empty one).
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        write_atomic(path, &serde_json::to_vec_pretty(self).map_err(invalid)?)
    }

    pub fn add(&mut self, preset: EffectPreset) -> Result<()> {
        if self.presets.len() >= MAX_PRESETS {
            return Err(ProjectError::Invalid(format!(
                "the library holds at most {MAX_PRESETS} presets"
            )));
        }
        self.presets.push(preset);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<&EffectPreset> {
        self.presets
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no preset {id}")))
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<()> {
        let name = name_of(name)?;
        let preset = self
            .presets
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no preset {id}")))?;
        preset.name = name;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let before = self.presets.len();
        self.presets.retain(|p| p.id != id);
        if self.presets.len() == before {
            return Err(ProjectError::Invalid(format!("no preset {id}")));
        }
        Ok(())
    }
}

fn invalid(err: serde_json::Error) -> ProjectError {
    ProjectError::Invalid(err.to_string())
}

fn check_version(version: u32, what: &str) -> Result<()> {
    if version == 0 || version > VERSION {
        return Err(ProjectError::Invalid(format!(
            "this {what} is version {version}; this Flux reads up to {VERSION}"
        )));
    }
    Ok(())
}

/// Written to a temporary file and renamed, so a crash cannot leave half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// The structure of a VJ show without its clips: its layers (with their blends, transitions and
/// effects), groups, decks and columns, composition effects, mappings, dashboard, lights, clock
/// and output settings. The media files and the clips that used them are left out, and so are the
/// mappings and dashboard links that pointed at a clip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub version: u32,
    pub name: String,
    show: VjFile,
}

/// What a template is listed as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    /// The file name without its extension.
    pub id: String,
    pub name: String,
    pub layers: usize,
    pub decks: usize,
    pub columns: usize,
}

impl Template {
    /// The template of the VJ show `project`, called `name`.
    pub fn of(project: &Project, name: &str) -> Result<Self> {
        if project.mode != ProjectMode::Vj {
            return Err(ProjectError::Invalid(
                "only a VJ show can be a template".into(),
            ));
        }
        let mut stripped = project.clone();
        stripped.videos.clear();
        for layer in &mut stripped.vj.layers {
            layer.clips.clear();
        }
        crate::vj_mapping::prune(&mut stripped.vj);
        stripped.validate()?;
        Ok(Self {
            version: VERSION,
            name: name_of(name)?,
            show: VjFile::of(&stripped),
        })
    }

    /// A new, unsaved show with the template's structure, named `name`. The ids are the
    /// template's: the show is new, so they cannot clash with another show's.
    pub fn project(&self, name: &str) -> Result<Project> {
        let mut project = self.show.clone().into_project();
        project.name = name_of(name)?;
        project.meta = crate::meta::Meta::new();
        project.validate()?;
        Ok(project)
    }

    pub fn parse(text: &str) -> Result<Self> {
        // The version is read first, so a newer file whose shape changed says so.
        let value: Value = serde_json::from_str(text)
            .map_err(|err| ProjectError::Invalid(format!("the template is not valid: {err}")))?;
        check_version(
            value
                .get("version")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0),
            "template",
        )?;
        serde_json::from_value(value)
            .map_err(|err| ProjectError::Invalid(format!("the template is not valid: {err}")))
    }

    fn info(&self, id: String) -> TemplateInfo {
        let vj = &self.show.grid;
        TemplateInfo {
            id,
            name: self.name.clone(),
            layers: vj.layers.len(),
            decks: vj.decks.len(),
            columns: vj.decks.iter().map(|d| d.columns.len()).sum(),
        }
    }
}

const EXTENSION: &str = "json";

fn template_path(dir: &Path, id: &str) -> Result<PathBuf> {
    // An id is a file name the folder made: nothing that walks out of it.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ProjectError::Invalid(format!("no template {id}")));
    }
    Ok(dir.join(id).with_extension(EXTENSION))
}

/// The templates in `dir`, by name; a file that is not a readable template is skipped.
pub fn list_templates(dir: &Path) -> Vec<TemplateInfo> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != EXTENSION {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_owned();
            let template = Template::parse(&fs::read_to_string(&path).ok()?).ok()?;
            Some(template.info(id))
        })
        .collect();
    found.sort_by_key(|t| t.name.to_lowercase());
    found
}

pub fn load_template(dir: &Path, id: &str) -> Result<Template> {
    Template::parse(&fs::read_to_string(template_path(dir, id)?)?)
}

/// Stores `template` as a new file of `dir`; `id` is a fresh unique file name.
pub fn save_template(dir: &Path, template: &Template, id: String) -> Result<TemplateInfo> {
    if list_templates(dir).len() >= MAX_TEMPLATES {
        return Err(ProjectError::Invalid(format!(
            "there can be at most {MAX_TEMPLATES} templates"
        )));
    }
    let bytes = serde_json::to_vec_pretty(template).map_err(invalid)?;
    write_atomic(&template_path(dir, &id)?, &bytes)?;
    Ok(template.info(id))
}

pub fn rename_template(dir: &Path, id: &str, name: &str) -> Result<()> {
    let mut template = load_template(dir, id)?;
    template.name = name_of(name)?;
    let bytes = serde_json::to_vec_pretty(&template).map_err(invalid)?;
    write_atomic(&template_path(dir, id)?, &bytes)
}

pub fn delete_template(dir: &Path, id: &str) -> Result<()> {
    fs::remove_file(template_path(dir, id)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj::VjOp;

    fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("id{n}")
        }
    }

    fn show() -> Project {
        let mut project = Project::new("Gira", crate::FrameRate::Fps30);
        project.mode = ProjectMode::Vj;
        project.vj = Vj::starting(counter());
        project
    }

    fn run(project: &mut Project, op: VjOp) {
        crate::vj::apply(project, op, counter_from(100)).unwrap();
    }

    /// Ids no other call of the tests gets.
    fn counter_from(_: u32) -> impl FnMut() -> String {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);
        || {
            format!(
                "n{}",
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )
        }
    }

    fn add_effect(project: &mut Project, target: EffectTarget, kind: &str) -> String {
        run(
            project,
            VjOp::AddEffect {
                target: target.clone(),
                kind: kind.into(),
            },
        );
        stack(&project.vj, &target)
            .unwrap()
            .last()
            .unwrap()
            .id
            .clone()
    }

    fn tint(project: &mut Project, effect: &str, colour: &str, amount: f32) {
        let kind: EffectKind = serde_json::from_value(
            serde_json::json!({"effect": "tint", "colour": colour, "amount": amount}),
        )
        .unwrap();
        run(
            project,
            VjOp::EffectParams {
                effect: effect.into(),
                kind,
            },
        );
    }

    fn settings(project: &Project, effect: &str) -> Map<String, Value> {
        stacks(&project.vj)
            .flatten()
            .find(|e| e.id == effect)
            .unwrap()
            .kind
            .settings()
    }

    #[test]
    fn a_preset_should_round_trip_through_the_library_file() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let tinted = add_effect(&mut project, EffectTarget::Layer(layer.clone()), "tint");
        tint(&mut project, &tinted, "#112233", 0.8);
        let preset = EffectPreset::of_effect(&project.vj, &tinted, "p1".into(), " Cold ").unwrap();
        assert_eq!(
            (preset.name.as_str(), preset.single()),
            ("Cold", Some("tint"))
        );
        let mut library = Library::default();
        library.add(preset.clone()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/vj-presets.json");
        library.save(&file).unwrap();
        assert_eq!(Library::load(&file).unwrap(), library);
        library.rename("p1", "Icy").unwrap();
        assert_eq!(library.get("p1").unwrap().name, "Icy");
        assert!(library.rename("p1", "  ").is_err());
        library.remove("p1").unwrap();
        assert!(library.remove("p1").is_err());
        // No file yet is an empty library.
        assert!(
            Library::load(&dir.path().join("none.json"))
                .unwrap()
                .presets
                .is_empty()
        );
    }

    #[test]
    fn a_library_of_a_newer_version_should_be_refused() {
        let text = format!(r#"{{"version": {}, "presets": []}}"#, VERSION + 1);
        assert!(Library::parse(&text).is_err());
        assert!(Library::parse(r#"{"version": 0, "presets": []}"#).is_err());
        assert!(Library::parse("nonsense").is_err());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vj-presets.json");
        fs::write(&file, text).unwrap();
        assert!(Library::load(&file).is_err());
    }

    #[test]
    fn applying_a_preset_should_set_its_settings_and_keep_the_effects_place() {
        let mut project = show();
        let (a, b) = (
            project.vj.layers[0].id.clone(),
            add_clip_layer(&mut project),
        );
        let from = add_effect(&mut project, EffectTarget::Layer(a), "tint");
        tint(&mut project, &from, "#ff0000", 0.9);
        let to = add_effect(&mut project, EffectTarget::Layer(b), "tint");
        let preset = EffectPreset::of_effect(&project.vj, &from, "p".into(), "Red").unwrap();
        crate::vj::apply(
            &mut project,
            VjOp::ApplyEffectPreset {
                effect: to.clone(),
                preset: preset.clone(),
            },
            counter(),
        )
        .unwrap();
        assert_eq!(settings(&project, &to), settings(&project, &from));
        // A kind other than the preset's is refused.
        let blur = add_effect(&mut project, EffectTarget::Composition, "brightness");
        let wrong = VjOp::ApplyEffectPreset {
            effect: blur,
            preset,
        };
        assert!(crate::vj::apply(&mut project, wrong, counter()).is_err());
    }

    fn add_clip_layer(project: &mut Project) -> String {
        run(project, VjOp::AddLayer { global: false });
        project.vj.layers.last().unwrap().id.clone()
    }

    #[test]
    fn the_settings_a_preset_lacks_should_keep_their_value() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let effect = add_effect(&mut project, EffectTarget::Layer(layer), "tint");
        tint(&mut project, &effect, "#00ff00", 0.25);
        let mut preset = EffectPreset::of_effect(&project.vj, &effect, "p".into(), "Old").unwrap();
        preset.effects[0].settings.remove("colour");
        preset.effects[0]
            .settings
            .insert("amount".into(), 0.75.into());
        // A setting this build does not know is ignored, a value out of range brought into it.
        preset.effects[0].settings.insert("legacy".into(), 3.into());
        crate::vj::apply(
            &mut project,
            VjOp::ApplyEffectPreset {
                effect: effect.clone(),
                preset,
            },
            counter(),
        )
        .unwrap();
        let now = settings(&project, &effect);
        assert_eq!(now["colour"], "#00ff00");
        assert_eq!(now["amount"], 0.75);
        assert!(!now.contains_key("legacy"));
    }

    #[test]
    fn a_stack_preset_should_add_its_effects_with_new_ids_where_they_are_allowed() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let first = add_effect(
            &mut project,
            EffectTarget::Layer(layer.clone()),
            "brightness",
        );
        add_effect(&mut project, EffectTarget::Layer(layer.clone()), "tint");
        let target = EffectTarget::Layer(layer);
        let preset = EffectPreset::of_stack(&project.vj, &target, "s".into(), "Look").unwrap();
        assert_eq!(preset.single(), None);
        assert!(
            crate::vj::apply(
                &mut project,
                VjOp::ApplyEffectPreset {
                    effect: first,
                    preset: preset.clone()
                },
                counter()
            )
            .is_err()
        );
        let op = VjOp::AddEffectPreset {
            target: target.clone(),
            preset: preset.clone(),
        };
        crate::vj::apply(&mut project, op, counter_from(500)).unwrap();
        let ids: Vec<_> = stack(&project.vj, &target)
            .unwrap()
            .iter()
            .map(|e| &e.id)
            .collect();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            ids.iter().collect::<std::collections::HashSet<_>>().len(),
            4
        );
        project.validate().unwrap();
        let empty =
            EffectPreset::of_stack(&project.vj, &EffectTarget::Composition, "e".into(), "None");
        assert!(empty.is_err());
    }

    #[test]
    fn a_template_should_keep_the_structure_and_lose_the_clips() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let column = project.vj.decks[0].columns[0].id.clone();
        run(
            &mut project,
            VjOp::PlaceGenerator {
                layer: layer.clone(),
                column,
                generator: crate::generator::Generator::Bars,
            },
        );
        add_effect(&mut project, EffectTarget::Layer(layer.clone()), "tint");
        project.vj.layers[0].blend = crate::vj::Blend::Screen;
        project.vj.layers[0].transition = 2.0;
        project.vj.layers[0].transition_effect = crate::vj::Blend::Multiply;
        project.videos.push(crate::VideoAsset {
            id: "v".into(),
            name: "v".into(),
            file: "video/v.mp4".into(),
            link: None,
        });
        assert!(!project.vj.layers[0].clips.is_empty());
        let template = Template::of(&project, "Club").unwrap();
        let text = serde_json::to_string(&template).unwrap();
        let back = Template::parse(&text).unwrap();
        assert_eq!(back, template);
        let made = back.project("Night").unwrap();
        assert_eq!(made.name, "Night");
        assert!(made.videos.is_empty());
        assert!(made.vj.layers.iter().all(|l| l.clips.is_empty()));
        assert_eq!(made.vj.layers.len(), project.vj.layers.len());
        assert_eq!(made.vj.decks, project.vj.decks);
        let first = &made.vj.layers[0];
        assert_eq!(first.effects, project.vj.layers[0].effects);
        assert_eq!(
            (first.blend, first.transition, first.transition_effect),
            (crate::vj::Blend::Screen, 2.0, crate::vj::Blend::Multiply)
        );
        assert_ne!(made.meta, project.meta);
        // A timeline show is no template.
        let timeline = Project::new("Cues", crate::FrameRate::Fps30);
        assert!(Template::of(&timeline, "x").is_err());
    }

    #[test]
    fn a_template_should_drop_the_mappings_of_the_clips_it_lost() {
        let mut project = show();
        let layer = project.vj.layers[0].id.clone();
        let column = project.vj.decks[0].columns[0].id.clone();
        run(
            &mut project,
            VjOp::PlaceGenerator {
                layer: layer.clone(),
                column,
                generator: crate::generator::Generator::Bars,
            },
        );
        let clip = project.vj.layers[0].clips[0].id.clone();
        let source = crate::vj::Source::Midi {
            port: None,
            channel: 1,
            kind: crate::vj_mapping::MidiKind::Note,
            number: 5,
        };
        run(
            &mut project,
            VjOp::AddMapping {
                source: source.clone(),
                target: crate::vj::Target::ClipLaunch { clip },
            },
        );
        let other = crate::vj::Source::Midi {
            port: None,
            channel: 1,
            kind: crate::vj_mapping::MidiKind::Note,
            number: 6,
        };
        run(
            &mut project,
            VjOp::AddMapping {
                source: other,
                target: crate::vj::Target::LayerStop { layer },
            },
        );
        assert_eq!(project.vj.mappings.len(), 2);
        let made = Template::of(&project, "Map")
            .unwrap()
            .project("New")
            .unwrap();
        assert_eq!(made.vj.mappings.len(), 1);
    }

    #[test]
    fn templates_should_be_stored_listed_renamed_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let template = Template::of(&show(), "Zeta").unwrap();
        let info = save_template(dir.path(), &template, "t-1".into()).unwrap();
        assert_eq!(info.name, "Zeta");
        assert!(info.layers > 0 && info.columns > 0);
        let other = Template::of(&show(), "alpha").unwrap();
        save_template(dir.path(), &other, "t-2".into()).unwrap();
        fs::write(dir.path().join("junk.json"), "{").unwrap();
        let names: Vec<_> = list_templates(dir.path())
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["alpha", "Zeta"]);
        rename_template(dir.path(), "t-1", "Beta").unwrap();
        assert_eq!(load_template(dir.path(), "t-1").unwrap().name, "Beta");
        delete_template(dir.path(), "t-1").unwrap();
        assert!(load_template(dir.path(), "t-1").is_err());
        assert!(load_template(dir.path(), "../x").is_err());
    }

    #[test]
    fn a_template_of_a_newer_version_should_be_refused() {
        let template = Template::of(&show(), "Club").unwrap();
        let mut value = serde_json::to_value(&template).unwrap();
        value["version"] = (VERSION + 1).into();
        assert!(Template::parse(&value.to_string()).is_err());
        // Even when its shape is one this build cannot read.
        assert!(Template::parse(&format!(r#"{{"version": {}}}"#, VERSION + 1)).is_err());
    }
}
