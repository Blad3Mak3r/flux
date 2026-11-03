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
| `output.audio` | `{ deviceName, channels }` \| `null` | Interface that plays the project's audio; `channels` lists its 1-based outputs, e.g. `[1, 2]`. Absent in files written before it existed, read as `null`. |
| `output.ltc` | `{ deviceName, channel }` \| `null` | LTC out through an audio interface; `channel` is 1-based. May share a device with `output.audio`. |
| `output.mtc` | `{ portName }` \| `null` | MTC out through a MIDI port. |

Track kinds:

- `{ "type": "audio", "clips": [{ id, asset, start, gainDb }] }` — `asset` is the id of an entry in `assets`.
- `{ "type": "events", "triggers": [{ id, at, label, action }] }` — `action` is `{ "type": "marker" }` or
  `{ "type": "midi", "bytes": [...] }`.

Ids are strings chosen by the caller. They must be unique: asset ids, asset files, track ids, and clip/trigger ids
(the last two kinds share one namespace). Output channels must be at least 1, and `output.audio.channels` must not
repeat a channel.

## Devices

Devices are machine-specific, so a project stores a **name hint** only (`deviceName`, `portName`), never an OS
identifier. Resolving the hint to a device, and the fallback when it is missing, happens when the project is opened.

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
