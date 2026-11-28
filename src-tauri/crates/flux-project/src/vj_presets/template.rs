//! Show templates: the structure of a VJ show without its clips.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;
use crate::model::Project;
use crate::vj::ProjectMode;
use crate::vj_file::VjFile;
use crate::{ProjectError, Result};

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

pub(super) fn template_path(dir: &Path, id: &str) -> Result<PathBuf> {
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

    use std::fs;

    use super::*;
    use crate::model::Project;

    use crate::vj::{EffectTarget, VjOp};

    use crate::vj_presets::support::*;

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
