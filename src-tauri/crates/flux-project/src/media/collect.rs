//! Collecting the media of a project into its own folder.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::error::{ProjectError, Result};
use crate::model::{AUDIO_DIR, MediaLink, Project, VIDEO_DIR};

/// One file to bring into the media folder.
#[derive(Debug, Clone)]
pub struct CollectJob {
    pub id: String,
    pub from: CollectFrom,
    /// The file name it gets in the media folder (before any clash is settled).
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub enum CollectFrom {
    File(PathBuf),
    /// An entry of the project archive.
    Entry {
        archive: PathBuf,
        file: String,
    },
}

/// What a collection will do: the folder it fills, the files it copies, and how many media it skips
/// because their file is gone.
#[derive(Debug, Clone)]
pub struct CollectPlan {
    pub folder: PathBuf,
    pub jobs: Vec<CollectJob>,
    pub skipped: usize,
}

/// The folder collected media goes to: `<project name> Media` beside the project file `flx`.
pub fn media_folder(flx: &Path) -> Option<PathBuf> {
    let stem = flx.file_stem()?.to_string_lossy();
    Some(flx.parent()?.join(format!("{stem} Media")))
}

/// The name an embedded entry had when it was imported: `<dir>/<uuid>_<name>` is `<name>`.
fn imported_name(file: &str) -> String {
    let name = file
        .strip_prefix(AUDIO_DIR)
        .or_else(|| file.strip_prefix(VIDEO_DIR))
        .unwrap_or(file);
    match name.split_once('_') {
        Some((id, rest)) if id.len() == 36 && id.matches('-').count() == 4 => rest.to_owned(),
        _ => name.to_owned(),
    }
}

/// Plans a collection into the folder of `flx`. `entry_size` gives the size of an archive entry
/// and `archive_of` the archive an embedded asset is read from.
pub fn plan_collect(
    project: &Project,
    flx: &Path,
    archive_of: impl Fn(&str) -> Option<crate::archive::AssetSource>,
    entry_size: impl Fn(&Path, &str) -> Result<u64>,
) -> Result<CollectPlan> {
    let folder =
        media_folder(flx).ok_or_else(|| ProjectError::Invalid("media.error.unsaved".into()))?;
    let mut plan = CollectPlan {
        folder: folder.clone(),
        jobs: Vec::new(),
        skipped: 0,
    };
    for e in entries(project) {
        match e.link {
            Some(link) if !is_file(&link.path) => plan.skipped += 1,
            Some(link) => {
                let from = PathBuf::from(&link.path);
                if from.parent() == Some(folder.as_path()) {
                    continue;
                }
                let name = from.file_name().map_or_else(
                    || imported_name(e.file),
                    |n| n.to_string_lossy().into_owned(),
                );
                let bytes = fs::metadata(&from).map_or(0, |m| m.len());
                plan.jobs.push(CollectJob {
                    id: e.id.to_owned(),
                    from: CollectFrom::File(from),
                    name,
                    bytes,
                });
            }
            None => {
                let (from, bytes) = match archive_of(e.id) {
                    Some(crate::archive::AssetSource::Archive(archive)) => {
                        let bytes = entry_size(&archive, e.file)?;
                        (
                            CollectFrom::Entry {
                                archive,
                                file: e.file.to_owned(),
                            },
                            bytes,
                        )
                    }
                    Some(crate::archive::AssetSource::Disk(file)) => {
                        let bytes = fs::metadata(&file).map_or(0, |m| m.len());
                        (CollectFrom::File(file), bytes)
                    }
                    None => {
                        plan.skipped += 1;
                        continue;
                    }
                };
                plan.jobs.push(CollectJob {
                    id: e.id.to_owned(),
                    from,
                    name: imported_name(e.file),
                    bytes,
                });
            }
        }
    }
    Ok(plan)
}

impl CollectPlan {
    pub fn total_bytes(&self) -> u64 {
        self.jobs.iter().map(|job| job.bytes).sum()
    }

    /// Copies the files (no session needed, so a long copy blocks nothing). `progress` is told the
    /// bytes done so far. Returns where each media went; a failure removes the file it was writing.
    pub fn run(&self, mut progress: impl FnMut(u64)) -> Result<Vec<(String, PathBuf)>> {
        fs::create_dir_all(&self.folder)?;
        let mut done = 0u64;
        let mut placed = Vec::new();
        for job in &self.jobs {
            let (mut out, dest) = create_free(&self.folder, &job.name)?;
            let copied = match &job.from {
                CollectFrom::File(from) => {
                    fs::File::open(from)
                        .map_err(Into::into)
                        .and_then(|mut input| {
                            copy_counted(&mut input, &mut out, &mut done, &mut progress)
                        })
                }
                CollectFrom::Entry { archive, file } => {
                    crate::archive::copy_entry(archive, file, &mut out, &mut |n| {
                        done += n;
                        progress(done);
                    })
                }
            };
            drop(out);
            if let Err(err) = copied {
                let _ = fs::remove_file(&dest);
                return Err(err);
            }
            placed.push((job.id.clone(), dest));
        }
        Ok(placed)
    }
}

fn copy_counted(
    input: &mut impl std::io::Read,
    out: &mut impl std::io::Write,
    done: &mut u64,
    progress: &mut impl FnMut(u64),
) -> Result<()> {
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        out.write_all(&buffer[..n])?;
        *done += n as u64;
        progress(*done);
    }
}

