//! Credentials and speech workers. Provider responses and secrets never reach diagnostics.
pub(crate) mod preview;
pub(crate) mod recording;

use crate::voice_jobs::{JobStore, Recording, Snapshot, StoredJob};
use crate::{
    commands::SharedSession,
    preferences,
    transport_commands::{SharedOutput, is_playing},
};
use flux_project::{GeneratedVoice, OperatorOp, Project, ProjectState, TrackKind, VoiceView};
use serde::Serialize;
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tauri::{AppHandle, Manager, State};

pub(crate) mod activity;
pub(crate) mod jobs;
pub(crate) mod operator;
pub(crate) mod provider;
mod transport;

use self::jobs::*;
use self::operator::*;
use self::provider::*;
pub(crate) use self::transport::send_show;
use self::transport::*;
pub use self::transport::{send_transport, stop_preview};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum Error {
    MissingKey,
    SecureStore,
    Busy,
    Stopped,
    MissingVoice,
    InvalidInput,
    Obsolete,
    Storage,
    Audio,
    Capture,
    Limit,
    MissingRoute,
    Output,
    Provider { error: flux_voice::Error },
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Default)]
pub struct VoiceState {
    key: Mutex<Option<String>>,
    recording: Mutex<Option<recording::ActiveRecording>>,
    session_only: Mutex<bool>,
    pending: Mutex<Option<Pending>>,
    serial: AtomicU64,
    preview: Mutex<Option<flux_engine::output::Output>>,
    control: Mutex<()>,
    preview_epoch: AtomicU64,
    voices: Mutex<std::collections::HashMap<String, String>>,
    transitioning: AtomicBool,
}

struct Pending {
    token: u64,
    unsaved: Option<Arc<Unsaved>>,
    saving: bool,
}

struct Unsaved {
    origin: flux_project::operator::RecordingOrigin,
    extension: String,
    voice_name: Option<String>,
    snapshot: Snapshot,
    session_id: String,
    bytes: Vec<u8>,
    samples: u64,
}

/// A worker unwinding must release its reservation without losing an already received response.
struct Reservation<'a> {
    state: &'a VoiceState,
    token: u64,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut pending = self
            .state
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pending
            .as_ref()
            .is_some_and(|p| p.token == self.token && p.unsaved.is_none())
        {
            *pending = None;
        }
    }
}

impl VoiceState {
    fn reserve(&self) -> Result<u64> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        if pending.is_some() || self.transitioning.load(Ordering::Acquire) {
            return Err(Error::Busy);
        }
        let token = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        *pending = Some(Pending {
            token,
            unsaved: None,
            saving: false,
        });
        Ok(token)
    }

    pub fn begin_transition(&self) -> flux_project::Result<VoiceTransition<'_>> {
        let _gate = self.control.lock().unwrap_or_else(PoisonError::into_inner);
        if self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
            || self.transitioning.swap(true, Ordering::AcqRel)
        {
            return Err(flux_project::ProjectError::Invalid(
                "voice work must finish and be saved before closing".into(),
            ));
        }
        invalidate_preview(self);
        Ok(VoiceTransition(self))
    }
}

pub struct VoiceTransition<'a>(&'a VoiceState);

impl Drop for VoiceTransition<'_> {
    fn drop(&mut self) {
        self.0.transitioning.store(false, Ordering::Release);
    }
}

const MAX_VOICE_FRAMES: u64 = flux_core::ENGINE_RATE as u64 * 3600;

fn decode_recording(bytes: Vec<u8>, extension: &str) -> Result<flux_audio::Decoded> {
    flux_audio::decode_limited(bytes, Some(extension), MAX_VOICE_FRAMES).map_err(|error| {
        if matches!(error, flux_audio::AudioError::Limit) {
            Error::Limit
        } else {
            Error::Audio
        }
    })
}

fn snapshot(project: &Project, id: &str, local: Option<&str>) -> Result<Snapshot> {
    for timeline in &project.timelines {
        for track in &timeline.tracks {
            if let TrackKind::Operator { orders, .. } = &track.kind
                && let Some(order) = orders.iter().find(|o| o.id == id)
            {
                let voice = flux_project::operator::effective_voice(project, track, local)
                    .0
                    .ok_or(Error::MissingVoice)?;
                if order.dialogue.trim().is_empty() && local != Some("recorded") {
                    return Err(Error::InvalidInput);
                }
                return Ok(Snapshot {
                    mode: order.mode,
                    synthesis_text: order.synthesis_text.clone(),
                    project: project.meta.id.clone(),
                    timeline: timeline.id.clone(),
                    order: id.into(),
                    dialogue: order.dialogue.clone(),
                    voice: voice.into(),
                    previous_asset: order.generated.as_ref().map(|g| g.asset.clone()),
                });
            }
        }
    }
    Err(Error::Obsolete)
}

async fn worker<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| Error::Storage)?
}

#[cfg(test)]
mod tests;
