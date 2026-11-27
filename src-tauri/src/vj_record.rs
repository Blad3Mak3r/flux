//! Recording the VJ composition to a HAP QuickTime file (`docs/vj-mode.md`, Recording). A thread draws
//! the composition on a GPU of its own with no window, one frame for each tick of the project's frame
//! rate, and hands the frames to `flux_video::Recorder`, which compresses and writes them on another.
//! The sound is the grid's programme mix, which the output's audio callback copies into a lock-free
//! ring while a recording is on (`flux_engine::analysis::record_tap`). Nothing here logs from the
//! frame loop: the counters are read by `vj_record_state` and written to the log when it stops.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::SharedSession;
use crate::transport_commands::SharedOutput;
use crate::video_output::{self, GridSources, SharedVideoOutput};
use flux_engine::analysis::{self, Ring};
use flux_project::{Project, ProjectState};
use flux_render::Screen;
use flux_video::{RecordConfig, RecordCounters, Recorder};

/// The recording being made, if any.
pub type SharedRecording = Arc<Mutex<Option<Active>>>;

/// What a recording that ended says of itself.
struct Finished {
    path: PathBuf,
}

pub struct Active {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<Finished, String>>,
    started: Instant,
    counters: Arc<RecordCounters>,
    path: PathBuf,
    audio: bool,
}

/// Where a recording is shown to be: what `vj_record_state` answers.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordState {
    pub recording: bool,
    pub elapsed_ms: u64,
    /// Frames written so far, repeats included.
    pub frames: u64,
    /// Frames that were not ready in time and repeat the one before.
    pub dropped: u64,
    pub path: Option<String>,
    /// Whether the file carries the grid's sound.
    pub audio: bool,
    /// What went wrong, when the recording cannot go on.
    pub error: Option<String>,
}

/// What `vj_record_stop` answers: the project with the recording added to its media.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStop {
    pub state: ProjectState,
    pub path: String,
    pub frames: u64,
    pub dropped: u64,
}

/// The folder recordings go to: the one chosen in the preferences, else `FLUX` in the user's
/// videos, else a `Recordings` folder in the app's data.
pub fn folder(chosen: Option<&str>, videos: Option<PathBuf>, data: &Path) -> PathBuf {
    match chosen.map(str::trim).filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => videos.map_or_else(|| data.join("Recordings"), |dir| dir.join("FLUX")),
    }
}

/// The path of a recording of `project` started at `stamp` (`2026-01-31 12-00-00`) in `dir` that no
/// file has yet: ` 2`, ` 3`... follow the stamp when two start in one second. Characters a file
/// name cannot hold become `_`.
pub fn file(dir: &Path, project: &str, stamp: &str) -> PathBuf {
    let clean: String = project
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.');
    let name = if clean.is_empty() { "FLUX" } else { clean };
    let first = dir.join(format!("{name} {stamp}.mov"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{name} {stamp} {n}.mov")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

/// The tick of a clock at `rate` frames a second (numerator, denominator) that `elapsed` is in.
fn tick_at(elapsed: Duration, rate: (u32, u32)) -> u64 {
    (elapsed.as_nanos() * u128::from(rate.0) / (u128::from(rate.1) * 1_000_000_000)) as u64
}

/// How long after the start tick `tick` begins.
fn tick_start(tick: u64, rate: (u32, u32)) -> Duration {
    Duration::from_nanos(
        (u128::from(tick) * u128::from(rate.1) * 1_000_000_000 / u128::from(rate.0)) as u64,
    )
}

/// A sample as 16-bit PCM.
fn pcm(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0) as i16
}

/// Moves what the audio callback queued to the recorder.
fn drain(ring: &Ring, recorder: &Recorder, block: &mut [f32]) {
    loop {
        let count = ring.pop(block);
        if count == 0 {
            return;
        }
        recorder.push_audio(block[..count].iter().map(|&s| pcm(s)).collect());
        if count < block.len() {
            return;
        }
    }
}

/// What the frame loop works with.
struct Job {
    session: SharedSession,
    sources: Sources,
    recorder: Recorder,
    ring: Option<Arc<Ring>>,
    stop: Arc<AtomicBool>,
    rate: (u32, u32),
    /// Where the recording is kept, to leave it when the project goes away on its own.
    slot: SharedRecording,
    /// The document the recording started in (its recovery id).
    document: String,
}

/// Whether the recording has lost its project: it is closed, another one was opened or made, or
/// it is no longer a VJ show.
fn project_gone(started_in: &str, now: &str, vj: bool) -> bool {
    !vj || started_in != now
}

/// Waits up to `limit` for `handle`; `false` when it did not finish in time (the thread is left to run).
fn join_within<T: Send + 'static>(handle: JoinHandle<T>, limit: Duration) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    rx.recv_timeout(limit).is_ok()
}

/// Finishes the recording at app exit, waiting up to 3 s for the trailer. The file is not linked
/// into any project: it is going away.
pub fn shutdown(app: &AppHandle) {
    let Some(active) = app
        .state::<SharedRecording>()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };
    active.stop.store(true, Ordering::Relaxed);
    let path = active.path.clone();
    if join_within(active.thread, Duration::from_secs(3)) {
        log::info!("recording finished at exit: {}", path.display());
    } else {
        log::warn!(
            "recording did not finish in time at exit: {}",
            path.display()
        );
    }
}

