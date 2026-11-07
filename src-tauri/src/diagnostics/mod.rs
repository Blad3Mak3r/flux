//! The log file and the diagnostics the user can reach from the Help menu.

mod lane;
mod logger;

pub use lane::LogLane;

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};
use tauri::{AppHandle, Manager, State};
use thiserror::Error;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::transport_commands::{SharedOutput, running_info};
use flux_engine::devices;
use flux_engine::output::OutputInfo;

/// Starts the log file and writes the first line. When the log folder cannot be used the app
/// goes on without a log: a show must not fail to start because of it.
pub fn start(app: &AppHandle) {
    let started = app
        .path()
        .app_log_dir()
        .map_err(|err| err.to_string())
        .and_then(|dir| logger::init(&dir).map_err(|err| err.to_string()));
    match started {
        Ok(()) => log::info!(
            "flux {} started on {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        Err(err) => eprintln!("flux: no log file, {err}"),
    }
}

/// Logs the output that was just opened: where each part ended up, and what did not open.
pub fn output_started(info: &OutputInfo) {
    log_output("output started", info);
}

/// Logs how the output ended up after a change of the settings that did not reopen it.
pub fn output_reconfigured(info: &OutputInfo) {
    log_output("output reconfigured", info);
}

fn log_output(what: &str, info: &OutputInfo) {
    let resolved = OutputInfo {
        warnings: Vec::new(),
        ..info.clone()
    };
    match serde_json::to_string(&resolved) {
        Ok(json) => log::info!("{what}: {json}"),
        Err(err) => log::info!("{what} on {} ({err})", info.device),
    }
    log_warnings(&info.warnings);
}

/// Logs what did not stop the output but the user should know about.
pub fn log_warnings(warnings: &[String]) {
    for warning in warnings {
        log::warn!("output warning: {warning}");
    }
}

#[derive(Debug, Error)]
pub enum DiagnosticsError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("could not write the archive: {0}")]
    Zip(#[from] ZipError),
    #[error("{0}")]
    Other(String),
}

impl Serialize for DiagnosticsError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

fn log_dir(app: &AppHandle) -> Result<PathBuf, DiagnosticsError> {
    app.path()
        .app_log_dir()
        .map_err(|err| DiagnosticsError::Other(err.to_string()))
}

/// Opens the folder the log files are in with the system's file manager.
#[tauri::command]
pub async fn diagnostics_open_log_dir(app: AppHandle) -> Result<(), DiagnosticsError> {
    let dir = log_dir(&app)?;
    fs::create_dir_all(&dir)?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|err| DiagnosticsError::Other(err.to_string()))
}

/// Writes a zip with the log files and a report of the system, the devices and the running output
/// to `path`, for the user to send along with a bug report.
#[tauri::command]
pub async fn diagnostics_export(
    app: AppHandle,
    output: State<'_, SharedOutput>,
    path: PathBuf,
) -> Result<(), DiagnosticsError> {
    let dir = log_dir(&app)?;
    let output = running_info(&output);
    let exported = tauri::async_runtime::spawn_blocking(move || {
        let report = report(&dir, output.as_ref());
        write_bundle(&path, &logger::files(&dir), &report)
    })
    .await
    .map_err(|err| DiagnosticsError::Other(err.to_string()))?;
    match &exported {
        Ok(()) => log::info!("diagnostics exported"),
        Err(err) => log::warn!("diagnostics could not be exported: {err}"),
    }
    exported
}

/// What a bug report needs besides the log: the app, the system, the devices and the output.
fn report(log_dir: &Path, output: Option<&OutputInfo>) -> String {
    fn json<T: Serialize>(value: &T) -> String {
        serde_json::to_string_pretty(value).unwrap_or_else(|err| format!("unavailable ({err})"))
    }
    let midi = devices::list_midi()
        .map_or_else(|err| format!("unavailable ({err})"), |ports| json(&ports));
    let midi_in = devices::list_midi_inputs()
        .map_or_else(|err| format!("unavailable ({err})"), |ports| json(&ports));
    format!(
        "Flux Timecode {}\nSystem: {} {}\nExported: {}\nLog folder: {}\n\n\
         Graphics:\n{}\n\nAudio drivers:\n{}\n\nAudio devices (default driver):\n{}\n\nMIDI ports:\n{midi}\n\nMIDI input ports:\n{midi_in}\n\nOutput:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        logger::now(),
        log_dir.display(),
        json(&crate::graphics::state()),
        json(&devices::list_hosts()),
        json(&devices::list_audio(None)),
        output.map_or_else(|| "not running".to_owned(), json),
    )
}

/// Writes the zip to a temporary file next to `path` and renames it over, so a failed export
/// leaves any file already there untouched.
fn write_bundle(path: &Path, logs: &[PathBuf], report: &str) -> Result<(), DiagnosticsError> {
    let tmp = path.with_extension("zip.tmp");
    let written = write_zip(&tmp, logs, report).and_then(|()| Ok(fs::rename(&tmp, path)?));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

fn write_zip(path: &Path, logs: &[PathBuf], report: &str) -> Result<(), DiagnosticsError> {
    let mut zip = ZipWriter::new(File::create(path)?);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("report.txt", options)?;
    zip.write_all(report.as_bytes())?;
    for log in logs {
        let name = log
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("flux.log");
        zip.start_file(name, options)?;
        io::copy(&mut File::open(log)?, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use tempfile::TempDir;
    use zip::ZipArchive;

    use super::*;

    #[test]
    fn write_bundle_should_hold_the_report_and_every_log() {
        let dir = TempDir::new().unwrap();
        let (one, two) = (dir.path().join("flux.log"), dir.path().join("flux.1.log"));
        fs::write(&one, "new").unwrap();
        fs::write(&two, "old").unwrap();
        let out = dir.path().join("diagnostics.zip");

        write_bundle(&out, &[one, two], "the report").unwrap();

        let mut zip = ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let mut read = |name: &str| {
            let mut text = String::new();
            zip.by_name(name)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            text
        };
        assert_eq!(read("report.txt"), "the report");
        assert_eq!(read("flux.log"), "new");
        assert_eq!(read("flux.1.log"), "old");
        assert!(!dir.path().join("diagnostics.zip.tmp").exists());
    }

    #[test]
    fn write_bundle_should_leave_an_existing_file_when_a_log_cannot_be_read() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("diagnostics.zip");
        fs::write(&out, "previous").unwrap();

        let missing = dir.path().join("missing.log");
        assert!(write_bundle(&out, &[missing], "report").is_err());

        assert_eq!(fs::read_to_string(&out).unwrap(), "previous");
        assert!(!dir.path().join("diagnostics.zip.tmp").exists());
    }
}
