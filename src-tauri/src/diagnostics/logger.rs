//! A small file logger: `flux.log` in the log folder, rotated by size and pruned by age.
//!
//! Lines are written as they come, not buffered, because events are rare and a crash must not lose
//! the last ones. Never call it from an audio callback: it takes a lock and writes to disk. The
//! callbacks count in atomics and a lane thread logs the counts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

const FILE: &str = "flux.log";
/// The size at which the log is rotated.
const MAX_BYTES: u64 = 1 << 20;
/// Rotated files kept next to the current one (`flux.1.log` is the newest).
const KEPT: u32 = 4;
/// A rotated file older than this is deleted when the app starts.
const MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

fn rotated(dir: &Path, n: u32) -> PathBuf {
    dir.join(format!("flux.{n}.log"))
}

/// The current log file and the rotated ones, newest first, for those that exist.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let current = dir.join(FILE);
    let older = (1..=KEPT).map(|n| rotated(dir, n));
    std::iter::once(current)
        .chain(older)
        .filter(|path| path.is_file())
        .collect()
}

/// The open log file, which moves aside to `flux.1.log` once it would pass `max_bytes`.
struct Sink {
    dir: PathBuf,
    max_bytes: u64,
    file: File,
    size: u64,
}

impl Sink {
    fn open(dir: &Path, max_bytes: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(FILE))?;
        let size = file.metadata()?.len();
        Ok(Self {
            dir: dir.to_path_buf(),
            max_bytes,
            file,
            size,
        })
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> {
        let len = line.len() as u64;
        if self.size > 0 && self.size + len > self.max_bytes {
            self.rotate()?;
        }
        self.file.write_all(line.as_bytes())?;
        self.size += len;
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        // The oldest is overwritten by the one before it, and so on down to the current file.
        for n in (1..KEPT).rev() {
            let from = rotated(&self.dir, n);
            if from.is_file() {
                fs::rename(from, rotated(&self.dir, n + 1))?;
            }
        }
        fs::rename(self.dir.join(FILE), rotated(&self.dir, 1))?;
        self.file = File::create(self.dir.join(FILE))?;
        self.size = 0;
        Ok(())
    }
}

/// Deletes the rotated files that were last written to before `max_age` ago.
fn prune(dir: &Path, max_age: Duration) {
    let now = SystemTime::now();
    for path in (1..=KEPT).map(|n| rotated(dir, n)) {
        let age = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok());
        if age.is_some_and(|age| age > max_age) {
            let _ = fs::remove_file(path);
        }
    }
}

struct Logger {
    sink: Mutex<Sink>,
    level: LevelFilter,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // The libraries Flux builds on only get to report their warnings and errors.
        let ours = metadata.target().starts_with("flux");
        metadata.level() <= if ours { self.level } else { LevelFilter::Warn }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}\n",
            timestamp(now_millis()),
            record.level(),
            record.args()
        );
        // A log that cannot be written has nowhere to say so.
        let _ = self
            .sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_line(&line);
    }

    fn flush(&self) {}
}

/// Starts logging to `dir`. `FLUX_LOG` (`debug`, `warn`, ...) sets the level, which is `info` otherwise.
pub fn init(dir: &Path) -> io::Result<()> {
    prune(dir, MAX_AGE);
    let sink = Sink::open(dir, MAX_BYTES)?;
    let level = std::env::var("FLUX_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(LevelFilter::Info);
    log::set_boxed_logger(Box::new(Logger {
        sink: Mutex::new(sink),
        level,
    }))
    .map_err(io::Error::other)?;
    log::set_max_level(level.max(LevelFilter::Warn));

    // `panic = "abort"` in release still runs the hook first.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        previous(info);
    }));
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The current time as [`timestamp`] writes it.
pub fn now() -> String {
    timestamp(now_millis())
}

/// `2026-11-06T12:34:56.789Z` for a time in milliseconds since the Unix epoch.
fn timestamp(millis: u64) -> String {
    let (secs, millis) = (millis / 1000, millis % 1000);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
    )
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn timestamp_should_format_utc_with_millis() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        // 2024-02-29 (a leap day) at 23:59:58.007.
        assert_eq!(timestamp(1_709_251_198_007), "2024-02-29T23:59:58.007Z");
    }

    #[test]
    fn write_line_should_rotate_when_the_file_would_pass_the_cap() {
        let dir = TempDir::new().unwrap();
        let mut sink = Sink::open(dir.path(), 10).unwrap();
        sink.write_line("first\n").unwrap();
        sink.write_line("second\n").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(FILE)).unwrap(),
            "second\n"
        );
        assert_eq!(
            fs::read_to_string(rotated(dir.path(), 1)).unwrap(),
            "first\n"
        );
    }

    #[test]
    fn rotate_should_keep_only_the_newest_files() {
        let dir = TempDir::new().unwrap();
        let mut sink = Sink::open(dir.path(), 1).unwrap();
        for n in 0..8 {
            sink.write_line(&format!("{n}\n")).unwrap();
        }

        assert_eq!(files(dir.path()).len(), 1 + KEPT as usize);
        assert_eq!(fs::read_to_string(rotated(dir.path(), 1)).unwrap(), "6\n");
        assert_eq!(
            fs::read_to_string(rotated(dir.path(), KEPT)).unwrap(),
            "3\n"
        );
    }

    #[test]
    fn open_should_append_to_the_log_of_a_previous_run() {
        let dir = TempDir::new().unwrap();
        Sink::open(dir.path(), 100)
            .unwrap()
            .write_line("one\n")
            .unwrap();
        let mut again = Sink::open(dir.path(), 100).unwrap();
        again.write_line("two\n").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(FILE)).unwrap(),
            "one\ntwo\n"
        );
    }

    #[test]
    fn prune_should_delete_old_rotated_files_but_not_the_current_one() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(FILE), "now").unwrap();
        fs::write(rotated(dir.path(), 1), "old").unwrap();
        fs::write(rotated(dir.path(), 2), "recent").unwrap();
        let hour = Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(rotated(dir.path(), 1))
            .unwrap()
            .set_modified(SystemTime::now() - hour)
            .unwrap();
        // Made old as well, so that the current file is spared by its name, not by its age.
        File::options()
            .write(true)
            .open(dir.path().join(FILE))
            .unwrap()
            .set_modified(SystemTime::now() - hour)
            .unwrap();

        prune(dir.path(), Duration::from_secs(60));

        assert!(dir.path().join(FILE).is_file());
        assert!(!rotated(dir.path(), 1).exists());
        assert!(rotated(dir.path(), 2).is_file());
    }
}
