//! Tauri commands that write the project out as files for other people: the cue sheet and the LTC
//! of a timeline as a WAV file.
//!
//! The content is built by [`flux_engine::cue_sheet`] from the same cues the Live view shows. The
//! cue lengths can need decoding audio, so the work runs on the blocking pool and never touches a
//! running output.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::commands::SharedSession;
use crate::live_commands::{active_timeline, cue_list};
use flux_audio::{AudioError, SharedAudio};
use flux_engine::cue_sheet::{self, SheetLabels, SheetTimeline};
use flux_engine::ltc_wav;
use flux_project::{DEFAULT_LTC_LEVEL, Project, ProjectError, Timeline};

/// Which timelines go into the sheet.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SheetScope {
    /// The timeline the backend has active.
    Active,
    /// Every timeline, in the order of the project.
    All,
}

/// A web page for `.html` and `.htm`; anything else is a CSV.
fn is_html(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm"))
}

/// Writes the cue sheet of `project` to `path`: HTML when the extension says so, CSV otherwise.
/// A clip whose audio cannot be read is a cue without length, never an error.
#[tauri::command]
pub async fn cue_sheet_export(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    scope: SheetScope,
    labels: SheetLabels,
    path: PathBuf,
) -> Result<(), AudioError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    let exported = tauri::async_runtime::spawn_blocking(move || {
        let timelines = match scope {
            SheetScope::Active => vec![active_timeline(&session, &project)?],
            SheetScope::All => project.timelines.iter().collect(),
        };
        let lists: Vec<_> = timelines
            .iter()
            .map(|timeline| cue_list(&project, timeline, &session, &audio))
            .collect();
        let sheet: Vec<SheetTimeline> = timelines
            .iter()
            .zip(&lists)
            .map(|(timeline, cues)| SheetTimeline {
                name: &timeline.name,
                cues,
            })
            .collect();
        let text = if is_html(&path) {
            cue_sheet::html(&project.name, &sheet, &labels)
        } else {
            cue_sheet::csv(&sheet, &labels)
        };
        std::fs::write(&path, text).map_err(ProjectError::from)?;
        Ok::<_, AudioError>(())
    })
    .await
    .map_err(|err| AudioError::Worker(io::Error::other(err).to_string()))?;
    match &exported {
        Ok(()) => log::info!("cue sheet exported"),
        Err(err) => log::warn!("cue sheet could not be exported: {err}"),
    }
    exported
}

/// A render that takes longer than this tells how far it is; a quicker one is over before anyone
/// could read a bar.
const PROGRESS_AFTER: Duration = Duration::from_secs(1);

/// What the user chose in the export dialog.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LtcOptions {
    pub sample_rate: u32,
    pub depth: ltc_wav::BitDepth,
    /// Seconds of LTC before the timeline starts, from 0 to [`ltc_wav::MAX_PRE_ROLL_SECONDS`].
    pub pre_roll_seconds: u32,
}

impl Default for LtcOptions {
    fn default() -> Self {
        let format = ltc_wav::Format::default();
        Self {
            sample_rate: format.sample_rate,
            depth: format.depth,
            pre_roll_seconds: 0,
        }
    }
}

/// Renders the LTC of `timeline` into the WAV file `path`: the pre-roll, then `frames` frames from
/// the timeline's start. `progress` hears how far it is, but only once the render has been going for
/// [`PROGRESS_AFTER`]. A file that failed is deleted, so no half-written one is left behind.
fn write_ltc(
    path: &Path,
    timeline: &Timeline,
    options: LtcOptions,
    frames: u64,
    level: f32,
    mut progress: impl FnMut(f32),
) -> Result<(), ProjectError> {
    if options.pre_roll_seconds > ltc_wav::MAX_PRE_ROLL_SECONDS
        || !ltc_wav::SAMPLE_RATES.contains(&options.sample_rate)
    {
        return Err(ProjectError::Invalid("show.error.ltcOptions".into()));
    }
    let rate = timeline.timecode.frame_rate;
    let pre_roll = ltc_wav::pre_roll_frames(rate, options.pre_roll_seconds);
    let first = ltc_wav::first_frame(rate, timeline.timecode.start, pre_roll);
    let format = ltc_wav::Format {
        sample_rate: options.sample_rate,
        depth: options.depth,
    };
    let begun = Instant::now();
    let mut out = BufWriter::new(File::create(path)?);
    let rendered = ltc_wav::write(
        &mut out,
        format,
        rate,
        first,
        pre_roll + frames,
        level,
        |fraction| {
            if begun.elapsed() >= PROGRESS_AFTER {
                progress(fraction);
            }
        },
    )
    .and_then(|_| out.flush());
    drop(out);
    rendered.map_err(|err| {
        let _ = std::fs::remove_file(path);
        match err.kind() {
            io::ErrorKind::InvalidInput => ProjectError::Invalid("show.error.ltcTooLong".into()),
            _ => err.into(),
        }
    })
}

