use serde::{Serialize, Serializer};
use thiserror::Error;

use crate::project::ProjectError;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("could not read this audio file: {0}")]
    Decode(String),
    #[error("the file has no audio track")]
    NoAudioTrack,
    #[error("could not convert the sample rate: {0}")]
    Resample(String),
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error("the audio worker stopped: {0}")]
    Worker(String),
}

/// Tauri sends command errors to the frontend as serialized values: use the message.
impl Serialize for AudioError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
