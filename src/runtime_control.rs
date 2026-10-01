use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::settings::SavedSettings;

pub struct RuntimeControl {
    settings: Mutex<SavedSettings>,
    generation: AtomicU64,
}

impl RuntimeControl {
    pub fn new(settings: SavedSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            generation: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> SavedSettings {
        self.settings
            .lock()
            .expect("runtime settings mutex poisoned")
            .clone()
    }

    pub fn replace(&self, settings: SavedSettings) {
        *self
            .settings
            .lock()
            .expect("runtime settings mutex poisoned") = settings;
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn restart(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub type SharedRuntimeControl = Arc<RuntimeControl>;
