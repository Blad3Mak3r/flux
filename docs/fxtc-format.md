# `.fxtc` project format (v1)

A `.fxtc` file is a zip archive. Implementation: `src-tauri/src/project/`.

```
project.json          deflated
audio/<file name>     stored (audio is large and usually compressed already)
peaks/<file name>.bin stored, optional: the waveform preview of that audio (see below)
```

- The `audio/` folder is flat: an asset file is `audio/<name>` with no further separators, `.` or `..`.
- Readers reject any archive entry whose name escapes the archive root (zip-slip).

## `project.json`

camelCase JSON. Every position is a **frame count** at the project frame rate: cues and LTC/MTC are
frame-aligned, and frame counts avoid rounding at 29.97 fps. `timecode.start` is the timecode of the timeline origin;
the positions of clips (`start`) and cues (`at`) are counted **from that origin**, so a clip at `start: 0` begins at
`timecode.start`. The transport's `seek` takes a timecode, `timecode.start` included.

| Field | Type | Notes |
| --- | --- | --- |
| `formatVersion` | number | `1`. A file with a newer version is rejected before it is parsed further. |
| `name` | string | |
| `timecode.frameRate` | `fps24` \| `fps25` \| `fps2997Drop` \| `fps30` | The four rates MTC can express; LTC supports them too. |
| `timecode.start` | number | Frame count of the timeline origin (e.g. 01:00:00:00). |
| `assets[]` | `{ id, name, file }` | `file` is the archive path under `audio/`. |
| `tracks[]` | `{ id, name, muted, gainDb?, color?, kind }` | `kind` is tagged by `type`, see below. `gainDb` is the track's fader in dB, audio tracks only; absent is 0 dB, and `-60` or less is silence (the fader's "-inf" end). It multiplies each clip's own `gainDb`. Absent in files written before it existed, so `formatVersion` stays 1. `color` is the colour the track is drawn in, `#rrggbb` (six hex digits; anything else is rejected on open and save); absent means the default for its kind (phosphor for audio, amber for events), also without a version bump. |
| `output.audio` | `{ deviceName, channels }` \| `null` | Interface that plays the project's audio; `channels` lists its 1-based outputs, e.g. `[1, 2]`. Absent in files written before it existed, read as `null`. |
| `output.ltc` | `{ deviceName, channel }` \| `null` | LTC out through an audio interface; `channel` is 1-based. May share a device with `output.audio` or name another one, which gets a stream of its own. |
| `output.mtc` | `{ portName }` \| `null` | MTC out through a MIDI port. |

Track kinds:

- `{ "type": "audio", "clips": [{ id, asset, start, gainDb }] }` — `asset` is the id of an entry in `assets`.
- `{ "type": "events", "triggers": [{ id, at, label, action }] }` — `action` is `{ "type": "marker" }` or
  `{ "type": "midi", "bytes": [...] }`.

Ids are strings chosen by the caller. They must be unique: asset ids, asset files, track ids, and clip/trigger ids
(the last two kinds share one namespace). Output channels must be at least 1, and `output.audio.channels` must not
repeat a channel.

## Waveform previews (`peaks/`)

