//! Progress of the slow steps that follow an import (probing, thumbnails, decoding, tempo), told to
//! the window as `media-progress` events so the clips of an asset can draw a loading bar.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const MEDIA_PROGRESS: &str = "media-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaProgress<'a> {
    asset: &'a str,
    stage: &'static str,
    /// 0 to 1, or `None` while the step cannot tell how far it is.
    fraction: Option<f32>,
    done: bool,
}

/// One step of one asset: announced when it starts, updated by [`Stage::report`] at most ten times
/// a second, and closed when dropped (also when the step fails).
pub struct Stage {
    app: AppHandle,
    asset: String,
    stage: &'static str,
    last: Instant,
}

impl Stage {
    pub fn start(app: &AppHandle, asset: &str, stage: &'static str) -> Self {
        let this = Self {
            app: app.clone(),
            asset: asset.to_owned(),
            stage,
            last: Instant::now(),
        };
        this.emit(None, false);
        this
    }

    pub fn report(&mut self, fraction: f32) {
        if self.last.elapsed() >= Duration::from_millis(100) {
            self.last = Instant::now();
            self.emit(Some(fraction), false);
        }
    }

    fn emit(&self, fraction: Option<f32>, done: bool) {
        let _ = self.app.emit(
            MEDIA_PROGRESS,
            MediaProgress {
                asset: &self.asset,
                stage: self.stage,
                fraction,
                done,
            },
        );
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.emit(None, true);
    }
}
