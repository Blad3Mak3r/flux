//! The shows opened or saved most recently, for the welcome window.
//!
//! The list lives in `recent.json` in the app data directory and holds only paths and times: what
//! is shown about each show is read from the file when the list is requested, so it can never be
//! stale. A list that is missing or unreadable is an empty list, never an error.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::project::{ProjectError, ProjectMeta, read_meta};

const FILE_NAME: &str = "recent.json";
const MAX_RECENTS: usize = 12;

/// Windows can open or save at the same time, and each change reads and rewrites the whole file.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Recent {
    path: PathBuf,
    /// Seconds since the Unix epoch.
    opened_at: u64,
}

/// A recent show as the welcome window lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentShow {
    pub path: PathBuf,
    pub opened_at: u64,
    /// `false` when the file was moved or deleted outside Flux.
    pub exists: bool,
    /// `None` when the file is missing or is not a readable project.
    pub meta: Option<ProjectMeta>,
}

/// Puts `path` first, once, and keeps the list within its limit.
fn touch(list: &mut Vec<Recent>, path: &Path, now: u64) {
    forget(list, path);
    list.insert(
        0,
        Recent {
            path: path.to_owned(),
            opened_at: now,
        },
    );
    list.truncate(MAX_RECENTS);
}

fn forget(list: &mut Vec<Recent>, path: &Path) {
    list.retain(|recent| recent.path != path);
}

fn load(file: &Path) -> Vec<Recent> {
    fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Written to a temporary file and renamed, so a crash cannot leave half a list.
fn store(file: &Path, list: &[Recent]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    fs::rename(&tmp, file)
}

fn describe(list: Vec<Recent>) -> Vec<RecentShow> {
    list.into_iter()
        .map(|recent| {
            let meta = read_meta(&recent.path).ok();
            RecentShow {
                exists: meta.is_some() || recent.path.is_file(),
                meta,
                path: recent.path,
                opened_at: recent.opened_at,
            }
        })
        .collect()
}

fn list_file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join(FILE_NAME))
}

fn update(app: &AppHandle, change: impl FnOnce(&mut Vec<Recent>)) {
    let Some(file) = list_file(app) else {
        return;
    };
    let _guard = WRITE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut list = load(&file);
    change(&mut list);
    // The list is a convenience: failing to save it must never fail opening or saving a show.
    if let Err(err) = store(&file, &list) {
        log::warn!("could not save the recent shows: {err}");
    }
}

/// Records that `path` was just opened or saved.
pub fn remember(app: &AppHandle, path: &Path) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    update(app, |list| touch(list, path, now));
}

/// Removes `path` from the list; the file itself is not touched.
pub fn remove(app: &AppHandle, path: &Path) {
    update(app, |list| forget(list, path));
}

#[tauri::command]
pub async fn recents_list(app: AppHandle) -> Result<Vec<RecentShow>, ProjectError> {
    tauri::async_runtime::spawn_blocking(move || {
        let list = list_file(&app).map(|file| load(&file)).unwrap_or_default();
        describe(list)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))
}

#[tauri::command]
pub fn recents_forget(app: AppHandle, path: PathBuf) {
    remove(&app, &path);
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn recent(path: &str, opened_at: u64) -> Recent {
        Recent {
            path: PathBuf::from(path),
            opened_at,
        }
    }

    #[test]
    fn touch_should_put_the_show_first() {
        let mut list = vec![recent("/a.fxtc", 1)];

        touch(&mut list, Path::new("/b.fxtc"), 2);

        assert_eq!(list, [recent("/b.fxtc", 2), recent("/a.fxtc", 1)]);
    }

    #[test]
    fn touch_should_not_repeat_a_show_it_already_has() {
        let mut list = vec![recent("/a.fxtc", 1), recent("/b.fxtc", 2)];

        touch(&mut list, Path::new("/a.fxtc"), 3);

        assert_eq!(list, [recent("/a.fxtc", 3), recent("/b.fxtc", 2)]);
    }

    #[test]
    fn touch_should_drop_the_oldest_beyond_the_limit() {
        let mut list = Vec::new();
        for n in 0..=MAX_RECENTS as u64 {
            touch(&mut list, Path::new(&format!("/{n}.fxtc")), n);
        }

        assert_eq!(list.len(), MAX_RECENTS);
        assert_eq!(list[0].path, Path::new(&format!("/{MAX_RECENTS}.fxtc")));
        assert!(list.iter().all(|r| r.path != Path::new("/0.fxtc")));
    }

    #[test]
    fn forget_should_remove_only_that_show() {
        let mut list = vec![recent("/a.fxtc", 1), recent("/b.fxtc", 2)];

        forget(&mut list, Path::new("/a.fxtc"));

        assert_eq!(list, [recent("/b.fxtc", 2)]);
    }

    #[test]
    fn store_then_load_should_round_trip() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data").join(FILE_NAME);
        let list = vec![recent("/a.fxtc", 1), recent("/b.fxtc", 2)];

        store(&file, &list).unwrap();

        assert_eq!(load(&file), list);
    }

    #[test]
    fn load_should_be_empty_for_a_missing_or_corrupt_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join(FILE_NAME);
        assert!(load(&file).is_empty());

        fs::write(&file, b"{ not json").unwrap();
        assert!(load(&file).is_empty());
    }

    #[test]
    fn describe_should_flag_a_missing_file_and_an_unreadable_one() {
        let dir = TempDir::new().unwrap();
        let junk = dir.path().join("junk.fxtc");
        fs::write(&junk, b"not a zip").unwrap();
        let gone = dir.path().join("gone.fxtc");

        let shows = describe(vec![
            Recent {
                path: junk,
                opened_at: 1,
            },
            Recent {
                path: gone,
                opened_at: 2,
            },
        ]);

        assert!(shows[0].exists && shows[0].meta.is_none());
        assert!(!shows[1].exists && shows[1].meta.is_none());
    }
}
