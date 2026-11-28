use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;
use zip::result::ZipError;

use super::error::{ProjectError, Result};
use super::model::{check_asset_path, check_video_path};

const PROJECT_JSON: &str = "project.json";

type FileArchive = ZipArchive<BufReader<File>>;

/// Where `save` takes the bytes of an asset from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "camelCase")]
pub enum AssetSource {
    /// A newly imported file on disk.
    Disk(PathBuf),
    /// An existing `.fxtc`; the entry named like the asset's `file` is copied without recompressing.
    Archive(PathBuf),
}

/// An opened `.fxtc`. Audio is streamed from the archive entry by entry, never loaded whole.
pub struct ProjectArchive {
    archive: FileArchive,
}

impl ProjectArchive {
    /// Streams the audio entry stored at `file` (an asset's archive path).
    pub fn asset_reader(&mut self, file: &str) -> Result<impl Read + '_> {
        check_asset_path(file)?;
        by_name(&mut self.archive, file)
    }

    /// Streams the video entry stored at `file` (a video's archive path).
    pub fn video_reader(&mut self, file: &str) -> Result<impl Read + '_> {
        check_video_path(file)?;
        by_name(&mut self.archive, file)
    }
}

fn open_archive(path: &Path) -> Result<FileArchive> {
    Ok(ZipArchive::new(BufReader::new(File::open(path)?))?)
}

fn by_name<'a>(
    archive: &'a mut FileArchive,
    file: &str,
) -> Result<zip::read::ZipFile<'a, BufReader<File>>> {
    archive.by_name(file).map_err(|err| match err {
        ZipError::FileNotFound => ProjectError::MissingAsset(file.to_owned()),
        other => other.into(),
    })
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

mod copy;
mod read;
#[cfg(test)]
mod support;
mod write;

pub(crate) use copy::*;
pub use read::*;
pub use write::*;
