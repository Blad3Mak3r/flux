//! Tauri commands for reading imported audio: how long it is and what it looks like.

use std::io;
use std::path::Path;
use std::sync::{Arc, PoisonError};

use serde::Serialize;
use tauri::State;

use crate::audio::{AudioError, Decoded, PEAKS_PER_SECOND, SharedAudio, decode, peaks};
use crate::commands::SharedSession;
use crate::project::AudioAsset;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub duration_seconds: f64,
    /// Sample rate of the file itself, before conversion to the engine's.
    pub source_rate: u32,
    pub peaks_per_second: u32,
    /// `[min0, max0, min1, max1, ...]` as signed bytes, full scale at -128 and 127.
    pub peaks: Vec<i8>,
}

/// The decoded audio of `asset`, decoding it the first time and remembering it afterwards.
pub fn load(
    session: &SharedSession,
    cache: &SharedAudio,
    asset: &AudioAsset,
) -> Result<Arc<Decoded>, AudioError> {
    if let Some(decoded) = cache.get(&asset.id) {
        return Ok(decoded);
    }
    // The lock is held only to read the bytes, never while decoding.
    let bytes = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .asset_bytes(asset)?;
    let extension = Path::new(&asset.file)
        .extension()
        .and_then(|extension| extension.to_str());
    let decoded = Arc::new(decode(bytes, extension)?);
    cache.insert(&asset.id, Arc::clone(&decoded));
    Ok(decoded)
}

/// Decodes an asset and returns its real length and waveform. Run when audio is imported or a
/// project is opened.
#[tauri::command]
pub async fn audio_analyze(
    session: State<'_, SharedSession>,
    cache: State<'_, SharedAudio>,
    asset: AudioAsset,
) -> Result<AudioInfo, AudioError> {
    let (session, cache) = (Arc::clone(&session), Arc::clone(&cache));
    tauri::async_runtime::spawn_blocking(move || {
        let decoded = load(&session, &cache, &asset)?;
        Ok(AudioInfo {
            duration_seconds: decoded.duration_seconds(),
            source_rate: decoded.source_rate,
            peaks_per_second: PEAKS_PER_SECOND,
            peaks: peaks(&decoded),
        })
    })
    .await
    .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))?
}
