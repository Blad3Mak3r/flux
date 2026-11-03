# `.fxtc` project format (v1)

A `.fxtc` file is a zip archive. Implementation: `src-tauri/src/project/`.

```
project.json          deflated
audio/<file name>     stored (audio is large and usually compressed already)
```

- The `audio/` folder is flat: an asset file is `audio/<name>` with no further separators, `.` or `..`.
- Readers reject any archive entry whose name escapes the archive root (zip-slip).

## `project.json`

camelCase JSON. Every position is a **frame count** at the project frame rate: cues and LTC/MTC are
frame-aligned, and frame counts avoid rounding at 29.97 fps.

| Field | Type | Notes |
| --- | --- | --- |
| `formatVersion` | number | `1`. A file with a newer version is rejected before it is parsed further. |
| `name` | string | |
| `timecode.frameRate` | `fps24` \| `fps25` \| `fps2997Drop` \| `fps30` | The four rates MTC can express; LTC supports them too. |
| `timecode.start` | number | Frame count of the timeline origin (e.g. 01:00:00:00). |
| `assets[]` | `{ id, name, file }` | `file` is the archive path under `audio/`. |
| `tracks[]` | `{ id, name, muted, kind }` | `kind` is tagged by `type`, see below. |
| `output.ltc` | `{ deviceName, channel }` \| `null` | LTC out through an audio interface. |
| `output.mtc` | `{ portName }` \| `null` | MTC out through a MIDI port. |

Track kinds:

- `{ "type": "audio", "clips": [{ id, asset, start, gainDb }] }` — `asset` is the id of an entry in `assets`.
- `{ "type": "events", "triggers": [{ id, at, label, action }] }` — `action` is `{ "type": "marker" }` or
  `{ "type": "midi", "bytes": [...] }`.

Ids are strings chosen by the caller. They must be unique: asset ids, asset files, track ids, and clip/trigger ids
(the last two kinds share one namespace).

## Devices

Devices are machine-specific, so a project stores a **name hint** only (`deviceName`, `portName`), never an OS
identifier. Resolving the hint to a device, and the fallback when it is missing, happens when the project is opened.

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
is copied without recompressing.

## Tauri commands

Defined in `src-tauri/src/commands.rs`, typed wrappers (with the file dialogs) in `src/project/api.ts`, TS types in
`src/project/types.ts`. They run blocking I/O off the main thread and report errors as message strings.

| Command | Arguments | Result |
| --- | --- | --- |
| `project_new` | `name`, `frameRate` | A new `Project`; forgets the previous project's assets. |
| `project_open` | `path` | The `Project` read from that `.fxtc`. |
| `project_import_audio` | `path` | An `AudioAsset` (generated id, file `audio/<id>_<name>`) to add to the project. |
| `project_save` | `project`, `path` | Writes the archive; afterwards all assets are read from `path`. |

The frontend owns the edited `Project` and sends it back on save. The backend only remembers where each asset's bytes
come from (the `.fxtc` it was opened or saved from, or an imported file not yet in any archive), so saving a project
that references an asset the backend never saw fails with `MissingAsset`.
