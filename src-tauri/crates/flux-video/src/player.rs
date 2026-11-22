use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use crate::decode::{Frame, VideoError, VideoFile, VideoInfo};

/// What the decoding thread did, counted for the `flux-log` lane to write: the thread never logs.
#[derive(Debug, Default)]
pub struct PlayerStats {
    /// Frames handed to `latest`.
    pub decoded: AtomicU64,
    /// Requests dropped because a newer one arrived before they were decoded.
    pub skipped: AtomicU64,
    /// Requests that failed to decode (past the end, a damaged file).
    pub errors: AtomicU64,
}

/// Decodes one video file on a thread of its own. The caller asks for the frame the clock is at
/// ([`VideoPlayer::request`]) and reads the newest decoded one ([`VideoPlayer::latest`]); only the
/// newest request is decoded, so a slow decode drops frames instead of falling behind the clock.
pub struct VideoPlayer {
    requests: Option<Sender<u64>>,
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    stats: Arc<PlayerStats>,
    info: VideoInfo,
    asset: String,
    thread: Option<JoinHandle<()>>,
}

impl VideoPlayer {
    /// Opens the file on its own thread (FFmpeg's state stays there) and waits until it is open,
    /// so an unreadable file fails here.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, VideoError> {
        let path = path.into();
        let (requests, inbox) = mpsc::channel();
        let (opened, open_result) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let stats = Arc::new(PlayerStats::default());
        let thread = thread::Builder::new().name("flux-video".into()).spawn({
            let latest = Arc::clone(&latest);
            let stats = Arc::clone(&stats);
            move || match VideoFile::open(&path) {
                Ok(file) => {
                    let _ = opened.send(Ok(file.info().clone()));
                    run(file, &inbox, &latest, &stats);
                }
                Err(err) => {
                    let _ = opened.send(Err(err));
                }
            }
        })?;
        let info = match open_result.recv() {
            Ok(Ok(info)) => info,
            Ok(Err(err)) => {
                let _ = thread.join();
                return Err(err);
            }
            // The thread ended without a word: it panicked while opening.
            Err(_) => {
                let _ = thread.join();
                return Err(VideoError::NoVideo);
            }
        };
        Ok(Self {
            requests: Some(requests),
            latest,
            stats,
            info,
            asset: String::new(),
            thread: Some(thread),
        })
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// Labels the player with the video asset id it plays, for [`crate::VideoDeck`].
    pub fn for_asset(mut self, asset: &str) -> Self {
        asset.clone_into(&mut self.asset);
        self
    }

    /// The asset id given with [`for_asset`](Self::for_asset); empty otherwise.
    pub fn asset(&self) -> &str {
        &self.asset
    }

    /// Asks for the frame at `index` (see [`crate::source_frame`]). Never blocks.
    pub fn request(&self, index: u64) {
        if let Some(requests) = &self.requests {
            // The thread only stops when this player is dropped.
            let _ = requests.send(index);
        }
    }

    /// The newest frame decoded, `None` until the first one is ready.
    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn stats(&self) -> &PlayerStats {
        &self.stats
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        // Closing the channel ends the thread's loop.
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    mut file: VideoFile,
    inbox: &Receiver<u64>,
    latest: &Mutex<Option<Arc<Frame>>>,
    stats: &PlayerStats,
) {
    while let Ok(mut index) = inbox.recv() {
        // Only the newest request matters: the clock has moved past the others.
        for newer in inbox.try_iter() {
            index = newer;
            stats.skipped.fetch_add(1, Ordering::Relaxed);
        }
        let shown = latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|frame| frame.index);
        if shown == Some(index) {
            continue;
        }
        match file.frame(index) {
            Ok(frame) => {
                *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(frame));
                stats.decoded.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
