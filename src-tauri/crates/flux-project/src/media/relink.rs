//! Pointing media at another file, and searching a folder for what is missing.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::error::{ProjectError, Result};
use crate::model::{MediaLink, Project};

/// The files of `folder` by lower-cased name, for the offline media that match one.
fn folder_files(folder: &Path) -> HashMap<String, PathBuf> {
    let mut found = HashMap::new();
    if let Ok(read) = fs::read_dir(folder) {
        for entry in read.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                found
                    .entry(entry.file_name().to_string_lossy().to_lowercase())
                    .or_insert_with(|| entry.path());
            }
        }
    }
    found
}

fn linked_name(link: &MediaLink) -> Option<String> {
    Some(
        Path::new(&link.path)
            .file_name()?
            .to_string_lossy()
            .to_lowercase(),
    )
}

/// Links every offline media whose file name is in `files`. Returns how many it found.
fn relink_by_name(project: &mut Project, files: &HashMap<String, PathBuf>) -> usize {
    let matches: Vec<(String, PathBuf)> = entries(project)
        .into_iter()
        .filter(Entry::offline)
        .filter_map(|e| {
            let found = files.get(&linked_name(e.link?)?)?;
            Some((e.id.to_owned(), found.clone()))
        })
        .collect();
    let count = matches.len();
    for (id, path) in matches {
        if let Some(link) = link_mut(project, &id) {
            *link = Some(MediaLink {
                path: path.to_string_lossy().into_owned(),
                relative: None,
            });
        }
    }
    count
}

/// Links the media `id` to the file at `file`, which must be the same kind of media, and then every
/// other offline media found by name in the same folder. Returns how many it linked, `id` included.
pub fn relink(project: &mut Project, id: &str, file: &Path) -> Result<usize> {
    let file = std::path::absolute(file)?;
    if !file.is_file() {
        return Err(ProjectError::Invalid(format!(
            "not a file: {}",
            file.display()
        )));
    }
    let wanted = entries(project)
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.kind())
        .ok_or_else(|| ProjectError::MissingAsset(id.to_owned()))?;
    if kind_of(&file) != Some(wanted) {
        return Err(ProjectError::Invalid("media.error.kind".into()));
    }
    let path = file.to_string_lossy().into_owned();
    let slot = link_mut(project, id).ok_or_else(|| ProjectError::MissingAsset(id.to_owned()))?;
    *slot = Some(MediaLink {
        path,
        relative: None,
    });
    let siblings = file.parent().map(folder_files).unwrap_or_default();
    Ok(1 + relink_by_name(project, &siblings))
}

/// Looks for the offline media by file name in `folder` and the folders under it, the shallowest
/// match first. Returns how many it found and how many are still offline.
pub fn search_folder(project: &mut Project, folder: &Path) -> Result<(usize, usize)> {
    if !folder.is_dir() {
        return Err(ProjectError::Invalid(format!(
            "not a folder: {}",
            folder.display()
        )));
    }
    let wanted: HashSet<String> = entries(project)
        .into_iter()
        .filter(Entry::offline)
        .filter_map(|e| linked_name(e.link?))
        .collect();
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    let mut queue = VecDeque::from([(folder.to_path_buf(), 0usize)]);
    let mut seen = 0usize;
    'walk: while let Some((dir, depth)) = queue.pop_front() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            seen += 1;
            if seen > SEARCH_ENTRIES {
                break 'walk;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if depth < SEARCH_DEPTH && !name.starts_with('.') {
                    queue.push_back((entry.path(), depth + 1));
                }
            } else if kind.is_file() {
                let name = name.to_lowercase();
                if wanted.contains(&name) {
                    files.entry(name).or_insert_with(|| entry.path());
                }
            }
        }
        if files.len() == wanted.len() {
            break;
        }
    }
    let found = relink_by_name(project, &files);
    let still = entries(project).into_iter().filter(Entry::offline).count();
    Ok((found, still))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    use crate::error::ProjectError;

    use crate::media::support::*;
    use crate::test_util::write_file;

    #[test]
    fn relink_should_also_fix_the_offline_siblings_of_the_same_folder() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, _) = linked_show(&dir);
        session.update(project);
        fs::rename(dir.path().join("media"), dir.path().join("old")).unwrap();
        write_file(&dir.path().join("new"), "song.wav", b"song");
        write_file(&dir.path().join("new"), "clip.MP4", b"clip");
        let id = session.document().unwrap().project.assets[0].id.clone();

        let (state, count) = session
            .media_relink(&id, &dir.path().join("new").join("song.wav"))
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(list(&state.project).missing, 0);
    }

    #[test]
    fn relink_should_refuse_a_file_of_another_kind() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, _) = linked_show(&dir);
        session.update(project.clone());
        let image = write_file(dir.path(), "logo.png", b"png");

        let result = session.media_relink(&project.assets[0].id, &image);

        assert!(matches!(result, Err(ProjectError::Invalid(code)) if code == "media.error.kind"));
    }

    #[test]
    fn search_folder_should_relink_by_name_in_subfolders_and_report_the_rest() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, _) = linked_show(&dir);
        session.update(project);
        fs::remove_dir_all(dir.path().join("media")).unwrap();
        write_file(&dir.path().join("drive/a/b"), "SONG.wav", b"song");
        write_file(&dir.path().join("drive/.hidden"), "Clip.mp4", b"clip");

        let (state, found, left) = session.media_search(&dir.path().join("drive")).unwrap();

        assert_eq!((found, left), (1, 1));
        assert_eq!(list(&state.project).missing, 1);
    }
}
