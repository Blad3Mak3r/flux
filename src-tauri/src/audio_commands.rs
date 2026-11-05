//! Tauri commands for reading imported audio: how long it is and what it looks like.

use std::io;
use std::path::Path;
use std::sync::{Arc, PoisonError};

use tauri::State;

use crate::audio::{AudioError, Decoded, PEAKS_PER_SECOND, SharedAudio, decode, detect_bpm, peaks};
use crate::commands::SharedSession;
use crate::project::{AudioAsset, AudioStamp, Preview};

/// The decoded audio of `asset`, decoding it the first time and remembering it afterwards.
pub fn load(
    session: &SharedSession,
    cache: &SharedAudio,
    asset: &AudioAsset,
) -> Result<Arc<Decoded>, AudioError> {
    if let Some(decoded) = cache.get(&asset.id) {
        return Ok(decoded);
    }
    let (_, decoded) = decode_asset(session, asset)?;
    let decoded = Arc::new(decoded);
    cache.insert(&asset.id, Arc::clone(&decoded));
    Ok(decoded)
}

/// Decodes `asset`, returning the stamp of the bytes it was decoded from.
fn decode_asset(
    session: &SharedSession,
    asset: &AudioAsset,
) -> Result<(AudioStamp, Decoded), AudioError> {
    // The lock is held only to read the bytes, never while decoding.
    let bytes = session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .asset_bytes(asset)?;
    let stamp = AudioStamp::of(&bytes);
    let extension = Path::new(&asset.file)
        .extension()
        .and_then(|extension| extension.to_str());
    Ok((stamp, decode(bytes, extension)?))
}

/// The length and waveform of `asset`: the stored preview if there is a valid one, otherwise decoded
/// (and kept for the next save).
fn analyze(
    session: &SharedSession,
    cache: &SharedAudio,
    asset: &AudioAsset,
) -> Result<Preview, AudioError> {
    let lock = || session.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(preview) = lock().preview(asset)? {
        return Ok(preview);
    }
    let (stamp, decoded) = decode_asset(session, asset)?;
    let preview = Preview {
        duration_seconds: decoded.duration_seconds(),
        source_rate: decoded.source_rate,
        peaks_per_second: PEAKS_PER_SECOND,
        peaks: peaks(&decoded),
    };
    // Playback will want the samples soon, so keep them rather than decode twice.
    cache.insert(&asset.id, Arc::new(decoded));
    lock().remember_preview(asset, stamp, preview.clone());
    Ok(preview)
}

/// Returns the real length and waveform of an asset. Run when audio is imported or a project is
/// opened; it decodes only an asset that has no stored preview yet.
#[tauri::command]
pub async fn audio_analyze(
    session: State<'_, SharedSession>,
    cache: State<'_, SharedAudio>,
    asset: AudioAsset,
) -> Result<Preview, AudioError> {
    let (session, cache) = (Arc::clone(&session), Arc::clone(&cache));
    tauri::async_runtime::spawn_blocking(move || analyze(&session, &cache, &asset))
        .await
        .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))?
}

/// The tempo of an asset in BPM, or `null` when it shows no steady pulse. It uses the decoded audio
/// when playback or the analysis already has it, and otherwise decodes the file just for this,
/// without keeping it: asking for the tempo of every song of a big show must not fill the memory
/// the way playing them does. A project stores the answer with the asset.
#[tauri::command]
pub async fn audio_detect_bpm(
    session: State<'_, SharedSession>,
    cache: State<'_, SharedAudio>,
    asset: AudioAsset,
) -> Result<Option<f64>, AudioError> {
    let (session, cache) = (Arc::clone(&session), Arc::clone(&cache));
    tauri::async_runtime::spawn_blocking(move || {
        let decoded = match cache.get(&asset.id) {
            Some(decoded) => decoded,
            None => Arc::new(decode_asset(&session, &asset)?.1),
        };
        Ok(detect_bpm(&decoded))
    })
    .await
    .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))?
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use tempfile::TempDir;

    use super::*;
    use crate::project::{FrameRate, Session};

    /// One second of a 48 kHz mono 16-bit WAV with a ramp, so its peaks are not silence.
    fn wav(seconds: u32) -> Vec<u8> {
        let samples: Vec<i16> = (0..48_000 * seconds)
            .map(|n| (n % 2_000) as i16 * 10)
            .collect();
        let data_len = (samples.len() * 2) as u32;
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36 + data_len).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes()); // PCM
        bytes.extend(1u16.to_le_bytes()); // mono
        bytes.extend(48_000u32.to_le_bytes());
        bytes.extend(96_000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_len.to_le_bytes());
        bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
        bytes
    }

    /// Saves a project holding one imported WAV, analysed first only if `analysed`.
    fn saved(dir: &TempDir, analysed: bool) -> (AudioAsset, PathBuf) {
        let file = dir.path().join("click.wav");
        fs::write(&file, wav(2)).unwrap();
        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_audio(&file).unwrap();
        project.assets.push(asset.clone());
        let path = dir.path().join("show.fxtc");
        let session = Arc::new(Mutex::new(session));
        if analysed {
            analyze(&session, &SharedAudio::default(), &asset).unwrap();
        }
        session.lock().unwrap().save(&project, &path).unwrap();
        (asset, path)
    }

    fn reopened(path: &Path) -> SharedSession {
        let mut session = Session::default();
        session.open(path).unwrap();
        Arc::new(Mutex::new(session))
    }

    #[test]
    fn analyze_should_not_decode_a_project_saved_with_its_preview() {
        let dir = TempDir::new().unwrap();
        let (asset, path) = saved(&dir, true);
        let expected = analyze(&reopened(&path), &SharedAudio::default(), &asset).unwrap();

        let cache = SharedAudio::default();
        let preview = analyze(&reopened(&path), &cache, &asset).unwrap();

        assert_eq!(preview, expected);
        assert_eq!(preview.peaks.len(), 2 * 100 * 2);
        assert!((preview.duration_seconds - 2.0).abs() < 1e-9);
        assert!(cache.get(&asset.id).is_none(), "the audio was decoded");
    }

    #[test]
    fn analyze_should_decode_when_the_archive_has_no_preview_and_save_it_for_next_time() {
        let dir = TempDir::new().unwrap();
        let (asset, path) = saved(&dir, false);
        let session = reopened(&path);
        let cache = SharedAudio::default();

        let preview = analyze(&session, &cache, &asset).unwrap();
        assert!(cache.get(&asset.id).is_some(), "the audio was not decoded");

        let project = Session::default().open(&path).unwrap();
        session.lock().unwrap().save(&project, &path).unwrap();
        let cache = SharedAudio::default();
        let again = analyze(&reopened(&path), &cache, &asset).unwrap();
        assert_eq!(again, preview);
        assert!(cache.get(&asset.id).is_none(), "the audio was decoded");
    }
}
