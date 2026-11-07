use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::decode::Decoded;

/// Decoded audio retained up to 256 MiB. Playback keeps independent Arc owners.
#[derive(Debug)]
pub struct AudioCache {
    items: Mutex<Entries>,
    budget: usize,
}
#[derive(Debug, Default)]
struct Entries {
    items: HashMap<String, (Arc<Decoded>, u64)>,
    tick: u64,
    bytes: usize,
}

impl Default for AudioCache {
    fn default() -> Self {
        Self::with_budget(256 * 1024 * 1024)
    }
}
pub type SharedAudio = Arc<AudioCache>;
impl AudioCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            items: Mutex::new(Entries::default()),
            budget,
        }
    }
    pub fn get(&self, asset_id: &str) -> Option<Arc<Decoded>> {
        let mut entries = self.lock();
        entries.tick = entries.tick.wrapping_add(1);
        let tick = entries.tick;
        entries.items.get_mut(asset_id).map(|(audio, seen)| {
            *seen = tick;
            Arc::clone(audio)
        })
    }
    pub fn insert(&self, asset_id: &str, decoded: Arc<Decoded>) {
        let mut entries = self.lock();
        if let Some((old, _)) = entries.items.remove(asset_id) {
            entries.bytes -= old.samples.len() * size_of::<i16>();
        }
        let bytes = decoded.samples.len() * size_of::<i16>();
        if bytes > self.budget {
            return;
        }
        while entries.bytes + bytes > self.budget {
            let Some(oldest) = entries
                .items
                .iter()
                .min_by_key(|(_, (_, tick))| tick)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            if let Some((old, _)) = entries.items.remove(&oldest) {
                entries.bytes -= old.samples.len() * size_of::<i16>();
            }
        }
        entries.tick = entries.tick.wrapping_add(1);
        let tick = entries.tick;
        entries.items.insert(asset_id.to_owned(), (decoded, tick));
        entries.bytes += bytes;
    }
    pub fn retained_bytes(&self) -> usize {
        self.lock().bytes
    }
    pub fn clear(&self) {
        *self.lock() = Entries::default();
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Entries> {
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

    #[test]
    fn cache_evicts_least_recently_used_without_invalidating_playback() {
        let cache = AudioCache::with_budget(16);
        let playing = decoded();
        cache.insert("playing", Arc::clone(&playing));
        cache.insert("second", decoded());
        cache.get("second").unwrap();
        cache.insert("third", decoded());
        assert!(cache.get("playing").is_none());
        assert!(cache.get("second").is_some());
        assert_eq!(playing.samples, vec![0; 4]);
        assert_eq!(cache.retained_bytes(), 16);
        cache.clear();
        assert_eq!(cache.retained_bytes(), 0);
        assert_eq!(playing.frames(), 2);
    }

    #[test]
    fn oversized_replacement_does_not_exceed_retained_budget() {
        let cache = AudioCache::with_budget(8);
        cache.insert("same", decoded());
        cache.insert(
            "same",
            Arc::new(Decoded {
                samples: vec![0; 100],
                source_rate: 48_000,
            }),
        );
        assert!(cache.get("same").is_none());
        assert_eq!(cache.retained_bytes(), 0);
    }
}