/// Makes a new file called `name` in `folder`, or `name (2)`, `name (3)` and on when it exists.
fn create_free(folder: &Path, name: &str) -> Result<(fs::File, PathBuf)> {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned());
    let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1u32.. {
        let candidate = match (n, &extension) {
            (1, _) => name.to_owned(),
            (n, Some(ext)) => format!("{stem} ({n}).{ext}"),
            (n, None) => format!("{stem} ({n})"),
        };
        let dest = folder.join(candidate);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(file) => return Ok((file, dest)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err.into()),
        }
    }
    unreachable!("a free name is found before the counter ends")
}

/// Links every media in `placed` to its new file.
pub fn apply_collected(project: &mut Project, placed: &[(String, PathBuf)]) {
    for (id, dest) in placed {
        if let Some(link) = link_mut(project, id) {
            *link = Some(MediaLink {
                path: dest.to_string_lossy().into_owned(),
                relative: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    use crate::error::ProjectError;

    use crate::media::support::*;
    use crate::test_util::write_file;

    use crate::{FrameRate, Session};

    #[test]
    fn collect_should_copy_linked_files_extract_embedded_ones_and_link_them() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project, flx) = linked_show(&dir);
        let wav = write_file(dir.path(), "old.wav", b"embedded bytes");
        project.assets.push(session.import_embedded(&wav).unwrap());
        let saved = session.save(&project, &flx).unwrap();
        fs::remove_file(&wav).unwrap();
        session.update(saved);

        let plan = session.media_collect_plan().unwrap();
        let mut last = 0;
        let placed = plan.run(|done| last = done).unwrap();
        let state = session.media_collect_apply(&placed).unwrap();

        assert_eq!(placed.len(), 3);
        assert_eq!(last, plan.total_bytes());
        let folder = dir.path().join("show").join("show Media");
        assert_eq!(fs::read(folder.join("song.wav")).unwrap(), b"song");
        assert_eq!(fs::read(folder.join("old.wav")).unwrap(), b"embedded bytes");
        assert!(state.project.assets.iter().all(|a| a.link.is_some()));
        let link = state.project.assets[1].link.as_ref().unwrap();
        assert_eq!(link.relative.as_deref(), Some("show Media/old.wav"));
    }

    #[test]
    fn collect_should_need_a_saved_project_and_skip_offline_media() {
        let dir = TempDir::new().unwrap();
        let mut unsaved = Session::default();
        unsaved.new_project("New", FrameRate::Fps25);
        assert!(matches!(
            unsaved.media_collect_plan(),
            Err(ProjectError::Invalid(code)) if code == "media.error.unsaved"
        ));

        let (mut session, project, _) = linked_show(&dir);
        session.update(project);
        fs::remove_file(dir.path().join("media/Clip.mp4")).unwrap();
        let plan = session.media_collect_plan().unwrap();
        assert_eq!((plan.jobs.len(), plan.skipped), (1, 1));
    }

    #[test]
    fn collect_should_let_an_undo_to_the_embedded_asset_save_again() {
        let dir = TempDir::new().unwrap();
        let mut session = Session::default();
        let wav = write_file(dir.path(), "old.wav", b"embedded bytes");
        let mut project = session.new_project("Show", FrameRate::Fps25);
        project.assets.push(session.import_embedded(&wav).unwrap());
        let flx = dir.path().join("show.flx");
        let embedded = session.save(&project, &flx).unwrap();
        session.update(embedded.clone());
        let plan = session.media_collect_plan().unwrap();
        let placed = plan.run(|_| {}).unwrap();
        let collected = session.media_collect_apply(&placed).unwrap();
        session.save(&collected.project, &flx).unwrap();

        // Undo brings back the embedded asset.
        session.update(embedded.clone());
        let again = dir.path().join("again.flx");
        session.save(&embedded, &again).unwrap();

        assert!(entries_of(&again).iter().any(|n| n.starts_with("audio/")));
    }

    #[test]
    fn collect_should_settle_a_name_clash_with_a_number() {
        let dir = TempDir::new().unwrap();
        let (file, path) = create_free(dir.path(), "a.wav").unwrap();
        drop(file);
        let (_, second) = create_free(dir.path(), "a.wav").unwrap();
        assert_eq!(path.file_name().unwrap(), "a.wav");
        assert_eq!(second.file_name().unwrap(), "a (2).wav");
    }
}