struct Sources {
    grid: crate::vj_commands::SharedGrid,
    modulation: crate::modulation::SharedModulation,
    deck: video_output::SharedDeck,
}

/// Draws and records until `stop`; the file is finished before it answers.
fn record(
    job: Job,
    ready: &std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<Finished, String> {
    let Job {
        session,
        sources,
        recorder,
        ring,
        stop,
        rate,
        slot,
        document,
    } = job;
    let path = recorder.path().to_path_buf();
    let mut screen = match Screen::offscreen() {
        Ok(screen) => screen,
        Err(err) => {
            let _ = recorder.finish(0);
            let _ = std::fs::remove_file(&path);
            let _ = ready.send(Err(err.to_string()));
            return Err(err.to_string());
        }
    };
    let _ = ready.send(Ok(()));

    let counters = Arc::clone(recorder.counters());
    let mut block = vec![0.0f32; 4096];
    if let Some(ring) = &ring {
        ring.clear();
        analysis::set_record_tap(true);
    }
    let started = Instant::now();
    let (mut seen, mut vj, mut size) = ((u64::MAX, String::new()), None, (1, 1));
    let mut drawn = None;
    let mut bpm = 120.0;
    let mut detached = false;
    let (mut tick, mut last) = (0u64, started);
    while !stop.load(Ordering::Relaxed) && counters.error().is_none() {
        let due = tick_start(tick, rate);
        let elapsed = started.elapsed();
        if elapsed < due {
            thread::sleep((due - elapsed).min(Duration::from_millis(20)));
            if let Some(ring) = &ring {
                drain(ring, &recorder, &mut block);
            }
            continue;
        }
        // Late by whole ticks: they are left out, and the recorder repeats the frame before.
        tick = tick.max(tick_at(elapsed, rate));
        let changed = {
            let session = session.lock().unwrap_or_else(PoisonError::into_inner);
            if project_gone(
                &document,
                session.recovery_revision().0,
                session.vj_project().is_some(),
            ) {
                detached = true;
                break;
            }
            let (revision, active) = session.revision();
            let changed = (revision, active) != (seen.0, seen.1.as_str());
            if changed {
                seen = (revision, active.to_owned());
                vj = session.grid_copy();
                bpm = session.tempo_bpm();
                drawn = None;
            }
            changed
        };
        if changed {
            size = video_output::composition_size(&session);
        }
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32();
        last = now;
        let (shown, looks, time, post) = match &vj {
            Some(vj) => {
                let frame = video_output::grid_frame(
                    vj,
                    bpm,
                    &GridSources {
                        grid: &sources.grid,
                        modulation: &sources.modulation,
                        deck: &sources.deck,
                    },
                    &mut drawn,
                    size,
                    dt,
                );
                (frame.shown, frame.looks, frame.time, frame.post)
            }
            None => Default::default(),
        };
        let items = video_output::items(&shown, &looks, size, time);
        for frame in screen.capture(&items, size, &post, tick) {
            recorder.push_frame(frame.tag, frame.rgba);
        }
        tick += 1;
        if let Some(ring) = &ring {
            drain(ring, &recorder, &mut block);
        }
    }
    analysis::set_record_tap(false);
    for frame in screen.capture_finish() {
        recorder.push_frame(frame.tag, frame.rgba);
    }
    if let Some(ring) = &ring {
        drain(ring, &recorder, &mut block);
    }
    recorder.finish(tick).map_err(|err| err.to_string())?;
    if detached {
        // Nobody will call stop: the file is left finished, not linked, and the screen sees it end.
        log::info!(
            "recording finished, its project is gone: {}",
            path.display()
        );
        let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|a| Arc::ptr_eq(&a.stop, &stop)) {
            *slot = None;
        }
    }
    Ok(Finished { path })
}

