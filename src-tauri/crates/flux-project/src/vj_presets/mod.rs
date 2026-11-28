//! Presets of the VJ grid, kept per user like Resolume keeps them in the user's Documents folder:
//! **effect presets** (the settings of one effect, or of a whole stack, under a name) and **show
//! templates** (the structure of a VJ show without its clips).
//!
//! Both live outside any show, in the app data folder, as versioned JSON; a preset is copied into
//! the show when it is used and the show never refers back to it, so a show stays portable. The
//! files are read and written here; the app crate only gives them a folder.

/// The version of the preset library and of a template file this build writes; a file with a
/// higher one was made by a newer Flux and is refused, never read half.
pub const VERSION: u32 = 1;
/// The most presets the library holds.
pub const MAX_PRESETS: usize = 500;
/// The most templates the folder holds.
pub const MAX_TEMPLATES: usize = 100;
/// The longest name of a preset or a template, in characters.
pub const MAX_NAME: usize = 64;

pub(crate) fn name_of(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(ProjectError::Invalid(format!(
            "a name is 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

use crate::{ProjectError, Result};

mod effect;
mod library;
#[cfg(test)]
mod support;
mod template;

pub use effect::*;
pub use library::*;
pub use template::*;
