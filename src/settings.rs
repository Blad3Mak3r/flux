use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::Cli;

const SETTINGS_FILE: &str = "settings.json";

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

pub fn load(path: &PathBuf) -> Result<Option<SavedSettings>> {
    let file = path.join(SETTINGS_FILE);
    match fs::read_to_string(&file) {
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("Unable to parse {}", file.display()))
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Unable to read {}", file.display())),
    }
}

pub fn save(path: &PathBuf, settings: &SavedSettings) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("Unable to create {}", path.display()))?;
    let file = path.join(SETTINGS_FILE);
    let data = serde_json::to_string_pretty(settings).context("Unable to serialize settings")?;
    fs::write(&file, data).with_context(|| format!("Unable to write {}", file.display()))
}
