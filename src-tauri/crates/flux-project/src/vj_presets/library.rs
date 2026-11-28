//! The library file of the user's effect presets.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::*;
use crate::{ProjectError, Result};

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

pub(crate) fn invalid(err: serde_json::Error) -> ProjectError {
    ProjectError::Invalid(err.to_string())
}

pub(super) fn check_version(version: u32, what: &str) -> Result<()> {
    if version == 0 || version > VERSION {
        return Err(ProjectError::Invalid(format!(
            "this {what} is version {version}; this Flux reads up to {VERSION}"
        )));
    }
    Ok(())
}

/// Written to a temporary file and renamed, so a crash cannot leave half a file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
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

#[cfg(test)]
mod tests {

    use std::fs;

    use super::*;

    use crate::vj::EffectTarget;

    use crate::vj_presets::support::*;

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
}
