//! Media kept outside the archive. A video can weigh several GB, so a project refers to its media by
//! path (see [`MediaLink`]) and a file that is gone is *offline*: shown as such, never an error.
//! The functions here are pure edits of a [`Project`]; the session wires them to its sources.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::error::{ProjectError, Result};
use crate::model::{AUDIO_DIR, MediaLink, Project, TrackKind, VIDEO_DIR};

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

/// Every media file of the project, the offline ones first and then by name.
pub fn list(project: &Project) -> MediaList {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for track in project.timelines.iter().flat_map(|t| &t.tracks) {
        match &track.kind {
            TrackKind::Audio { clips } => {
                for clip in clips {
                    *counts.entry(&clip.asset).or_default() += 1;
                }
            }
            TrackKind::Video { clips } => {
                for clip in clips {
                    *counts.entry(&clip.asset).or_default() += 1;
                }
            }
            _ => {}
        }
    }
    for clip in project.vj.layers.iter().flat_map(|layer| &layer.clips) {
        *counts.entry(&clip.asset).or_default() += 1;
    }
    for slice in project.output.screens.iter().flat_map(|s| &s.slices) {
        *counts.entry(&slice.test_card.logo).or_default() += 1;
    }
    let mut items: Vec<MediaItem> = entries(project)
        .into_iter()
        .map(|e| MediaItem {
            id: e.id.to_owned(),
            name: e.name.to_owned(),
            kind: e.kind(),
            state: match e.link {
                None => MediaState::Embedded,
                Some(_) if e.offline() => MediaState::Offline,
                Some(_) => MediaState::Online,
            },
            path: e.link.map(|link| link.path.clone()),
            uses: counts.get(e.id).copied().unwrap_or(0),
        })
        .collect();
    items.sort_by_cached_key(|item| {
        (
            item.state != MediaState::Offline,
            item.name.to_lowercase(),
            item.id.clone(),
        )
    });
    let missing = items
        .iter()
        .filter(|item| item.state == MediaState::Offline)
        .count() as u32;
    MediaList { items, missing }
}

/// Ids of the offline media, for the code that draws or plays them.
pub fn offline_ids(project: &Project) -> Vec<String> {
    entries(project)
        .into_iter()
        .filter(Entry::offline)
        .map(|e| e.id.to_owned())
        .collect()
}

/// Points every linked asset whose file moved to where the project file's own folder has it
/// (the `relative` path); what is still not found stays offline. Returns whether anything changed.
pub fn resolve(project: &mut Project, flx: &Path) -> bool {
    let Some(base) = flx.parent() else {
        return false;
    };
    let mut changed = false;
    let moved: Vec<(String, String)> = entries(project)
        .into_iter()
        .filter(Entry::offline)
        .filter_map(|e| {
            let relative = e.link?.relative.as_deref()?;
            let found = base.join(relative);
            found
                .is_file()
                .then(|| (e.id.to_owned(), found.to_string_lossy().into_owned()))
        })
        .collect();
    for (id, path) in moved {
        if let Some(Some(link)) = link_mut(project, &id) {
            link.path = path;
            changed = true;
        }
    }
    changed
}

/// Writes the `relative` path of every link against the folder of the project file `flx`; `None`
/// when it cannot be said (another drive).
pub fn relativize(project: &mut Project, flx: &Path) {
    let base = flx.parent();
    let links = project
        .assets
        .iter_mut()
        .map(|a| &mut a.link)
        .chain(project.videos.iter_mut().map(|v| &mut v.link));
    for link in links.flatten() {
        link.relative = base.and_then(|base| relative_to(base, Path::new(&link.path)));
    }
}

