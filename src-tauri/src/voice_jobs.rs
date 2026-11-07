//! Durable speech results. A failed attachment must never require a second billable request.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    #[serde(default)]
    pub synthesis_text: Option<String>,
    pub project: String,
    pub timeline: String,
    pub order: String,
    pub dialogue: String,
    pub voice: String,
    pub previous_asset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredJob {
    #[serde(default)]
    pub origin: flux_project::operator::RecordingOrigin,
    #[serde(default = "default_extension")]
    pub extension: String,
    #[serde(default)]
    pub voice_name: Option<String>,
    version: u8,
    pub id: String,
    pub snapshot: Snapshot,
    pub session_id: String,
    pub samples: u64,
    pub created_millis: u64,
}
fn default_extension() -> String {
    "mp3".into()
}

pub(crate) struct Recording<'a> {
    pub bytes: &'a [u8],
    pub samples: u64,
    pub extension: &'a str,
    pub origin: flux_project::operator::RecordingOrigin,
    pub voice_name: Option<String>,
}

pub(crate) struct JobStore {
    root: PathBuf,
}

impl JobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn directory(&self, id: &str) -> io::Result<PathBuf> {
        if !id.starts_with("job-")
            || id.len() > 80
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid speech job id",
            ));
        }
        Ok(self.root.join(id))
    }

    fn write_record(directory: &Path, record: &StoredJob) -> io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut file, record)?;
        file.as_file().sync_all()?;
        file.persist(directory.join("record.json"))
            .map_err(|e| e.error)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn save(
        &self,
        snapshot: Snapshot,
        session_id: String,
        bytes: &[u8],
        samples: u64,
    ) -> io::Result<StoredJob> {
        self.save_recording(
            snapshot,
            session_id,
            Recording {
                bytes,
                samples,
                extension: "mp3",
                origin: Default::default(),
                voice_name: None,
            },
        )
    }

    pub fn save_recording(
        &self,
        snapshot: Snapshot,
        session_id: String,
        recording: Recording<'_>,
    ) -> io::Result<StoredJob> {
        if !["mp3", "wav", "aif", "aiff", "flac", "ogg"].contains(&recording.extension) {
            return Err(io::Error::other("unsupported recording format"));
        }
        fs::create_dir_all(&self.root)?;
        let directory = tempfile::Builder::new()
            .prefix("job-")
            .tempdir_in(&self.root)?;
        let id = directory
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::other("invalid speech directory"))?
            .to_owned();
        let mut audio = fs::File::create(
            directory
                .path()
                .join(format!("audio.{}", recording.extension)),
        )?;
        audio.write_all(recording.bytes)?;
        audio.sync_all()?;
        drop(audio);
        let record = StoredJob {
            version: 1,
            id,
            snapshot,
            session_id,
            samples: recording.samples,
            extension: recording.extension.into(),
            origin: recording.origin,
            voice_name: recording.voice_name,
            created_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        Self::write_record(directory.path(), &record)?;
        let _ = directory.keep();
        Ok(record)
    }

    pub fn read(&self, id: &str) -> io::Result<StoredJob> {
        let directory = self.directory(id)?;
        let mut bytes = Vec::new();
        fs::File::open(directory.join("record.json"))?
            .take(128 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 128 * 1024 {
            return Err(io::Error::other("oversized speech record"));
        }
        let record: StoredJob = serde_json::from_slice(&bytes)?;
        let valid = record.version == 1
            && record.created_millis <= 253_402_300_799_999
            && record
                .voice_name
                .as_ref()
                .is_none_or(|s| s.chars().count() <= 512)
            && record
                .snapshot
                .synthesis_text
                .as_ref()
                .is_none_or(|s| s.chars().count() <= flux_project::operator::MAX_DIALOGUE)
            && record.id == id
            && record.samples > 0
            && record.samples <= u64::from(flux_core::ENGINE_RATE) * 3600
            && record.snapshot.dialogue.chars().count() <= flux_project::operator::MAX_DIALOGUE
            && [
                &record.snapshot.project,
                &record.snapshot.timeline,
                &record.snapshot.order,
            ]
            .iter()
            .all(|s| !s.is_empty() && s.len() <= 128)
            && flux_project::operator::validate_voice(Some(&record.snapshot.voice)).is_ok();
        if !valid
            || !["mp3", "wav", "aif", "aiff", "flac", "ogg"].contains(&record.extension.as_str())
            || !directory
                .join(format!("audio.{}", record.extension))
                .is_file()
        {
            return Err(io::Error::other("invalid speech record"));
        }
        Ok(record)
    }

    pub fn audio(&self, id: &str) -> io::Result<PathBuf> {
        let record = self.read(id)?;
        Ok(self
            .directory(id)?
            .join(format!("audio.{}", record.extension)))
    }

    pub fn list(&self, project: &str) -> io::Result<Vec<StoredJob>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut jobs = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Ok(job) = self.read(&id)
                && job.snapshot.project == project
            {
                jobs.push(job);
            }
        }
        jobs.sort_by_key(|j| j.created_millis);
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            synthesis_text: None,
            project: "show".into(),
            timeline: "timeline".into(),
            order: "order".into(),
            dialogue: "Go".into(),
            voice: "voice".into(),
            previous_asset: None,
        }
    }

    #[test]
    fn result_survives_restart_and_remains_available_for_recovery_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path().to_owned());
        let job = store
            .save(snapshot(), "session".into(), b"generated audio", 48_000)
            .unwrap();
        drop(store);
        let reopened = JobStore::new(dir.path().to_owned());
        assert_eq!(reopened.list("show").unwrap()[0].id, job.id);
        assert_eq!(
            fs::read(reopened.audio(&job.id).unwrap()).unwrap(),
            b"generated audio"
        );
        assert!(reopened.audio("../bad").is_err());
        assert_eq!(reopened.list("show").unwrap().len(), 1);
        assert!(
            reopened.audio(&job.id).unwrap().exists(),
            "undo still needs the audio"
        );
    }

    #[test]
    fn incomplete_or_corrupt_jobs_do_not_hide_other_results() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path().to_owned());
        let job = store
            .save(snapshot(), "session".into(), b"audio", 1)
            .unwrap();
        fs::create_dir(dir.path().join("job-incomplete")).unwrap();
        assert_eq!(store.list("show").unwrap().len(), 1);
        fs::write(
            store.directory(&job.id).unwrap().join("record.json"),
            b"broken",
        )
        .unwrap();
        assert!(store.read(&job.id).is_err());
        assert!(store.list("show").unwrap().is_empty());
        assert!(store.list("other-show").unwrap().is_empty());
    }

    #[test]
    fn original_jobs_remain_readable_and_imported_formats_cannot_escape_directory() {
        let dir = tempfile::tempdir().unwrap();
        let store = JobStore::new(dir.path().to_owned());
        let job = store
            .save(snapshot(), "session".into(), b"audio", 1)
            .unwrap();
        let path = store.directory(&job.id).unwrap().join("record.json");
        let mut old = serde_json::to_value(&job).unwrap();
        for key in ["extension", "origin", "voiceName"] {
            old.as_object_mut().unwrap().remove(key);
        }
        old["snapshot"]
            .as_object_mut()
            .unwrap()
            .remove("synthesisText");
        fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(store.read(&job.id).unwrap().extension, "mp3");
        old["extension"] = "../../outside".into();
        fs::write(path, serde_json::to_vec(&old).unwrap()).unwrap();
        assert!(store.read(&job.id).is_err());
    }
}
