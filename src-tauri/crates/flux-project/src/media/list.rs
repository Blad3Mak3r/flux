//! Listing a project's media and finding what is offline.

use std::collections::HashMap;
use std::path::{Component, Path};

use super::*;
use crate::model::{Project, TrackKind};

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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;

    use tempfile::TempDir;

    use super::*;
    use crate::archive;
    use crate::error::ProjectError;

    use crate::media::support::*;
    use crate::test_util::write_file;

    use crate::{FrameRate, ProjectMode, Session};

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
        let wav = write_file(dir.path(), "click.wav", b"embedded bytes");
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
    fn a_vj_file_should_keep_the_link_of_its_videos() {
        let dir = TempDir::new().unwrap();
        let clip = write_file(dir.path(), "loop.mp4", b"loop");
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