Drawing a waveform and knowing a clip's length needs the audio decoded, which is slow for a show with many songs. So
the first time an asset is analysed its preview is kept, and the next save writes it into the archive as
`peaks/<audio file name>.bin` (the name of the asset's `audio/` entry plus `.bin`). Opening the project reads the
preview instead of decoding; the audio is decoded only when it is played.

It is a **cache**: the entry is optional, `formatVersion` stays 1, readers ignore a missing one and regenerate it, and
older builds ignore it. It may be regenerated or dropped at any time. All integers are little-endian.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | Layout version of the entry, `1`. Another value is treated as missing. |
| 4 | u32 | `peaksPerSecond` (100). |
| 8 | u32 | `sourceRate`: sample rate of the file before conversion. |
| 12 | u32 | CRC-32 of the audio the preview was made from. |
| 16 | u64 | Size in bytes of that audio. |
| 24 | f64 | `durationSeconds`. |
| 32 | i8[] | `[min0, max0, min1, max1, ...]`, full scale at -128 and 127; an even count. |

The CRC-32 and size are the ones the zip directory records for the `audio/` entry, so a preview is used only while they
match the audio the archive holds: replacing the audio invalidates it, with no need to read the audio to find out. An
unreadable entry counts as missing.

When saving, a preview generated in this session is written; otherwise the entry of the archive the asset is copied
from is copied without recompressing (like the audio), and an asset that has none gets none.

## Devices

Devices are machine-specific, so a project stores a **name hint** only (`deviceName`, `portName`), never an OS
identifier. Resolving the hint to a device, and the fallback when it is missing, happens when the project is opened.
An empty `deviceName` means the system's default output device, which is what plays when a project names none.

Implementation: `src-tauri/src/devices.rs` (cpal for audio, midir for MIDI), wrappers in `src/devices/api.ts`.
Nothing is cached, so calling a list command again is the refresh after a hot-plug.

| Command | Arguments | Result |
| --- | --- | --- |
| `devices_list_audio` | | Output devices of every audio host: `{ host, name, outputChannels, sampleRates, isDefault }`. |
| `devices_list_midi` | | MIDI output ports: `{ name }`. Fails if the system has no MIDI support. |
| `devices_find_audio` | `name` | The matching audio device, or `null` when it is not connected. |
| `devices_find_midi` | `name` | The matching MIDI port, or `null` when it is not connected (or MIDI is unavailable). |

A hint matches exactly, or else ignoring case, surrounding whitespace and the `client:port` numbers ALSA appends to
MIDI port names. A missing device is `null`, never an error: the UI asks for a replacement and opening a project
never fails because of hardware.

## Versioning

`FORMAT_VERSION` is the highest version this build understands. Files with a higher version fail with
`UnsupportedVersion`. When the format changes, bump the version and migrate older files while reading.

## Saving

`save` writes the archive to `<target>.tmp` and renames it over the target, so a failed save leaves the existing file
as it was. Each asset is taken from either a file on disk (newly imported) or an existing `.fxtc`, from which the entry
(and its preview) is copied without recompressing.

## Tauri commands

Defined in `src-tauri/src/commands.rs`, typed wrappers (with the file dialogs) in `src/project/api.ts`, TS types in
`src/project/types.ts`. They run blocking I/O off the main thread and report errors as message strings.

| Command | Arguments | Result |
| --- | --- | --- |
| `project_document` | | The `Document` being edited (`project`, `path`, `baseline`), or `null` before one was started. |
| `project_sync` | `project` | Keeps the latest state of the project being edited, for `project_document`. |
| `project_new` | `name`, `frameRate` | A new `Project`; forgets the previous project's assets. |
| `project_open` | `path` | The `Project` read from that `.fxtc`. |
| `project_import_audio` | `path` | An `AudioAsset` (generated id, file `audio/<id>_<name>`) to add to the project. |
| `project_save` | `project`, `path` | Writes the archive; afterwards all assets are read from `path`. |

The Live view's commands (`live_cues`, `live_state`) read the project but write nothing, and cues are derived, not
stored, so they add nothing to `project.json`; they are described in [timecode-signals.md](timecode-signals.md).

The frontend edits the `Project` and sends each state of it with `project_sync` (the newest wins while one is being
written). The backend keeps the latest as the `Document`, so that a window that is reloaded asks `project_document` and
picks the project up where it was left, instead of starting over: `baseline` is the project as it was last opened or
saved (as it started, for a new one) and the document is unsaved when `project` differs from it. Undo history, the
selection and the zoom are the window's and do not survive a reload; the waveforms are read again, from the stored
previews or the decoded audio. Starting the window also stops an output a previous load left running.

The editor window opens on, in this order: the document the backend already holds (a reload), the show named by
`?open=` in its address (what the welcome window asks for; the address keeps it on a reload, which is why the backend's
copy, with the edits made since, comes first), or a new project. The editor window is built once per run, so a document
the backend holds is always that window's own.

The backend also remembers where each asset's bytes come from (the `.fxtc` it was opened or saved from, or an imported
file not yet in any archive), so saving a project that references an asset the backend never saw fails with
`MissingAsset`.