fn current(active: &Active) -> RecordState {
    RecordState {
        recording: true,
        elapsed_ms: active.started.elapsed().as_millis() as u64,
        frames: active.counters.written.load(Ordering::Relaxed),
        dropped: active.counters.dropped.load(Ordering::Relaxed),
        path: Some(active.path.to_string_lossy().into_owned()),
        audio: active.audio,
        error: active.counters.error(),
    }
}

fn start(app: &AppHandle, recording: &SharedRecording) -> Result<RecordState, String> {
    if !flux_video::hap_available() {
        return Err("this build of FFmpeg cannot write HAP, which recordings use".into());
    }
    let mut slot = recording.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.is_some() {
        return Err("a recording is already running".into());
    }
    let session = Arc::clone(&app.state::<SharedSession>());
    let (name, rate, document) = {
        let session = session.lock().unwrap_or_else(PoisonError::into_inner);
        let project = session.vj_project().ok_or("recording needs a VJ project")?;
        let rate = session
            .active_timeline_copy()
            .map_or((25, 1), |t| t.timecode.frame_rate.fraction());
        (
            project.name.clone(),
            (rate.0 as u32, rate.1 as u32),
            session.recovery_revision().0.to_owned(),
        )
    };
    let chosen =
        crate::preferences::preferences_get(app.clone(), app.state::<crate::preferences::Store>())?
            .recordings_dir;
    let data = app.path().app_data_dir().map_err(|err| err.to_string())?;
    let dir = folder(chosen.as_deref(), dirs::video_dir(), &data);
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("could not make the folder {}: {err}", dir.display()))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    let path = file(&dir, &name, &stamp);

    let size = video_output::composition_size(&session);
    // The sound exists only while an audio output runs.
    let audio = app
        .state::<SharedOutput>()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some();
    let outputs = Arc::clone(&app.state::<SharedVideoOutput>());
    video_output::ensure_deck(outputs.deck(), app)?;
    let recorder = Recorder::start(RecordConfig {
        path: path.clone(),
        size,
        rate,
        audio: audio.then_some((flux_audio::ENGINE_RATE, 2)),
    })
    .map_err(|err| err.to_string())?;
    let counters = Arc::clone(recorder.counters());
    let stop = Arc::new(AtomicBool::new(false));
    let job = Job {
        session,
        sources: Sources {
            grid: Arc::clone(&app.state::<crate::vj_commands::SharedGrid>()),
            modulation: Arc::clone(&app.state::<crate::modulation::SharedModulation>()),
            deck: Arc::clone(outputs.deck()),
        },
        recorder,
        ring: audio.then(analysis::record_tap),
        stop: Arc::clone(&stop),
        rate,
        slot: Arc::clone(recording),
        document,
    };
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let thread = thread::Builder::new()
        .name("flux-vj-record".into())
        .spawn(move || record(job, &ready_tx))
        .map_err(|err| err.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(format!("no graphics device to draw the recording: {err}")),
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            return Err("the graphics device did not answer".into());
        }
    }
    log::info!(
        "recording started: {} ({}x{} at {}/{} fps, {})",
        path.display(),
        size.0,
        size.1,
        rate.0,
        rate.1,
        if audio { "with sound" } else { "no sound" }
    );
    let active = Active {
        stop,
        thread,
        started: Instant::now(),
        counters,
        path,
        audio,
    };
    let state = current(&active);
    *slot = Some(active);
    Ok(state)
}

/// Starts recording the composition of the VJ project to a HAP file in the recordings folder.
#[tauri::command]
pub async fn vj_record_start(
    app: AppHandle,
    recording: State<'_, SharedRecording>,
) -> Result<RecordState, String> {
    let recording = Arc::clone(&recording);
    tauri::async_runtime::spawn_blocking(move || start(&app, &recording))
        .await
        .map_err(|err| err.to_string())?
}

