//! Copying one entry of an archive into another.

use std::io::{Read, Write};
use std::path::Path;

use super::*;
use crate::error::Result;

/// The size of the entry `file` of the archive at `path`.
pub(crate) fn entry_size(path: &Path, file: &str) -> Result<u64> {
    let mut archive = open_archive(path)?;
    Ok(by_name(&mut archive, file)?.size())
}

/// Copies the entry `file` of the archive at `path` into `out`, telling `progress` the bytes of each chunk.
pub(crate) fn copy_entry(
    path: &Path,
    file: &str,
    out: &mut impl Write,
    progress: &mut impl FnMut(u64),
) -> Result<()> {
    let mut archive = open_archive(path)?;
    let mut entry = by_name(&mut archive, file)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = entry.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        out.write_all(&buffer[..n])?;
        progress(n as u64);
    }
}
