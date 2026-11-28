use super::*;

pub(super) fn entry() -> Result<keyring::v1::Entry> {
    keyring::v1::Entry::new("Flux Timecode", "elevenlabs-api-key").map_err(|_| Error::SecureStore)
}

pub(super) fn key(state: &VoiceState) -> Result<String> {
    if let Some(key) = state
        .key
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        return Ok(key);
    }
    entry()?.get_password().map_err(|error| match error {
        keyring::v1::Error::NoEntry => Error::MissingKey,
        _ => Error::SecureStore,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    configured: bool,
    session_only: bool,
}

#[tauri::command]
pub async fn voice_key_status(state: State<'_, Arc<VoiceState>>) -> Result<KeyStatus> {
    let state = Arc::clone(&state);
    worker(move || {
        Ok(KeyStatus {
            configured: key(&state).is_ok(),
            session_only: *state
                .session_only
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        })
    })
    .await
}

#[tauri::command]
pub async fn voice_key_set(
    state: State<'_, Arc<VoiceState>>,
    api_key: String,
    session_only: bool,
) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        if api_key.trim().is_empty() || api_key.len() > 1024 || api_key.contains(['\r', '\n']) {
            return Err(Error::InvalidInput);
        }
        if !session_only {
            entry()?
                .set_password(api_key.trim())
                .map_err(|_| Error::SecureStore)?;
        }
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = Some(api_key.trim().into());
        *state
            .session_only
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = session_only;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn voice_key_delete(state: State<'_, Arc<VoiceState>>) -> Result<()> {
    let state = Arc::clone(&state);
    worker(move || {
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::v1::Error::NoEntry) => {}
            Err(_) => return Err(Error::SecureStore),
        }
        state
            .voices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *state.key.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *state
            .session_only
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = false;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn voice_list(state: State<'_, Arc<VoiceState>>) -> Result<Vec<flux_voice::Voice>> {
    let state = Arc::clone(&state);
    worker(move || {
        let api_key = key(&state)?;
        let voices = flux_voice::SpeechClient::new()
            .and_then(|client| client.voices(&api_key))
            .map_err(|error| Error::Provider { error })?;
        *state.voices.lock().unwrap_or_else(PoisonError::into_inner) = voices
            .iter()
            .map(|v| (v.voice_id.clone(), v.name.clone()))
            .collect();
        Ok(voices)
    })
    .await
}