fn relative_to(base: &Path, target: &Path) -> Option<String> {
    let (base, target): (Vec<_>, Vec<_>) =
        (base.components().collect(), target.components().collect());
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0
        || !target
            .first()
            .is_some_and(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
    {
        return None;
    }
    let mut parts: Vec<String> = base[common..].iter().map(|_| "..".to_owned()).collect();
    for part in &target[common..] {
        parts.push(part.as_os_str().to_str()?.to_owned());
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

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
    use std::io::Read;

    use tempfile::TempDir;

    use super::*;
    use crate::archive;
    use crate::{FrameRate, ProjectMode, Session};

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }

    /// A saved show in `dir/show/show.flx` with a linked song and a linked video in `dir/media`.
    fn linked_show(dir: &TempDir) -> (Session, Project, PathBuf) {
        let song = write(&dir.path().join("media"), "song.wav", b"song");
        let clip = write(&dir.path().join("media"), "Clip.mp4", b"clip");
        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        project.assets.push(session.import_audio(&song).unwrap());
        session.update(project.clone());
        let state = session.import_video(&clip, 0).unwrap();
        let flx = dir.path().join("show").join("show.flx");
        fs::create_dir_all(flx.parent().unwrap()).unwrap();
        let saved = session.save(&state.project, &flx).unwrap();
        (session, saved, flx)
    }

    fn entries_of(flx: &Path) -> Vec<String> {
        let zip = zip::ZipArchive::new(fs::File::open(flx).unwrap()).unwrap();
        zip.file_names().flatten().map(|n| n.into_owned()).collect()
    }

    #[test]
    fn save_should_write_no_media_for_a_linked_asset_and_keep_a_relative_path() {
        let dir = TempDir::new().unwrap();
        let (_, saved, flx) = linked_show(&dir);

        let names = entries_of(&flx);
        assert!(
            !names
                .iter()
                .any(|n| n.starts_with("audio/") || n.starts_with("video/"))
        );
        assert_eq!(
            saved.assets[0].link.as_ref().unwrap().relative.as_deref(),
            Some("../media/song.wav")
        );
    }

    #[test]
    fn open_should_find_media_that_moved_with_the_project_by_its_relative_path() {
        let dir = TempDir::new().unwrap();
        let (_, _, flx) = linked_show(&dir);
        let moved = dir.path().join("moved");
        fs::create_dir_all(&moved).unwrap();
        fs::rename(dir.path().join("show"), moved.join("show")).unwrap();
        fs::rename(dir.path().join("media"), moved.join("media")).unwrap();
        let flx = moved.join("show").join(flx.file_name().unwrap());

        let mut session = Session::default();
        let project = session.open(&flx).unwrap();

        assert_eq!(list(&project).missing, 0);
        assert!(
            project.assets[0]
                .link
                .as_ref()
                .unwrap()
                .path
                .contains("moved")
        );
        let document = session.document().unwrap();
        assert_eq!(document.project, document.baseline);
    }

    #[test]
    fn open_should_not_fail_for_media_that_is_gone_and_list_it_offline_first() {
        let dir = TempDir::new().unwrap();
        let (_, _, flx) = linked_show(&dir);
        fs::remove_dir_all(dir.path().join("media")).unwrap();

        let project = Session::default().open(&flx).unwrap();
        let found = list(&project);

        assert_eq!(found.missing, 2);
        assert!(found.items.iter().all(|i| i.state == MediaState::Offline));
        assert_eq!(found.items[0].name, "Clip");
        assert_eq!(found.items[0].uses, 1);
        assert_eq!(offline_ids(&project).len(), 2);
    }

    #[test]
    fn a_session_should_fail_to_read_offline_media_without_failing_anything_else() {
        let dir = TempDir::new().unwrap();
        let (_, _, flx) = linked_show(&dir);
        fs::remove_dir_all(dir.path().join("media")).unwrap();
        let mut session = Session::default();
        let project = session.open(&flx).unwrap();

        assert!(matches!(
            session.asset_bytes(&project.assets[0]),
            Err(ProjectError::MissingAsset(_))
        ));
        assert!(
            session
                .video_path(&project.videos[0], &dir.path().join("cache"))
                .is_err()
        );
        assert_eq!(session.preview(&project.assets[0]).unwrap(), None);
        assert!(session.save(&project, &flx).is_ok());
    }

    #[test]
    fn a_legacy_project_with_embedded_media_should_open_and_save_embedded() {
        let dir = TempDir::new().unwrap();
        let wav = write(dir.path(), "click.wav", b"embedded bytes");
        let mut session = Session::default();
        let mut project = session.new_project("Old", FrameRate::Fps25);
        project.assets.push(session.import_embedded(&wav).unwrap());
        let flx = dir.path().join("old.fxtc");
        session.save(&project, &flx).unwrap();
        fs::remove_file(&wav).unwrap();

        let mut reopened = Session::default();
        let opened = reopened.open(&flx).unwrap();
        assert_eq!(list(&opened).items[0].state, MediaState::Embedded);
        let again = dir.path().join("again.flx");
        reopened.save(&opened, &again).unwrap();

        assert!(entries_of(&again).iter().any(|n| n.starts_with("audio/")));
        assert_eq!(
            reopened.asset_bytes(&opened.assets[0]).unwrap(),
            b"embedded bytes"
        );
    }

    #[test]
    fn relink_should_also_fix_the_offline_siblings_of_the_same_folder() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, _) = linked_show(&dir);
        session.update(project);
        fs::rename(dir.path().join("media"), dir.path().join("old")).unwrap();
        write(&dir.path().join("new"), "song.wav", b"song");
        write(&dir.path().join("new"), "clip.MP4", b"clip");
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
        let image = write(dir.path(), "logo.png", b"png");

        let result = session.media_relink(&project.assets[0].id, &image);

        assert!(matches!(result, Err(ProjectError::Invalid(code)) if code == "media.error.kind"));
    }

    #[test]
    fn search_folder_should_relink_by_name_in_subfolders_and_report_the_rest() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, _) = linked_show(&dir);
        session.update(project);
        fs::remove_dir_all(dir.path().join("media")).unwrap();
        write(&dir.path().join("drive/a/b"), "SONG.wav", b"song");
        write(&dir.path().join("drive/.hidden"), "Clip.mp4", b"clip");

        let (state, found, left) = session.media_search(&dir.path().join("drive")).unwrap();

        assert_eq!((found, left), (1, 1));
        assert_eq!(list(&state.project).missing, 1);
    }

    #[test]
    fn collect_should_copy_linked_files_extract_embedded_ones_and_link_them() {
        let dir = TempDir::new().unwrap();
        let (mut session, mut project, flx) = linked_show(&dir);
        let wav = write(dir.path(), "old.wav", b"embedded bytes");
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
        let wav = write(dir.path(), "old.wav", b"embedded bytes");
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

    #[test]
    fn a_vj_file_should_keep_the_link_of_its_videos() {
        let dir = TempDir::new().unwrap();
        let clip = write(dir.path(), "loop.mp4", b"loop");
        let mut session = Session::default();
        let grid = session
            .new_project_of("Show", FrameRate::Fps25, ProjectMode::Vj)
            .vj;
        let (layer, column) = (&grid.layers[0].id, &grid.decks[0].columns[0].id);
        let state = session.import_vj_video(&clip, layer, Some(column)).unwrap();
        let flx = dir.path().join("vj.flx");
        session.save(&state.project, &flx).unwrap();

        let (opened, mut zip) = archive::open(&flx).unwrap();

        let link = opened.videos[0].link.as_ref().unwrap();
        assert_eq!(link.path, clip.to_string_lossy());
        assert_eq!(link.relative.as_deref(), Some("loop.mp4"));
        assert!(zip.video_reader(&opened.videos[0].file).is_err());
        let mut json = String::new();
        zip::ZipArchive::new(fs::File::open(&flx).unwrap())
            .unwrap()
            .by_name("project.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        assert!(json.contains("\"link\""));
    }

    #[test]
    fn a_linked_audio_should_keep_its_preview_through_a_save() {
        let dir = TempDir::new().unwrap();
        let (mut session, project, flx) = linked_show(&dir);
        let asset = project.assets[0].clone();
        let stamp = session.stamp_of(&asset, None).unwrap();
        let preview = crate::Preview {
            duration_seconds: 1.5,
            source_rate: 48_000,
            peaks_per_second: 100,
            peaks: vec![-3, 3],
        };
        session.remember_preview(&asset, stamp, preview.clone());
        session.save(&project, &flx).unwrap();
        session
            .save(&project, &dir.path().join("show").join("copy.flx"))
            .unwrap();

        assert_eq!(session.preview(&asset).unwrap(), Some(preview));
    }
}
