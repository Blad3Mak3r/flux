//! Text as a picture: the fonts the system has (and one bundled, so a missing font never stops a
//! text from drawing), a block of lines laid out and rasterised on the CPU into straight RGBA over a
//! transparent background. Pure Rust: `fontdb` finds the fonts, `rustybuzz` (a HarfBuzz port) shapes
//! each line (GPOS kerning, ligatures, marks) and `ab_glyph` draws the glyph ids it returns.
//!
//! A character the chosen face does not have is drawn with another face: a face of the system whose
//! cmap has it (the same style first), else the bundled Inter. A line is cut into runs of one face
//! and each run is shaped on its own. A glyph the face keeps in colour (bitmaps in CBDT or sbix,
//! layers in COLR v0/v1: see [`colour`]) is drawn with its own colours, only fading with the
//! block's opacity; any other glyph is an outline drawn in the block's colour.

mod colour;

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ab_glyph::{Font, FontRef, GlyphId, PxScale, ScaleFont, point};
use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use rustybuzz::ttf_parser;

mod fonts;
mod layout;
mod raster;

pub use fonts::*;
pub(crate) use layout::*;
pub use raster::*;

#[cfg(test)]
mod tests;

/// The family that is bundled: the fallback for a family the machine does not have.
pub const BUNDLED_FAMILY: &str = "Inter";

const REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular-subset.otf");
const BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold-subset.otf");

/// Largest side of the bitmap one block is drawn in; a block past it is cut.
const MAX_SIDE: usize = 4096;
/// Blank pixels round the block, so a glyph that leans out of its advance is not cut.
const PAD: usize = 2;

/// How the lines of a block sit against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// A family the machine has, with the names of its faces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    pub name: String,
    pub styles: Vec<String>,
}

/// A block of text to draw on a canvas; sizes are in pixels of that canvas.
#[derive(Debug, Clone)]
pub struct Block<'a> {
    /// Its lines, split by `\n`.
    pub text: &'a str,
    pub family: &'a str,
    pub style: &'a str,
    /// The em of the font, in pixels.
    pub size: f32,
    pub colour: [u8; 3],
    /// 0 to 1: the alpha of the glyphs.
    pub opacity: f32,
    pub align: Align,
    /// Where the centre of the block is.
    pub centre: [f32; 2],
    /// Degrees, clockwise, about the centre.
    pub rotation: f32,
    /// Extra space after each glyph, in ems.
    pub tracking: f32,
    /// Extra space between lines, in ems.
    pub leading: f32,
}

// ---------------------------------------------------------------------------------------------
// Fonts
