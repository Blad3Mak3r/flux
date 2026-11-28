//! How the video reaches the screen, like the advanced output of a media server: the picture of
//! the timeline (the composition) is cut into slices, and each slice is a rectangle of it placed on
//! the output by its four corners, which can lean it to fit a surface seen at an angle. No slices
//! is the plain output: the whole picture fitted to the screen.
//!
//! A slice can fade to black towards its edges (a soft edge), so two projectors whose pictures
//! overlap blend into one; a screen's masks black out shapes of it, or everything but them.
//!
//! The output has one or more screens, each a window on a display of its own with its own slices.
//! The main screen always exists, even when the project does not list it; which display each
//! screen opens on is a preference of the computer, not of the project.
//!
//! Every number is a fraction of a width or height, from the top left corner, so a mapping keeps
//! its shape on a screen of another resolution.

mod apply;
mod card;
mod mask;
mod screen;
mod slice;
#[cfg(test)]
mod support;

pub use apply::*;
pub use card::*;
pub use mask::*;
pub use screen::*;
pub use slice::*;
#[cfg(test)]
use support::added;

/// Most slices a screen has.
pub const MAX_SLICES: usize = 64;
/// Most screens an output has.
pub const MAX_SCREENS: usize = 8;
/// Most masks a screen has, and most points of one.
pub const MAX_MASKS: usize = 8;
pub const MAX_MASK_POINTS: usize = 32;
/// The id of the screen every project has.
pub const MAIN_SCREEN: &str = "main";
/// Longest slice name, in characters.
const MAX_NAME: usize = 80;
/// Smallest side of a slice's rectangle, so it never vanishes.
const MIN_SIDE: f32 = 0.005;
/// How far a corner may go past the screen's edges: a projector's picture can hang off them.
const CORNER_REACH: f32 = 1.0;
