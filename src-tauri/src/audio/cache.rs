use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::decode::Decoded;

/// Decoded audio by asset id, so a file is decoded once and then shared with the output.
#[derive(Debug, Default)]
pub struct AudioCache {
    items: Mutex<HashMap<String, Arc<Decoded>>>,
}

pub type SharedAudio = Arc<AudioCache>;

impl AudioCache {
    pub fn get(&self, asset_id: &str) -> Option<Arc<Decoded>> {
        self.lock().get(asset_id).cloned()
    }

    pub fn insert(&self, asset_id: &str, decoded: Arc<Decoded>) {
        self.lock().insert(asset_id.to_owned(), decoded);
    }

    /// Forgets everything: the assets of a closed project are not the next project's.
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Decoded>>> {
        self.items.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded() -> Arc<Decoded> {
        Arc::new(Decoded {
            samples: vec![0; 4],
            source_rate: 48_000,
        })
    }

    #[test]
    fn get_should_return_what_was_inserted() {
        let cache = AudioCache::default();
        let audio = decoded();
        cache.insert("a1", Arc::clone(&audio));
        assert!(Arc::ptr_eq(&cache.get("a1").unwrap(), &audio));
        assert!(cache.get("a2").is_none());
    }

    #[test]
    fn clear_should_forget_every_asset() {
        let cache = AudioCache::default();
        cache.insert("a1", decoded());
        cache.clear();
        assert!(cache.get("a1").is_none());
    }
}