/// Writes the LTC of the active timeline to a WAV file at `path`, from its start timecode to the end
/// of its last cue, after the pre-roll if any: mono, at the level of the project's first LTC output,
/// in the sample rate and bit depth of `options` (48 kHz, 24-bit by default). It is the
/// signal of the live output, frame for frame, rendered offline on the blocking pool without
/// touching a running output. `on_progress` hears the fraction done, only for a render that takes
/// over a second. Answers with the frames written.
#[tauri::command]
pub async fn ltc_export(
    session: State<'_, SharedSession>,
    audio: State<'_, SharedAudio>,
    project: Project,
    path: PathBuf,
    options: Option<LtcOptions>,
    on_progress: Channel<f32>,
) -> Result<u64, ProjectError> {
    let (session, audio) = (Arc::clone(&session), Arc::clone(&audio));
    let exported = tauri::async_runtime::spawn_blocking(move || {
        let timeline = active_timeline(&session, &project)?;
        let rate = timeline.timecode.frame_rate;
        let end = cue_list(&project, timeline, &session, &audio).end_samples();
        let frames = ltc_wav::frames_to_cover(end, rate);
        if frames == 0 {
            return Err(ProjectError::Invalid("show.error.ltcEmpty".into()));
        }
        let level = project
            .output
            .ltc
            .first()
            .and_then(|signal| signal.level)
            .unwrap_or(DEFAULT_LTC_LEVEL);
        write_ltc(
            &path,
            timeline,
            options.unwrap_or_default(),
            frames,
            level,
            |fraction| {
                let _ = on_progress.send(fraction);
            },
        )?;
        Ok(frames)
    })
    .await
    .map_err(|err| ProjectError::Io(io::Error::other(err)))?;
    match &exported {
        Ok(frames) => log::info!("LTC exported: {frames} frames"),
        Err(err) => log::warn!("LTC could not be exported: {err}"),
    }
    exported
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline_at_one_hour() -> Timeline {
        let mut project = Project::new("Show", flux_project::FrameRate::Fps25);
        project.timelines[0].timecode.start = 90_000;
        project.timelines.remove(0)
    }

    fn written(options: LtcOptions, frames: u64) -> u64 {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("ltc.wav");
        let mut told = 0;
        write_ltc(&path, &timeline_at_one_hour(), options, frames, 0.5, |_| {
            told += 1
        })
        .unwrap();
        assert_eq!(told, 0, "a quick render reports nothing");
        std::fs::metadata(&path).unwrap().len()
    }

    #[test]
    fn write_ltc_should_write_the_defaults_as_48_khz_24_bit() {
        // Two seconds of mono, after the 44-byte header.
        assert_eq!(written(LtcOptions::default(), 50), 44 + 2 * 48_000 * 3);
    }

    #[test]
    fn write_ltc_should_follow_the_rate_depth_and_pre_roll_chosen() {
        let options = LtcOptions {
            sample_rate: 96_000,
            depth: ltc_wav::BitDepth::Float32,
            pre_roll_seconds: 3,
        };
        // Five seconds in all: three of pre-roll and two of timeline; a float file has a 58-byte header.
        assert_eq!(written(options, 50), 58 + 5 * 96_000 * 4);
    }

    #[test]
    fn write_ltc_should_refuse_options_out_of_range_and_leave_no_file() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("ltc.wav");
        for options in [
            LtcOptions {
                sample_rate: 22_050,
                ..LtcOptions::default()
            },
            LtcOptions {
                pre_roll_seconds: 61,
                ..LtcOptions::default()
            },
        ] {
            let err =
                write_ltc(&path, &timeline_at_one_hour(), options, 50, 0.5, |_| {}).unwrap_err();
            assert!(matches!(err, ProjectError::Invalid(code) if code == "show.error.ltcOptions"));
            assert!(!path.exists());
        }
    }

    #[test]
    fn write_ltc_should_leave_no_file_when_the_timeline_is_too_long() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("ltc.wav");

        let err = write_ltc(
            &path,
            &timeline_at_one_hour(),
            LtcOptions::default(),
            9 * 3_600 * 25,
            0.5,
            |_| {},
        )
        .unwrap_err();

        assert!(matches!(err, ProjectError::Invalid(code) if code == "show.error.ltcTooLong"));
        assert!(!path.exists());
    }

    #[test]
    fn is_html_should_follow_the_extension_in_any_case() {
        assert!(is_html(Path::new("show.html")));
        assert!(is_html(Path::new("show.HTM")));
        assert!(!is_html(Path::new("show.csv")));
        assert!(!is_html(Path::new("show")));
    }
}
