use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cli::Cli;

const SETTINGS_FILE: &str = "settings.json";

pub fn default_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Flux");
    }
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("flux");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("flux")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SavedSettings {
    pub listen: SocketAddr,
    pub universe: u16,
    pub device: Option<String>,
    pub channels: u16,
    pub fps: u16,
}

impl From<&Cli> for SavedSettings {
    fn from(cli: &Cli) -> Self {
        Self {
            listen: cli.listen,
            universe: cli.universe,
            device: cli.device.clone(),
            channels: cli.channels,
            fps: cli.fps,
        }
    }
}

impl SavedSettings {
    pub fn validate(&self) -> Result<()> {
        if self.universe > 0x7fff {
            bail!("Universe must be between 0 and 32767");
        }
        if !(1..=512).contains(&self.channels) {
            bail!("DMX channels must be between 1 and 512");
        }
        if !(1..=44).contains(&self.fps) {
            bail!("Refresh rate must be between 1 and 44 Hz");
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<Option<SavedSettings>> {
    let file = path.join(SETTINGS_FILE);
    match fs::read_to_string(&file) {
        Ok(content) => {
            let settings: SavedSettings = serde_json::from_str(&content)
                .with_context(|| format!("Unable to parse {}", file.display()))?;
            settings
                .validate()
                .with_context(|| format!("Invalid settings in {}", file.display()))?;
            Ok(Some(settings))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Unable to read {}", file.display())),
    }
}

pub fn save(path: &Path, settings: &SavedSettings) -> Result<()> {
    settings.validate()?;
    fs::create_dir_all(path).with_context(|| format!("Unable to create {}", path.display()))?;
    let file = path.join(SETTINGS_FILE);
    let data = serde_json::to_string_pretty(settings).context("Unable to serialize settings")?;
    fs::write(&file, data).with_context(|| format!("Unable to write {}", file.display()))
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::SavedSettings;

    fn settings() -> SavedSettings {
        SavedSettings {
            listen: "127.0.0.1:6454".parse::<SocketAddr>().unwrap(),
            universe: 0,
            device: None,
            channels: 512,
            fps: 30,
        }
    }

    #[test]
    fn validates_supported_settings() {
        assert!(settings().validate().is_ok());
    }

    #[test]
    fn rejects_out_of_range_runtime_settings() {
        let mut invalid = settings();
        invalid.channels = 513;
        assert!(invalid.validate().is_err());

        invalid = settings();
        invalid.fps = 45;
        assert!(invalid.validate().is_err());

        invalid = settings();
        invalid.universe = 0x8000;
        assert!(invalid.validate().is_err());
    }
}
