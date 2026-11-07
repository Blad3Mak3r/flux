//! Recovery I/O has its own worker and gate, never an engine or audio lane.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flux_project::{Document, ProjectError, Result, Session, recovery};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::SharedSession;

pub struct Recoveries {
    pub dir: PathBuf,
    // Lifecycle operations and writes take this before the session lock. Edits do not take it.
    gate: Mutex<Option<(String, std::fs::File)>>,
}

pub type SharedRecoveries = Arc<Recoveries>;

#[derive(Default)]
struct Schedule {
    observed: Option<recovery::Recovery>,
    changed_at: Option<Instant>,
    written_at: Option<Instant>,
    pending: bool,
    retry_at: Option<Instant>,
}

impl Schedule {
    fn observe(&mut self, record: &recovery::Recovery, now: Instant) -> bool {
        let switched = self.observed.as_ref().is_none_or(|old| old.id != record.id);
        if switched {
            *self = Self::default();
            self.written_at = Some(now);
        }
        let changed = self
            .observed
            .as_ref()
            .is_none_or(|old| old.document != record.document || old.sources != record.sources);
        if changed {
            self.observed = Some(record.clone());
            self.changed_at = Some(now);
            self.pending = true;
        }
        let prefs = record.document.project.autosave;
        self.pending
            && prefs.enabled
            && self.retry_at.is_none_or(|at| now >= at)
            && record.document.project != record.document.baseline
            && (self
                .changed_at
                .is_some_and(|at| now.duration_since(at) >= Duration::from_secs(2))
                || self.written_at.is_some_and(|at| {
                    now.duration_since(at) >= Duration::from_secs(prefs.interval_seconds)
                }))
    }

    fn written(&mut self, now: Instant) {
        self.written_at = Some(now);
        self.pending = false;
        self.retry_at = None;
    }
}

#[derive(Default)]
struct Worker {
    schedule: Schedule,
    current: Option<recovery::Recovery>,
    revision: (String, u64),
}

impl Worker {
    fn poll(&mut self, store: &Recoveries, session: &SharedSession, now: Instant) -> Result<bool> {
        let mut lease = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let (id, number) = session.recovery_revision();
            if self.revision.0 != id || self.revision.1 != number {
                self.current = session.recovery();
                self.revision = (id.to_owned(), number);
            }
        }
        let Some(record) = self.current.as_mut() else {
            self.schedule = Schedule::default();
            return Ok(false);
        };
        let due = self.schedule.observe(record, now);
        if record.document.project == record.document.baseline && self.schedule.pending {
            recovery::remove(&store.dir, &record.id)?;
            self.schedule.written(now);
        }
        if !due {
            return Ok(false);
        }
        record.saved_at = recovery::timestamp();
        let written = (|| {
            if lease.as_ref().is_none_or(|(id, _)| id != &record.id) {
                *lease = Some((record.id.clone(), recovery::claim(&store.dir, &record.id)?));
            }
            recovery::write(&store.dir, record)
        })();
        if let Err(err) = written {
            self.schedule.retry_at =
                Some(now + Duration::from_secs(record.document.project.autosave.interval_seconds));
            return Err(err);
        }
        self.schedule.written(now);
        Ok(true)
    }
}

pub fn start(app: &AppHandle) -> Result<()> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| ProjectError::Io(io::Error::other(err)))?
        .join("recovery");
    let store = Arc::new(Recoveries {
        dir,
        gate: Mutex::new(None),
    });
    app.manage(Arc::clone(&store));
    let session = Arc::downgrade(&app.state::<SharedSession>());
    let store = Arc::downgrade(&store);
    let app = app.clone();
    std::thread::Builder::new()
        .name("flux-recovery".into())
        .spawn(move || {
            let mut worker = Worker::default();
            let mut failed = false;
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let (Some(store), Some(session)) = (store.upgrade(), session.upgrade()) else {
                    break;
                };
                match worker.poll(&store, &session, Instant::now()) {
                    Ok(true) => failed = false,
                    Ok(false) => {}
                    Err(err) => {
                        if !failed {
                            log::warn!("autosave failed: {err}");
                            let _ = app.emit("recovery-error", ());
                        }
                        failed = true;
                    }
                }
            }
        })?;
    Ok(())
}

