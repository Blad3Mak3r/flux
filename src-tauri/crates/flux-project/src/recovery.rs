//! Small recovery records: project JSON and source paths, never audio or preview data.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AssetSource, Document, FORMAT_VERSION, ProjectError, Result};

pub fn timestamp() -> String {
    crate::meta::format_time(SystemTime::now())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub version: u32,
    pub id: String,
    pub saved_at: String,
    pub document: Document,
    pub sources: HashMap<String, AssetSource>,
}

impl Recovery {
    pub fn new(id: String, document: Document, sources: HashMap<String, AssetSource>) -> Self {
        Self {
            version: 1,
            id,
            saved_at: timestamp(),
            document,
            sources,
        }
    }

    pub fn validate(&self) -> Result<()> {
        check_id(&self.id)?;
        if self.version != 1
            || self.document.project.format_version != FORMAT_VERSION
            || self.document.baseline.format_version != FORMAT_VERSION
        {
            return Err(ProjectError::Invalid("unsupported recovery version".into()));
        }
        self.document.project.validate()?;
        self.document.baseline.validate()?;
        if self.document.project.meta.id != self.document.baseline.meta.id
            || self
                .document
                .project
                .timeline(&self.document.active)
                .is_none()
        {
            return Err(ProjectError::Invalid(
                "inconsistent recovery document".into(),
            ));
        }
        for source in self.sources.values() {
            let (AssetSource::Disk(path) | AssetSource::Archive(path)) = source;
            if !path.is_absolute() {
                return Err(ProjectError::Invalid(
                    "recovery audio sources must be absolute".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub saved_at: Option<String>,
    pub readable: bool,
}

fn check_id(id: &str) -> Result<()> {
    if Uuid::parse_str(id).is_err() {
        return Err(ProjectError::Invalid("invalid recovery id".into()));
    }
    Ok(())
}

fn file(dir: &Path, id: &str) -> Result<PathBuf> {
    check_id(id)?;
    Ok(dir.join(format!("{id}.json")))
}

/// The OS releases this lock even when the process is killed. Other app instances must not
/// offer a live session as crash recovery or restore/discard a record already being recovered.
pub fn claim(dir: &Path, id: &str) -> Result<File> {
    let path = file(dir, id)?.with_extension("lock");
    fs::create_dir_all(dir)?;
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    lock.try_lock().map_err(|err| match err {
        std::fs::TryLockError::WouldBlock => {
            ProjectError::Invalid("recovery is in use by another session".into())
        }
        std::fs::TryLockError::Error(err) => ProjectError::Io(err),
    })?;
    Ok(lock)
}

pub fn write(dir: &Path, recovery: &Recovery) -> Result<()> {
    recovery.validate()?;
    fs::create_dir_all(dir)?;
    let path = file(dir, &recovery.id)?;
    let tmp = path.with_extension("json.tmp");
    let result = (|| {
        let mut writer = BufWriter::new(File::create(&tmp)?);
        serde_json::to_writer(&mut writer, recovery)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

pub fn read(dir: &Path, id: &str) -> Result<Recovery> {
    let recovery: Recovery = serde_json::from_reader(BufReader::new(File::open(file(dir, id)?)?))?;
    recovery.validate()?;
    if recovery.id != id {
        return Err(ProjectError::Invalid("recovery id mismatch".into()));
    }
    Ok(recovery)
}

pub fn remove(dir: &Path, id: &str) -> Result<()> {
    match fs::remove_file(file(dir, id)?) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

pub fn list(dir: &Path) -> Result<Vec<Summary>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut summaries = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        if check_id(id).is_err() {
            continue;
        }
        let Ok(_lease) = claim(dir, id) else {
            continue;
        };
        let record = read(dir, id).ok();
        summaries.push(Summary {
            id: id.to_owned(),
            name: record.as_ref().map(|r| r.document.project.name.clone()),
            path: record.as_ref().and_then(|r| r.document.path.clone()),
            saved_at: record.as_ref().map(|r| r.saved_at.clone()),
            readable: record.is_some(),
        });
    }
    summaries.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then_with(|| a.id.cmp(&b.id)));
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FrameRate, Session};

    #[test]
    fn recovery_should_roundtrip_find_and_delete_without_reading_audio() {
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("large.wav");
        File::create(&audio)
            .unwrap()
            .set_len(64 * 1024 * 1024)
            .unwrap();
        let original = dir.path().join("original.fxtc");
        fs::write(&original, b"original archive untouched").unwrap();
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        let asset = session.import_audio(&audio).unwrap();
        let mut record = session.recovery().unwrap();
        record.document.project.assets.push(asset);
        record.document.path = Some(original.clone());
        let recovery_dir = dir.path().join("recovery");
        write(&recovery_dir, &record).unwrap();
        let bytes = fs::read(file(&recovery_dir, &record.id).unwrap()).unwrap();
        assert!(bytes.len() < 10_000);
        assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
        assert_eq!(fs::read(&original).unwrap(), b"original archive untouched");
        fs::remove_file(audio).unwrap();
        assert_eq!(read(&recovery_dir, &record.id).unwrap(), record);
        assert_eq!(list(&recovery_dir).unwrap().len(), 1);
        let mut restored = Session::default();
        assert_eq!(
            restored.restore_recovery(record.clone()).unwrap(),
            record.document
        );
        assert!(
            restored
                .asset_bytes(&record.document.project.assets[0])
                .is_err()
        );
        remove(&recovery_dir, &record.id).unwrap();
        assert!(list(&recovery_dir).unwrap().is_empty());
    }

    #[test]
    fn recovery_should_list_corrupt_and_incompatible_records_without_blocking_startup() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        fs::write(file(dir.path(), &id).unwrap(), b"broken").unwrap();
        assert!(!list(dir.path()).unwrap()[0].readable);
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        let mut record = session.recovery().unwrap();
        record.version = 99;
        assert!(write(dir.path(), &record).is_err());
        assert!(read(dir.path(), "../project").is_err());
        remove(dir.path(), &id).unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn recovery_should_not_offer_a_live_session_and_should_become_available_after_release() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::default();
        session.new_project("Live session", FrameRate::Fps25);
        let record = session.recovery().unwrap();
        let lease = claim(dir.path(), &record.id).unwrap();
        write(dir.path(), &record).unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
        assert!(claim(dir.path(), &record.id).is_err());
        drop(lease);
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }

    #[test]
    fn recovery_should_restore_archive_and_import_sources_even_with_missing_audio() {
        let dir = tempfile::tempdir().unwrap();
        let archive_audio = dir.path().join("bundled.wav");
        let imported_audio = dir.path().join("imported.wav");
        fs::write(&archive_audio, b"bundled audio").unwrap();
        fs::write(&imported_audio, b"imported audio").unwrap();
        let mut session = Session::default();
        let mut project = session.new_project("Show", FrameRate::Fps25);
        project
            .assets
            .push(session.import_audio(&archive_audio).unwrap());
        let original = dir.path().join("show.fxtc");
        let mut project = session.save(&project, &original).unwrap();
        let original_bytes = fs::read(&original).unwrap();
        project
            .assets
            .push(session.import_audio(&imported_audio).unwrap());
        project.name = "Recovered show".into();
        project.autosave = crate::Autosave {
            enabled: false,
            interval_seconds: 60,
        };
        session.update(project.clone());
        let record = session.recovery().unwrap();
        write(dir.path(), &record).unwrap();
        fs::remove_file(archive_audio).unwrap();
        fs::remove_file(imported_audio).unwrap();
        let mut restored = Session::default();
        let document = restored
            .restore_recovery(read(dir.path(), &record.id).unwrap())
            .unwrap();
        assert_eq!(document.project, project);
        assert_ne!(document.project, document.baseline);
        assert_eq!(
            restored.asset_bytes(&project.assets[0]).unwrap(),
            b"bundled audio"
        );
        assert!(restored.asset_bytes(&project.assets[1]).is_err());
        assert_eq!(fs::read(original).unwrap(), original_bytes);
    }

    #[test]
    fn atomic_replacement_should_preserve_the_previous_record_if_validation_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        let mut record = session.recovery().unwrap();
        write(dir.path(), &record).unwrap();
        record.document.project.name = "Second edit".into();
        write(dir.path(), &record).unwrap();
        let previous = read(dir.path(), &record.id).unwrap();
        record.document.project.autosave.interval_seconds = 9;
        assert!(write(dir.path(), &record).is_err());
        assert_eq!(read(dir.path(), &record.id).unwrap(), previous);
        assert!(
            !file(dir.path(), &record.id)
                .unwrap()
                .with_extension("json.tmp")
                .exists()
        );
    }
}
