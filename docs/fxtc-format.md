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
| `formatVersion` | number | `3`. A file with a newer version is rejected before it is parsed further; an older one is migrated on opening (see Versioning). |
| `name` | string | |
| `meta` | object | Identity and history of the project, see below. |
| `timecode.frameRate` | `fps24` \| `fps25` \| `fps2997Drop` \| `fps30` | The four rates MTC can express; LTC supports them too. |
| `timecode.start` | number | Frame count of the timeline origin (e.g. 01:00:00:00). |
| `assets[]` | `{ id, name, file }` | `file` is the archive path under `audio/`. |
| `tracks[]` | `{ id, name, muted, gainDb?, color?, kind }` | `kind` is tagged by `type`, see below. `gainDb` is the track's fader in dB, audio tracks only; absent is 0 dB, and `-60` or less is silence (the fader's "-inf" end). It multiplies each clip's own `gainDb`. Absent in files written before it existed, without a version bump. `color` is the colour the track is drawn in, `#rrggbb` (six hex digits; anything else is rejected on open and save); absent means the default for its kind (phosphor for audio, amber for notes), also without a version bump. |
| `output.audio` | `{ deviceName, channels }` \| `null` | Interface that plays the project's audio; `channels` lists its 1-based outputs, e.g. `[1, 2]`. Absent in files written before it existed, read as `null`. |
| `output.preview` | `{ deviceName, channels }` | Optional. Interface the Edit view auditions the clips on, with its own 1-based `channels`. While it is set, Edit plays only the audio there and sends no LTC or MTC; Live always uses `output.audio`, `output.ltc` and `output.mtc`. Absent (the file leaves it out) means Edit plays like Live. Added without a version bump. |
| `output.ltc` | `[{ deviceName, channel }]` | LTC outputs, any number, each through an audio interface; `channel` is 1-based. An entry may share a device with `output.audio` (its channel is written into the programme's stream) or name another one, which gets a stream of its own, shared by every entry that names it. A repeated device and channel plays once. Absent is an empty list. |
| `output.mtc` | `[{ portName }]` | MTC outputs, any number, each through a MIDI port, each with its own thread. Absent is an empty list. |

Track kinds:

- `{ "type": "audio", "clips": [{ id, asset, start, gainDb }] }` — `asset` is the id of an entry in `assets`.
- `{ "type": "notes", "notes": [{ id, at, length?, text }] }` — text for the operator at frame `at` (counted from the
  timeline origin). `text` may have several lines. A note is shown for `length` frames (at least 1) or, when `length` is
  absent, until the next note of its track starts. Other kinds of track (video, Art-Net, MIDI keys, OSC) will be added as
  new kinds.

Ids are strings chosen by the caller. They must be unique: asset ids, asset files, track ids, and clip/note ids
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

## Project metadata

`meta` says who the project is and when it was made, apart from the show it holds.

| Field | Type | Notes |
| --- | --- | --- |
| `id` | string | A UUID, created with the project. It survives renames, copies of the file and saves, so two files with the same id are the same project. |
| `createdAt` | string | RFC 3339 UTC, e.g. `2026-11-05T14:29:03Z`. |
| `modifiedAt` | string | Same format. The time of the last save. |
| `createdWith` | string | FluxTimecode version that created the project; empty when unknown. |
| `savedWith` | string | FluxTimecode version of the last save. Empty marks a file written before `meta` existed. |
| `author` | string | Free text. Left out when empty. |
| `description` | string | Free text. Left out when empty. |

The backend sets `modifiedAt` and `savedWith` on every save and returns the project as written, so the editor's copy is
the saved one. Everything else in `meta` is the user's or the project's own; new fields go here, with a default for the
files that lack them, so they do not need a version bump.

## Versioning

`FORMAT_VERSION` is the highest version this build understands. Files with a higher version fail with
`UnsupportedVersion`. When the format changes, bump the version and migrate older files while reading: `migrate.rs`
rewrites the raw JSON before it is parsed, so the model only describes the current shape. Each step looks at the shape of
the file, not at its number, because two builds wrote a different "version 2" while the format was changing. Opening
does not rewrite a file by itself, except a file without `meta`, which is saved right away so that the id it was given
is the one it keeps. Saving writes the current version, which older builds cannot open.

- **2**: the `events` track kind was replaced by `notes`. A version 1 `marker` trigger becomes a note with its label as
  text. `midi` triggers are dropped (nothing ever sent them; MIDI cues will return as a track kind of their own), and an
  events track left with no marker stays as an empty notes track. `output.ltc` and `output.mtc` became lists; an object
  becomes a one-item list, and `null` or absent an empty list (the model reads all three shapes).
- **3**: added `meta`. A file without it gets a new `id` and a creation and modification date taken from the file's own
  modification time.

## Saving

`save` writes the archive to `<target>.tmp` and renames it over the target, so a failed save leaves the existing file
as it was. Each asset is taken from either a file on disk (newly imported) or an existing `.fxtc`, from which the entry
(and its preview) is copied without recompressing.

## File association

`bundle.fileAssociations` in `tauri.conf.json` registers `.fxtc` (`application/x-fxtc`) with Flux in the installed
bundles. A file opened from the OS arrives on the command line (Linux, Windows) or as a `RunEvent::Opened` (macOS),
is kept in `windows::PendingOpen`, and when the launcher finishes the editor opens it instead of the welcome window.
Opening a file while Flux already runs starts a second instance; the macOS event is only honoured during start-up.

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
