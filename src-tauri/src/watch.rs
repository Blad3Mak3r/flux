//! State the window used to ask for on a timer, pushed instead: a thread reads the state at a pace
//! and sends it down a [`Channel`] only when it differs from what it sent last. The window opens a
//! watch with a `*_watch` command and closes it with [`watch_stop`], by the id of its channel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use tauri::State;
use tauri::ipc::{Channel, IpcResponse};

/// The watches that run, by the id of the channel each sends on.
#[derive(Default)]
pub struct Watches(Mutex<HashMap<u32, Arc<AtomicBool>>>);

impl Watches {
    /// Calls `next` every `every` on a thread of its own and sends what it answers, until
    /// [`Self::stop`] or the channel cannot send any more.
    pub fn start<T: IpcResponse + Send + 'static>(
        &self,
        channel: Channel<T>,
        every: Duration,
        mut next: impl FnMut() -> Option<T> + Send + 'static,
    ) {
        let stop = Arc::new(AtomicBool::new(false));
        let id = channel.id();
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("flux-watch".into())
            .spawn(move || {
                while !flag.load(Ordering::Relaxed) {
                    if let Some(value) = next()
                        && channel.send(value).is_err()
                    {
                        break;
                    }
                    thread::sleep(every);
                }
            });
        if spawned.is_ok() {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(id, stop);
        }
    }

    /// Ends the watch that sends on channel `id`.
    pub fn stop(&self, id: u32) {
        if let Some(stop) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id)
        {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// Remembers what was sent last, so that a state that did not change is not sent again.
pub struct Changes<T>(Option<T>);

impl<T> Default for Changes<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: PartialEq + Clone> Changes<T> {
    /// `state` when it is the first or differs from the last one given.
    pub fn next(&mut self, state: T) -> Option<T> {
        if self.0.as_ref() == Some(&state) {
            return None;
        }
        self.0 = Some(state.clone());
        Some(state)
    }
}

/// Ends a watch the window opened.
#[tauri::command]
pub fn watch_stop(watches: State<'_, Watches>, id: u32) {
    watches.stop(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_should_send_the_first_state() {
        assert_eq!(Changes::default().next(1), Some(1));
    }

    #[test]
    fn changes_should_not_repeat_a_state_that_did_not_change() {
        let mut changes = Changes::default();
        changes.next(1);
        assert_eq!(changes.next(1), None);
        assert_eq!(changes.next(2), Some(2));
        assert_eq!(changes.next(2), None);
        assert_eq!(changes.next(1), Some(1));
    }
}