/// Finishes the recording and adds the file to the project's media, linked, as the project
/// `project` has it.
#[tauri::command]
pub async fn vj_record_stop(
    session: State<'_, SharedSession>,
    recording: State<'_, SharedRecording>,
    project: Project,
) -> Result<RecordStop, String> {
    let active = recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or("no recording is running")?;
    let counters = Arc::clone(&active.counters);
    active.stop.store(true, Ordering::Relaxed);
    let finished = tauri::async_runtime::spawn_blocking(move || active.thread.join())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|_| "the recording stopped unexpectedly".to_owned())?;
    let finished = finished.inspect_err(|err| log::warn!("recording failed: {err}"))?;
    let (frames, dropped) = (
        counters.written.load(Ordering::Relaxed),
        counters.dropped.load(Ordering::Relaxed),
    );
    log::info!(
        "recording saved: {} ({frames} frames, {dropped} repeated)",
        finished.path.display()
    );
    let path = finished.path.clone();
    let state = crate::commands::run(&session, move |s| {
        s.update(project);
        s.import_media(&path)
    })
    .await
    .map_err(|err| err.to_string())?;
    Ok(RecordStop {
        state,
        path: finished.path.to_string_lossy().into_owned(),
        frames,
        dropped,
    })
}

/// What the recording is doing now.
#[tauri::command]
pub fn vj_record_state(recording: State<'_, SharedRecording>) -> RecordState {
    recording
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(current)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_should_prefer_the_choice_then_videos_then_the_app_data() {
        let data = Path::new("/data");
        let videos = Some(PathBuf::from("/vids"));
        assert_eq!(
            folder(Some("/mine"), videos.clone(), data),
            Path::new("/mine")
        );
        assert_eq!(
            folder(Some("  "), videos.clone(), data),
            Path::new("/vids/FLUX")
        );
        assert_eq!(folder(None, videos, data), Path::new("/vids/FLUX"));
        assert_eq!(folder(None, None, data), Path::new("/data/Recordings"));
    }

    #[test]
    fn the_file_should_be_named_after_the_project_and_the_start_and_stay_unique() {
        let dir = tempfile::tempdir().unwrap();
        let first = file(dir.path(), "Club: night/1", "2026-01-31 12-00-00");
        assert_eq!(
            first.file_name().unwrap(),
            "Club_ night_1 2026-01-31 12-00-00.mov"
        );
        std::fs::write(&first, b"").unwrap();
        let second = file(dir.path(), "Club: night/1", "2026-01-31 12-00-00");
        assert_eq!(
            second.file_name().unwrap(),
            "Club_ night_1 2026-01-31 12-00-00 2.mov"
        );
        assert_eq!(
            file(dir.path(), " .. ", "t").file_name().unwrap(),
            "FLUX t.mov"
        );
    }

    #[test]
    fn ticks_should_follow_the_frame_rate_with_no_drift() {
        let ntsc = (30_000, 1_001);
        assert_eq!(tick_at(Duration::from_secs(10), ntsc), 299);
        assert_eq!(tick_at(Duration::from_secs(1001), ntsc), 30_000);
        assert_eq!(tick_at(Duration::from_millis(39), (25, 1)), 0);
        assert_eq!(tick_at(Duration::from_millis(40), (25, 1)), 1);
        assert_eq!(tick_start(25, (25, 1)), Duration::from_secs(1));
        // A tick starts at or after the instant that makes it the current one.
        for n in [1, 7, 1000, 100_000] {
            assert_eq!(
                tick_at(tick_start(n, ntsc) + Duration::from_nanos(1), ntsc),
                n
            );
        }
    }

    #[test]
    fn a_recording_should_lose_its_project_when_it_closes_changes_or_stops_being_vj() {
        assert!(!project_gone("a", "a", true));
        assert!(project_gone("a", "a", false));
        assert!(project_gone("a", "b", true));
        assert!(project_gone("a", "", false));
    }

    #[test]
    fn joining_should_give_up_after_the_limit() {
        let quick = thread::spawn(|| ());
        assert!(join_within(quick, Duration::from_secs(2)));
        let slow = thread::spawn(|| thread::sleep(Duration::from_millis(500)));
        assert!(!join_within(slow, Duration::from_millis(20)));
    }

    #[test]
    fn samples_should_become_16_bit_and_clip() {
        assert_eq!(pcm(0.0), 0);
        assert_eq!(pcm(1.0), 32_767);
        assert_eq!(pcm(-3.0), -32_767);
    }
}
