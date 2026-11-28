//! Parameter modulation of the VJ grid, as in Resolume: a numeric setting is driven automatically,
//! between a minimum and a maximum, by an LFO (synced to the tempo or free in Hz) or by the sound
//! (the low, mid or high band, or the level).
//!
//! A [`Modulation`] is stored with the thing it moves: an effect (any numeric setting of its kind),
//! a layer (`master`, `opacity`, `volume`) or a clip (`scale`, `positionX`, `positionY`, and the
//! numbers of its generator as `generator.size`, `generator.rotation`...). It never
//! changes the stored value: the screens and the sound ask [`modulate`] for a copy of the grid
//! with every modulated value in place of the stored one, and the stored grid stays as it was.
//!
//! The value is `min + (max - min) * s`, `s` from 0 to 1. `min` may be above `max`, which inverts
//! the movement. An LFO runs on the composition's clock, in seconds or in beats, never on a clip's
//! own time, so every LFO of the grid stays in phase whatever clips are launched when. Evaluating
//! is pure: the one state is the smoothing of an audio source ([`Modulator`]).

mod apply;
mod catalog;
mod modulator;
mod shape;
#[cfg(test)]
mod support;
mod types;

pub use apply::*;
pub use catalog::*;
pub use modulator::*;
pub use shape::*;
pub use types::*;