/// A successful save/replacement discards only the previous session's recovery. A failed operation
/// leaves it intact. The gate prevents a worker snapshot from resurrecting it afterwards.
pub async fn transition<T, F>(session: &SharedSession, store: &SharedRecoveries, op: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Session) -> Result<T> + Send + 'static,
{
    let session = Arc::clone(session);
    let store = Arc::clone(store);
    tauri::async_runtime::spawn_blocking(move || {
        let mut lease = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = session.recovery().map(|r| r.id);
        let result = op(&mut session)?;
        if let Some(id) = previous
            && let Err(err) = recovery::remove(&store.dir, &id)
        {
            log::warn!("recovery cleanup failed: {err}");
        }
        *lease = None;
        Ok(result)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

#[tauri::command]
pub async fn recovery_list(store: State<'_, SharedRecoveries>) -> Result<Vec<recovery::Summary>> {
    let store = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        recovery::list(&store.dir)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

#[tauri::command]
pub async fn recovery_discard(store: State<'_, SharedRecoveries>, id: String) -> Result<()> {
    let store = Arc::clone(&store);
    tauri::async_runtime::spawn_blocking(move || {
        let _gate = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let _lease = recovery::claim(&store.dir, &id)?;
        recovery::remove(&store.dir, &id)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

#[tauri::command]
pub async fn project_close(
    session: State<'_, SharedSession>,
    store: State<'_, SharedRecoveries>,
) -> Result<()> {
    transition(&session, &store, |session| {
        session.clear();
        Ok(())
    })
    .await
}

pub async fn restore(
    session: &SharedSession,
    store: &SharedRecoveries,
    id: String,
) -> Result<Document> {
    let dir = store.dir.clone();
    // Restoring keeps the record until an explicit save/discard, including a second crash.
    let session = Arc::clone(session);
    let store = Arc::clone(store);
    tauri::async_runtime::spawn_blocking(move || {
        let mut lease = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let claimed = recovery::claim(&dir, &id)?;
        let record = recovery::read(&dir, &id)?;
        let restored = session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .restore_recovery(record)?;
        *lease = Some((id, claimed));
        Ok(restored)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?
}

/// Undo a failed editor-window creation without discarding the recovery we just installed.
pub async fn abandon(session: &SharedSession, store: &SharedRecoveries) -> Result<()> {
    let session = Arc::clone(session);
    let store = Arc::clone(store);
    tauri::async_runtime::spawn_blocking(move || {
        let mut lease = store.gate.lock().unwrap_or_else(PoisonError::into_inner);
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        *lease = None;
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::FrameRate;

    #[test]
    #[ignore = "subprocess fixture for killed_process_should_leave_a_restorable_edit"]
    fn crash_worker_probe() {
        let Ok(dir) = std::env::var("FLUX_RECOVERY_TEST_DIR") else {
            return;
        };
        let store = Recoveries {
            dir: dir.into(),
            gate: Mutex::new(None),
        };
        let session = SharedSession::default();
        let mut project = session
            .lock()
            .unwrap()
            .new_project("Before crash", FrameRate::Fps25);
        project.name = "Edit before process kill".into();
        session.lock().unwrap().update(project);
        let mut worker = Worker::default();
        let start = Instant::now();
        worker.poll(&store, &session, start).unwrap();
        assert!(
            worker
                .poll(&store, &session, start + Duration::from_secs(2))
                .unwrap()
        );
        std::fs::write(store.dir.join("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }

    #[test]
    fn killed_process_should_leave_a_restorable_edit() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery::tests::crash_worker_probe",
                "--ignored",
            ])
            .env("FLUX_RECOVERY_TEST_DIR", dir.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.path().join("ready").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let ready = dir.path().join("ready").exists();
        let active = recovery::list(dir.path()).unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(
            ready,
            "child failed to write its recovery before the deadline"
        );
        assert!(
            active.is_empty(),
            "live sessions must not be offered as recovery"
        );
        let entries = recovery::list(dir.path()).unwrap();
        assert_eq!(entries.len(), 1);
        let record = recovery::read(dir.path(), &entries[0].id).unwrap();
        let mut session = Session::default();
        let document = session.restore_recovery(record).unwrap();
        assert_eq!(document.project.name, "Edit before process kill");
        assert_eq!(document.baseline.name, "Before crash");
    }

    #[test]
    fn a_failed_write_should_retry_at_the_project_interval() {
        let (dir, _store, session) = setup();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        let store = Recoveries {
            dir: blocked.clone(),
            gate: Mutex::new(None),
        };
        let mut worker = Worker::default();
        let start = Instant::now();
        assert!(!worker.poll(&store, &session, start).unwrap());
        assert!(
            worker
                .poll(&store, &session, start + Duration::from_secs(2))
                .is_err()
        );
        assert!(
            !worker
                .poll(&store, &session, start + Duration::from_secs(3))
                .unwrap()
        );
        std::fs::remove_file(blocked).unwrap();
        assert!(
            worker
                .poll(&store, &session, start + Duration::from_secs(32))
                .unwrap()
        );
        assert!(
            !worker
                .poll(&store, &session, start + Duration::from_secs(60))
                .unwrap()
        );
    }

    #[test]
    fn failed_window_creation_should_release_the_session_without_discarding_recovery() {
        tauri::async_runtime::block_on(async {
            let (_dir, store, session) = setup();
            let record = session.lock().unwrap().recovery().unwrap();
            recovery::write(&store.dir, &record).unwrap();
            session.lock().unwrap().clear();
            restore(&session, &store, record.id.clone()).await.unwrap();
            assert!(recovery::list(&store.dir).unwrap().is_empty());
            abandon(&session, &store).await.unwrap();
            assert!(session.lock().unwrap().document().is_none());
            assert_eq!(recovery::list(&store.dir).unwrap().len(), 1);
            assert!(recovery::read(&store.dir, &record.id).is_ok());
        });
    }

    fn setup() -> (tempfile::TempDir, SharedRecoveries, SharedSession) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Recoveries {
            dir: dir.path().into(),
            gate: Mutex::new(None),
        });
        let session = SharedSession::default();
        let mut project = session
            .lock()
            .unwrap()
            .new_project("Show", FrameRate::Fps25);
        project.name = "Edited show".into();
        session.lock().unwrap().update(project);
        (dir, store, session)
    }

    #[test]
    fn worker_should_keep_disabled_recovery_and_not_recreate_it_after_close() {
        tauri::async_runtime::block_on(async {
            let (_dir, store, session) = setup();
            let id = session.lock().unwrap().recovery().unwrap().id;
            let start = Instant::now();
            let mut worker = Worker::default();
            assert!(!worker.poll(&store, &session, start).unwrap());
            assert!(
                worker
                    .poll(&store, &session, start + Duration::from_secs(2))
                    .unwrap()
            );
            let mut project = session.lock().unwrap().document().unwrap().project;
            project.autosave.enabled = false;
            session.lock().unwrap().update(project);
            assert!(
                !worker
                    .poll(&store, &session, start + Duration::from_secs(60))
                    .unwrap()
            );
            assert!(recovery::read(&store.dir, &id).is_ok());
            transition(&session, &store, |s| {
                s.clear();
                Ok(())
            })
            .await
            .unwrap();
            assert!(
                !worker
                    .poll(&store, &session, start + Duration::from_secs(120))
                    .unwrap()
            );
            assert!(recovery::read(&store.dir, &id).is_err());
        });
    }

    #[test]
    fn close_racing_a_due_worker_should_leave_no_recovery() {
        let (_dir, store, session) = setup();
        let start = Instant::now();
        let mut worker = Worker::default();
        worker.poll(&store, &session, start).unwrap();
        let id = session.lock().unwrap().recovery().unwrap().id;
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let writer = {
            let (store, session, barrier) = (
                Arc::clone(&store),
                Arc::clone(&session),
                Arc::clone(&barrier),
            );
            std::thread::spawn(move || {
                barrier.wait();
                worker
                    .poll(&store, &session, start + Duration::from_secs(2))
                    .unwrap();
                worker
            })
        };
        let closer = {
            let (store, session, barrier) = (
                Arc::clone(&store),
                Arc::clone(&session),
                Arc::clone(&barrier),
            );
            std::thread::spawn(move || {
                barrier.wait();
                tauri::async_runtime::block_on(transition(&session, &store, |s| {
                    s.clear();
                    Ok(())
                }))
                .unwrap();
            })
        };
        barrier.wait();
        closer.join().unwrap();
        let mut worker = writer.join().unwrap();
        worker
            .poll(&store, &session, start + Duration::from_secs(60))
            .unwrap();
        assert!(recovery::read(&store.dir, &id).is_err());
    }

    #[test]
    fn save_and_project_replacement_should_not_reuse_a_pending_snapshot() {
        tauri::async_runtime::block_on(async {
            let (dir, store, session) = setup();
            let start = Instant::now();
            let mut worker = Worker::default();
            worker.poll(&store, &session, start).unwrap();
            worker
                .poll(&store, &session, start + Duration::from_secs(2))
                .unwrap();
            let id = session.lock().unwrap().recovery().unwrap().id;
            let path = dir.path().join("show.fxtc");
            transition(&session, &store, move |s| {
                let project = s.document().unwrap().project;
                s.save(&project, &path)
            })
            .await
            .unwrap();
            worker
                .poll(&store, &session, start + Duration::from_secs(3))
                .unwrap();
            assert!(recovery::read(&store.dir, &id).is_err());
            transition(&session, &store, |s| {
                s.new_project("Other", FrameRate::Fps25);
                Ok(())
            })
            .await
            .unwrap();
            assert!(
                !worker
                    .poll(&store, &session, start + Duration::from_secs(60))
                    .unwrap()
            );
            assert!(recovery::list(&store.dir).unwrap().is_empty());
        });
    }

    #[test]
    fn schedule_should_debounce_bound_continuous_edits_and_follow_project_preferences() {
        let mut session = Session::default();
        session.new_project("Show", FrameRate::Fps25);
        let mut record = session.recovery().unwrap();
        record.document.project.name = "Edited".into();
        let mut schedule = Schedule::default();
        let start = Instant::now();
        assert!(!schedule.observe(&record, start));
        assert!(!schedule.observe(&record, start + Duration::from_secs(1)));
        assert!(schedule.observe(&record, start + Duration::from_secs(2)));
        schedule.written(start + Duration::from_secs(2));
        assert!(!schedule.observe(&record, start + Duration::from_secs(20)));
        for second in 21..32 {
            record.document.project.name = format!("Edit {second}");
            assert!(!schedule.observe(&record, start + Duration::from_secs(second)));
        }
        record.document.project.name = "Last".into();
        assert!(schedule.observe(&record, start + Duration::from_secs(32)));
        record.document.project.autosave.enabled = false;
        assert!(!schedule.observe(&record, start + Duration::from_secs(100)));
        session.new_project("Other", FrameRate::Fps25);
        let clean = session.recovery().unwrap();
        assert!(!schedule.observe(&clean, start + Duration::from_secs(101)));
    }

    #[test]
    fn successful_transition_should_clean_and_failed_transition_should_preserve() {
        tauri::async_runtime::block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let store = Arc::new(Recoveries {
                dir: dir.path().into(),
                gate: Mutex::new(None),
            });
            let session = SharedSession::default();
            session
                .lock()
                .unwrap()
                .new_project("Show", FrameRate::Fps25);
            let record = session.lock().unwrap().recovery().unwrap();
            recovery::write(&store.dir, &record).unwrap();
            let failed: Result<()> = transition(&session, &store, |_| {
                Err(ProjectError::Invalid("save failed".into()))
            })
            .await;
            assert!(failed.is_err());
            assert_eq!(recovery::list(&store.dir).unwrap().len(), 1);
            transition(&session, &store, |s| {
                s.clear();
                Ok(())
            })
            .await
            .unwrap();
            assert!(recovery::list(&store.dir).unwrap().is_empty());
            assert!(session.lock().unwrap().recovery().is_none());
        });
    }
}
