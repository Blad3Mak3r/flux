use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid project.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("archive has no project.json")]
    MissingProjectJson,
    #[error("project format version {found} is newer than the supported version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("asset is missing from the archive: {0}")]
    MissingAsset(String),
    #[error("invalid asset path: {0}")]
    InvalidAssetPath(String),
    #[error("invalid project: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ProjectError>;
