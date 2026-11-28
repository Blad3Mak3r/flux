//! Media kept outside the archive. A video can weigh several GB, so a project refers to its media by
//! path (see [`MediaLink`]) and a file that is gone is *offline*: shown as such, never an error.
//! The functions here are pure edits of a [`Project`]; the session wires them to its sources.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::model::{MediaLink, Project, TrackKind};

pub const AUDIO_EXTENSIONS: &[&str] = &["wav", "aif", "aiff", "flac", "mp3", "ogg"];
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "webm"];
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];

/// How deep and how wide a folder search goes: a drive root must not hang the app.
const SEARCH_DEPTH: usize = 8;
const SEARCH_ENTRIES: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaKind {
    Audio,
    Video,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaState {
    Online,
    Offline,
    /// Inside the archive: it cannot go missing.
    Embedded,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub name: String,
    pub kind: MediaKind,
    pub state: MediaState,
    pub path: Option<String>,
    /// Clips (and test cards) that show it.
    pub uses: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaList {
    pub items: Vec<MediaItem>,
    pub missing: u32,
}

/// What a media entry of a project is, whichever list it is in.
struct Entry<'a> {
    id: &'a str,
    name: &'a str,
    file: &'a str,
    link: Option<&'a MediaLink>,
    audio: bool,
}

impl Entry<'_> {
    fn kind(&self) -> MediaKind {
        if self.audio {
            MediaKind::Audio
        } else {
            kind_of(Path::new(self.file)).unwrap_or(MediaKind::Video)
        }
    }

    fn offline(&self) -> bool {
        self.link.is_some_and(|link| !is_file(&link.path))
    }
}

fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

/// The kind of media a file is by its extension, or `None` for one that is none of them.
pub fn kind_of(file: &Path) -> Option<MediaKind> {
    let extension = file.extension()?.to_str()?.to_ascii_lowercase();
    let is = |list: &[&str]| list.contains(&extension.as_str());
    if is(AUDIO_EXTENSIONS) {
        Some(MediaKind::Audio)
    } else if is(VIDEO_EXTENSIONS) {
        Some(MediaKind::Video)
    } else if is(IMAGE_EXTENSIONS) {
        Some(MediaKind::Image)
    } else {
        None
    }
}

/// The generated speech of the project: it stays inside the archive and is not user media.
fn voice_ids(project: &Project) -> HashSet<&str> {
    let tracks = project.timelines.iter().flat_map(|t| &t.tracks);
    tracks
        .filter_map(|track| match &track.kind {
            TrackKind::Operator { orders, .. } => Some(orders),
            _ => None,
        })
        .flatten()
        .filter_map(|order| order.generated.as_ref().map(|g| g.asset.as_str()))
        .collect()
}

fn entries(project: &Project) -> Vec<Entry<'_>> {
    let voice = voice_ids(project);
    let audio = project.assets.iter().map(|a| Entry {
        id: &a.id,
        name: &a.name,
        file: &a.file,
        link: a.link.as_ref(),
        audio: true,
    });
    let video = project.videos.iter().map(|v| Entry {
        id: &v.id,
        name: &v.name,
        file: &v.file,
        link: v.link.as_ref(),
        audio: false,
    });
    audio
        .chain(video)
        .filter(|e| !voice.contains(e.id))
        .collect()
}

/// The link of the asset `id`, to set or clear.
fn link_mut<'a>(project: &'a mut Project, id: &str) -> Option<&'a mut Option<MediaLink>> {
    if let Some(asset) = project.assets.iter_mut().find(|a| a.id == id) {
        return Some(&mut asset.link);
    }
    project
        .videos
        .iter_mut()
        .find(|v| v.id == id)
        .map(|v| &mut v.link)
}

mod collect;
mod list;
mod relink;
#[cfg(test)]
mod support;

pub use collect::*;
pub use list::*;
pub use relink::*;
