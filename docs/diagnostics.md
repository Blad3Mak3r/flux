# Logging and diagnostics

Code: `src-tauri/src/diagnostics/`. A bundled app has no console, so the backend keeps its own log.

## Log file

`flux.log` lives in the app's log folder (Tauri's `app_log_dir`, identifier `com.fluxtimecode.app`):

| OS | Folder |
| --- | --- |
| Linux | `~/.local/share/com.fluxtimecode.app/logs/` |
| macOS | `~/Library/Logs/com.fluxtimecode.app/` |
| Windows | `%LOCALAPPDATA%\com.fluxtimecode.app\logs\` |

- One line per event, `2026-11-06T12:34:56.789Z INFO  message`, UTC, written and flushed at once so a crash loses nothing.
- **Size limit**: at 1 MiB the file moves to `flux.1.log` (older ones to `flux.2.log` ... `flux.4.log`, the oldest is
  dropped), so the logs never take more than about 5 MiB.
- **Age limit**: rotated files older than 14 days are deleted when the app starts.
- Level `info`; `FLUX_LOG=debug` (or `warn`, ...) before starting the app changes it. Libraries only log warnings and errors.
- If the folder cannot be used the app runs without a log and prints one line to stderr.

## What is logged

App start (version, OS, architecture); a panic (message and place); project new / open / import audio / save / delete;
every error a project command returns; output start with the resolved devices as JSON (`OutputInfo` without the
warnings) and each warning; output stop; transport state changes with the frame; an audio or LTC device lost; a device
that renders faster than real time; the show ending; and fault counts.

### Faults and the audio threads

Audio callbacks must not allocate or wait, so they never log. The stream error callbacks, `flux-mtc`, `flux-midi` and `flux-osc` count into
`SharedClock::faults`:

| Counter | Counts |
| --- | --- |
| `xruns` | `cpal::ErrorKind::Xrun`: the device under- or overran its buffer, heard as a dropout. The device stays in use. |
| `stream_errors` | Every other stream error. The device is marked lost. |
| `mtc_errors` | MIDI messages that could not be sent. |
| `cue_errors` | Messages of the MIDI and OSC cue tracks that could not be sent (`flux-midi`, `flux-osc`). |

The `flux-log` lane (`diagnostics/lane.rs`) compares them every ~33 ms and writes one line at most each second, e.g.
`3 audio dropouts, 1 MIDI messages not sent`. cpal has no counter for late callbacks beyond `Xrun`; the existing
real-time pace check (`clock_unpaced`) covers a device that runs ahead of the clock.

## Help menu

| Item | Command | Does |
| --- | --- | --- |
| Open log folder | `diagnostics_open_log_dir` | Opens the log folder in the system's file manager. |
| Export diagnostics… | `diagnostics_export(path)` | Writes a zip to `path` (the UI asks with the save dialog): `report.txt` and every log file. |

`report.txt` holds the app version, OS, export time, log folder, the graphics settings (what is saved and what this run
started the webview with), the audio devices and the MIDI output and input ports the system lists, and the `OutputInfo` of the running output
(or `not running`). The zip is written to a temporary file and renamed, so a
failed export leaves any file already there as it was.
