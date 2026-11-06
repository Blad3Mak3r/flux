//! The types every other crate counts with: frame rates, timecode and the sample clock. No I/O,
//! no devices, no project format, so anything can depend on it.

mod clock;
mod timecode;

pub use clock::Clock;
pub use timecode::{FrameRate, Timecode, TimecodeError};

/// The sample rate the output stream runs at, and so the rate everything is decoded to.
pub const ENGINE_RATE: u32 = 48_000;
