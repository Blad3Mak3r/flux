//! Tauri commands for the automatic update: look for a newer release and install it.
//!
//! The release is read from the `latest.json` the release workflow uploads to GitHub Releases
//! (the endpoint and the signing key are in `tauri.conf.json`). The window only shows what these
//! commands answer: a check finds a release or not, an install downloads it, verifies its
//! signature and restarts the app. FLUX drives timecode on stage, so an install is refused while
//! the transport plays.

use std::sync::{Mutex, PoisonError};

use serde::{Serialize, Serializer};
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::transport_commands::{SharedOutput, is_playing};

/// The release found by [`update_check`], waiting for [`update_install`].
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("stop the transport before installing an update")]
    Playing,
    #[error("check for updates first")]
    NothingPending,
    #[error("{0}")]
    Updater(#[from] tauri_plugin_updater::Error),
}

impl Serialize for UpdateError {
    /// The window translates the two codes; any other error is shown as the updater worded it.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Playing => serializer.serialize_str("updatePlaying"),
            Self::NothingPending => serializer.serialize_str("updateNothingPending"),
            Self::Updater(err) => serializer.collect_str(err),
        }
    }
}

/// Looks for a release newer than the running app and answers with its version. `None` means the
/// app is up to date.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<String>, UpdateError> {
    let found = app.updater()?.check().await;
    let found = match found {
        Ok(found) => found,
        Err(err) => {
            log::warn!("update check failed: {err}");
            return Err(err.into());
        }
    };
    let version = found.as_ref().map(|update| update.version.clone());
    match &version {
        Some(version) => log::info!("update {version} is available"),
        None => log::info!("the app is up to date"),
    }
    *pending.0.lock().unwrap_or_else(PoisonError::into_inner) = found;
    Ok(version)
}

/// Downloads the release [`update_check`] found, installs it and restarts the app.
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    output: State<'_, SharedOutput>,
) -> Result<(), UpdateError> {
    if is_playing(&output) {
        return Err(UpdateError::Playing);
    }
    // Taken out so the lock is not held while downloading.
    let update = pending
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or(UpdateError::NothingPending)?;
    log::info!("installing update {}", update.version);
    if let Err(err) = update.download_and_install(|_, _| {}, || {}).await {
        log::warn!("update could not be installed: {err}");
        return Err(err.into());
    }
    log::info!("update installed, restarting");
    app.restart()
}
