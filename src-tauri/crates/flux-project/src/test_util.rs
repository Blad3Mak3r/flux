//! Helpers shared by the tests of the crate's modules.

use std::fs;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `dir/name`, creating the folders in between, and returns the path.
pub(crate) fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}
