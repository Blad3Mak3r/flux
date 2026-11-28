# `.flx` / `.fxtc` project format (v16)

A `.flx` file is a zip archive. Legacy `.fxtc` files use exactly the same format. Implementation: `src-tauri/crates/flux-project/src/`.

New projects and Save as use `.flx` by default. Opening accepts either extension, case-insensitively. Saving an existing project preserves its path and extension, so legacy projects stay `.fxtc` unless saved as a new `.flx` file. Changing the extension requires no archive migration or format-version bump.

```
project.json          deflated
audio/<file name>     stored, only for audio kept inside the archive (see Linked media)
peaks/<file name>.bin stored, optional: the waveform preview of that audio (see below)
video/<file name>     stored, only for videos and images kept inside the archive
```

Media is **linked** by default: a new `.flx` holds `project.json` (and the previews) and the audio, videos and images stay
where they are on disk, referred to by path (see [Linked media](#linked-media-version-16)). A project can weigh a few KB
next to videos of several GB. The `audio/` and `video/` entries exist only for **embedded** media: projects written
before version 16, generated speech, and what a project holds after it was opened from such a file.

- The `audio/` and `video/` folders are flat: an asset file is `audio/<name>` (a video `video/<name>`) with no further
  separators, `.` or `..`.
- Readers reject any archive entry whose name escapes the archive root (zip-slip).

## `project.json`

camelCase JSON. A project holds one or more **timelines** (a song, a block of the show), each with its own timecode and
tracks; the audio files and logical output signals belong to the project and are shared. Only one timeline plays at a time:
the backend keeps which one is active, and it is not stored in the file.

Every position is a **frame count** at the frame rate of its timeline: cues and LTC/MTC are frame-aligned, and frame
counts avoid rounding at 29.97 fps. `timecode.start` is the timecode of the timeline origin; the positions of clips
(`start`) and cues (`at`) are counted **from that origin**, so a clip at `start: 0` begins at `timecode.start`. The
transport's `seek` takes a timecode, `timecode.start` included.

| Field | Type | Notes |
| --- | --- | --- |
| `formatVersion` | number | `14`. A file with a newer version is rejected before it is parsed further; an older one is migrated on opening (see Versioning). |
| `name` | string | |
| `meta` | object | Identity and history of the project, see below. |
| `autosave` | `{ enabled, intervalSeconds }` | Project-owned recovery preferences. Defaults to enabled, every 30 seconds; supported intervals are 15, 30 and 60 seconds. Also saves after 2 seconds without edits. Older projects receive these defaults on migration. |
| `assets[]` | `{ id, name, file, link?, bpm? }` | `file` is the archive path under `audio/` (the key of the asset; the bytes are there only when `link` is absent). Shared by all timelines; an asset no clip plays is dropped when the last clip using it goes. `bpm` is the tempo found in the audio (or typed by the user), 20 to 400; absent when never detected. Added without a version bump. |
| `videos[]` | `{ id, name, file, link? }` | The video files (and images) the video tracks and the grid show. `file` is the archive path under `video/`, the key of the asset. Ids share one namespace with `assets` (an audio clip cannot point at a video, nor a video clip at audio), and files are unique across both. A video no clip shows is dropped on save. Absent is an empty list. Added in version 13. |
| `mode` | `timeline` \| `vj` | The kind of project, chosen when it is created and never changed: a timeline project plays its timelines, a VJ project its VJ grid. Absent is `timeline`. Added in version 14. |
| `vj` | `{ layers, decks, deck }` | The VJ grid, see [VJ grid](#vj-grid-version-14). Present in a VJ project only; a timeline project with a grid is rejected. Added in version 14. |
| `timelines[]` | `{ id, name, timecode, tempo, tracks }` | Never empty. `id` is unique among the timelines; `name` is free text. |
| `timelines[].tempo` | `{ bpm, beatsPerBar, click }` | The beat grid of the timeline, counted from its origin: `bpm` 20 to 400, `beatsPerBar` 1 to 16 (the beat is a quarter note), `click` whether the metronome plays while this timeline runs. Absent in files written before it existed, read as 120 BPM, 4 beats, click off, without a version bump. Which bars the ruler shows is a view setting of the window and is not stored. |
| `timelines[].timecode.frameRate` | `fps24` \| `fps25` \| `fps2997Drop` \| `fps30` | The four rates MTC can express; LTC supports them too. |
| `timelines[].timecode.start` | number | Frame count of the timeline origin (e.g. 01:00:00:00). |
| `timelines[].tracks[]` | `{ id, name, muted, gainDb?, color?, kind }` | Track, clip and note ids are unique in the **whole project**, not per timeline. `kind` is tagged by `type`, see below. `gainDb` is the track's fader in dB, audio tracks only; absent is 0 dB, and `-60` or less is silence (the fader's "-inf" end). It multiplies each clip's own `gainDb`. Absent in files written before it existed, without a version bump. `color` is the colour the track is drawn in, `#rrggbb` (six hex digits; anything else is rejected on open and save); absent means the default for its kind (phosphor for audio, amber for notes), also without a version bump. |
| `output.metronome` | `{ gainDb }` | Optional logical click output. The project owns its level; each timeline owns tempo and click activation. The computer assigns its interface and channels. |
| `output.ltc` | `[{ slot, level? }]` | Logical LTC outputs. `slot` is a unique positive integer within LTC, preserved when other outputs are removed. Level is linear 0–1; absent is 0.5. Device, channel and latency are local preferences. |
| `output.mtc` | `[{ slot }]` | Logical MTC outputs with unique positive slots, independent of LTC slots. The computer assigns the MIDI port, same-name port index and latency. |
| `output.midiClock` | `true` | The project sends MIDI clock; absent is off. The port is a local preference. Added without a version bump. |
| `output.osc` | `[{ id, name, host, port }]` | The OSC receivers (UDP) the OSC tracks send to, defined once for the project. `id` is unique among them; `host` is a name or an address without spaces, `port` 1 to 65535. A host that does not resolve is reported when the output opens and never blocks the project. Absent is an empty list. Added in version 5. |
| `output.composition` | `{ width, height }` | The size in pixels of the picture every video clip is sized into (by its `fit`) and the slices of `output.screens` are cut from, each side 16 to 8192. Absent is the size of the first entry of `videos`, or 1920 × 1080 without one. |
| `output.screens` | `[{ id, name, slices?: [{ id, name, input: { x, y, width, height }, corners: [{ x, y } ×4], hidden?, softEdge?: { left, right, top, bottom, gamma }, testCard?: { show, cabinetWidth, cabinetHeight, numbering, scheme, guideWidth, showName, showResolution, labelPlate, labelY, base, layers, mode, custom, showLogo, logo, logoBack, logoSize } }], masks?: [{ id, name, points: [{ x, y }], invert?, hidden? }], enabled? }]` | The screens of the video output, at most 8, each a window on a display of its own. The screen `main` always exists: when it is not listed it has no slices and is named "Screen 1". `id` is unique and `name` 1 to 80 characters; `enabled` (absent: true, no format change) false makes the screen's window show black; the display each opens on is a local preference. `slices` are drawn in order. `input` is the part of the picture shown, inside it and at least 0.005 on each side; `corners` are where its top left, top right, bottom right and bottom left go on the screen, each from −1 to 2. All numbers are fractions of the width or height from the top left. `id` is unique, `name` 1 to 80 characters, at most 64 slices. No slices is the whole picture fitted to the screen. `testCard` (absent: the defaults, no format change) sets the slice's test card: cabinet sides 8 to 2048 pixels (default 128), `numbering` `alpha`/`dotted`/`sequence`, `scheme` `vivid`/`dark`/`light`/`contrast`, `guideWidth` 1 to 12, `labelY` 0 to 100, `base` and `custom` as `#rrggbb` (`base` empty: a colour from the slice's place), `mode` `pattern`/`red`/`green`/`blue`/`white`/`black`/`custom`, `showLogo` (default false) with `logo`, the id of an image in `videos` (empty: none), `logoBack` as `#rrggbb` (default `#10131a`) and `logoSize` 10 to 60 (percent of the slice's short side, default 24), and `layers` booleans `grid`, `numbers`, `circle`, `diagonals`, `crosshair`, `label`, `bars`, `motion`; see [the test card](./vj-mode.md#the-test-card). `softEdge` sides are fractions 0 to 1 of the slice (absent: hard edges) and `gamma` 0.2 to 5 (default 1). `masks` (at most 8, 3 to 32 points each, within 0 to 1) black out their shape over the screen's slices, or everything else when `invert`. |

Track kinds:

- `{ "type": "audio", "clips": [{ id, asset, start, gainDb, name?, offset?, length?, fadeIn?, fadeOut?, fadeInCurve?, fadeOutCurve?, envelope? }] }` —
  `asset` is the id of an entry in `assets`. `name` is the name the clip goes by in the timeline and in Live (1 to 64
  characters, not blank, anything else is rejected on open and save); absent means the asset's name. Added without a
  version bump. The optional fields below trim and fade the clip, are counted in **frames at the
  timeline's rate** like `start`, and are added without a version bump (a clip without them plays the whole asset with a
  hard start and cut):
  - `offset`: frames of the asset skipped at the start of the clip (trim in). Absent is 0.
  - `length`: frames of the asset the clip plays (trim out), at least 1. Absent means up to the end of the asset; one
    that reaches past it is cut at the end of the asset. The app always writes it when `offset` is not 0.
  - `fadeIn` / `fadeOut`: frames to rise from silence at the start / fall to silence at the end. Absent is 0.
    They are kept inside the clip, and `fadeOut` gives way to `fadeIn`.
  - `fadeInCurve` / `fadeOutCurve`: the shape of each fade, one of `"linear"` (a straight line in amplitude; absent
    means this), `"equalPower"` (a quarter sine, `sin(x·π/2)`), `"sCurve"` (`0.5 − 0.5·cos(x·π)`) or `"exponential"`
    (`x²`), where `x` is how far up from silence the fade is, 0 to 1; a fade out is read from its end. Added without a
    version bump; any other value is rejected on open. When a clip is split, each half keeps the curve of the fade
    it keeps.
  - `envelope`: the volume drawn over the clip, `[{ at, gainDb, curve? }]`. It multiplies `gainDb` and the fades.
    `at` is in **thousandths of a frame counted from the start of the audio file** (like `start - offset`, not from
    the clip), so moving or trimming a clip never rewrites its points, and a clip split in two gives each half the same
    points, which sound sample for sample as before. `gainDb` runs from `-60` (silence) to `+6`. `curve` shapes the
    line from this point to the next, one of `"linear"` (absent means this), `"smooth"`, `"fast"`, `"slow"` or
    `"hold"` (keeps the gain, then steps to the next point just before it, with a few milliseconds of ramp). Points
    are interpolated in dB, except when one is silence, which is interpolated in amplitude. Before the first point and
    after the last, the gain is that of the nearest. Up to 256 points, in strictly increasing order of `at`; anything
    else is rejected on open and save. Absent or empty means no envelope. Added without a version bump.

  `start` is where the clip sounds; the asset's first frame would sit at `start - offset`. A frame is rarely a whole
  number of samples (29.97 fps), so the player converts that one frame to samples and reads the asset from there. Two
  clips cut from one another share that frame, so a clip split in two plays back sample for sample like the original.
- `{ "type": "notes", "notes": [{ id, at, length?, text }] }` — text for the operator at frame `at` (counted from the
  timeline origin). `text` may have several lines. A note is shown for `length` frames (at least 1) or, when `length` is
  absent, until the next note of its track starts.
- `{ "type": "midi", "port"?: { portName, portIndex? }, "events": [{ id, at, message }] }` — MIDI messages sent at frame
  `at` to the track's port, resolved by name like the MTC ports (`portIndex` counts from 0 among ports of the same
  name). A track with no `port`, or whose port is not connected, sends nothing and is reported as a warning. `message`
  is tagged by `type`; channels are **1 to 16**, every other number 0 to 127:
  - `noteOn { channel, note, velocity, length? }`: with `length` (frames, at least 1) the note is released by itself
    that many frames later.
  - `noteOff { channel, note }`, `controlChange { channel, controller, value }`, `programChange { channel, program }`.
  - `msc { deviceId, format, command: "go" | "stop" | "resume", cue?, list? }`: MIDI Show Control, sent as
    `F0 7F <deviceId> 02 <format> <command> <cue> 00 <list> F7`. `format` is 1 to 127 (1 lighting, 127 every kind);
    `cue` and `list` are digits and dots, and a `list` needs a `cue`.
  - `sysEx { data }`: the bytes between `F0` and `F7`, each below `0x80`, at least one.
- `{ "type": "osc", "destination"?: "<id of output.osc>", "events": [{ id, at, address, args? }] }` — OSC messages sent
  at frame `at` to the track's destination. `address` starts with `/` and has no spaces. `args` are tagged
  `{ "type": "int" | "float" | "string" | "bool", "value": … }` and sent as `i`, `f`, `s`, `T`/`F`. A destination that
  is not in `output.osc` is rejected on open and save; one that is absent sends nothing.

- `{ "type": "video", "clips": [{ id, asset, start, name?, offset?, length?, fit?, fadeIn?, fadeOut? }] }` — video shown in time with the
  timeline. `asset` is the id of an entry in `videos`; `name`, `offset` and `length` work as for audio clips (frames at
  the timeline's rate; `length` at least 1, absent means up to the end of the video). Clip ids share the namespace of
  clip and note ids. `fit` is how the picture is sized into the composition (`output.composition`): `fit` (the
  default, written as absent) fits it whole keeping its shape, `fill` covers the composition keeping its shape and crops
  the rest, `stretch` takes exactly the composition, `none` keeps the video's own size in pixels; each is centred and
  the composition outside it shows the tracks below, or black. `fadeIn` and `fadeOut` are frames, as for audio clips
  (absent is 0): the picture and its sound rise from nothing over the first and fall to nothing over the last. Added in version 13. The clips play on the screens of `output.screens` in time with the clock,
  their sound plays through the track's routing, and Live lists each clip as a cue (see [Video](video-output.md)).

Other kinds of track (Art-Net) will be added as new kinds. Events are kept in order of `at`; event ids share the
namespace of clip and note ids.

Ids are strings chosen by the caller. They must be unique: asset ids, asset files, track ids, and clip/note ids
(the last two kinds share one namespace). Local output channels must be at least 1, and audio channel lists must not
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

Version 6 projects store logical signals, not operational hardware routes. `preferences.json` in the app data
folder stores the computer's driver, programme and Edit-preview routes, metronome route, LTC and MTC routes
keyed by logical slot, and `locale` (`en` or `es`, English by default). Routes are shared by every project on that
computer. Changing one does not dirty a project or enter undo history; hardware changes wait for pause or stop.
Names are hints, never OS identifiers. An empty audio name explicitly selects the system default. Missing or
unassigned LTC/MTC routes never fall back to another device or port.

For older archives, migration preserves physical settings under optional `output.legacyRoutes` in their previous
shape, converts the ordered outputs into slots starting at 1, and keeps signal levels. These hints never affect
playback until the user reviews and confirms **Import assignments from this project**. Import overwrites only
included routes, preserving other local routes and language. Opening an archive never changes local routing.

The Rust commands `preferences_get`, `preferences_change` and `preferences_resolve` own persistence, validation
and signal resolution. Changes are broadcast as `preferences-changed` across windows. Project signal edits use
`output_signal_edit`, return project state and enter undo history. Graphics remain in their existing separate file.

Implementation: `src-tauri/crates/flux-engine/src/devices.rs` (cpal for audio, midir for MIDI), wrappers in `src/devices/api.ts`.
Nothing is cached, so calling a list command again is the refresh after a hot-plug.

| Command | Arguments | Result |
| --- | --- | --- |
| `devices_list_hosts` | | Audio drivers (APIs) available on this machine, the default first: `{ name, isDefault }`. |
| `devices_list_audio` | `driver?` | Output devices of the audio driver `driver` names (absent: the default one), one per physical output: `{ name, outputChannels, sampleRates, isDefault }`. On ALSA only `plughw:` devices are listed (each name once), not the `hw:`, `front:`, `surround*:`, `hdmi:`, `pipewire`, `pulse` ... variants. |
| `devices_list_midi` | | MIDI output ports: `{ name }`. Fails if the system has no MIDI support. |
| `devices_find_audio` | `name`, `driver?` | The matching audio device, or `null` when it is not connected. |
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
| `createdWith` | string | FLUX version that created the project; empty when unknown. |
| `savedWith` | string | FLUX version of the last save. Empty marks a file written before `meta` existed. |
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
does not rewrite a file, including files without `meta`; their generated identity is persisted on the next explicit save. Saving writes the current version, which older builds cannot open.

- **2**: the `events` track kind was replaced by `notes`. A version 1 `marker` trigger becomes a note with its label as
  text. `midi` triggers are dropped (nothing ever sent them; MIDI cues will return as a track kind of their own), and an
  events track left with no marker stays as an empty notes track. `output.ltc` and `output.mtc` became lists; an object
  becomes a one-item list, and `null` or absent an empty list (the model reads all three shapes).
- **3**: added `meta`. A file without it gets a new `id` and a creation and modification date taken from the file's own
  modification time.
- **5**: added the `midi` and `osc` track kinds and `output.osc`. Nothing was reshaped, so there is no migration; the
  bump makes an older build say the file is newer instead of failing on an unknown track kind.
- **4**: `timecode` and `tracks` moved from the top level into `timelines`. An older file becomes a project with one
  timeline, named like the project, with a new `id`. The listing of recent projects reads the frame rate of the first
  timeline and the tracks of all of them (it also reads the older top-level shape).
- **6**: split portable logical signals from physical routes. `output.ltc` and `output.mtc` identify slots;
  metronome retains only its level. Previous routes survive as explicit-import hints. Saving in version 6 makes
  the archive unreadable by builds supporting version 5 or earlier; merely opening it does not rewrite it.
- **7**: added `autosave`, owned by the project. Older files receive `{ "enabled": true, "intervalSeconds": 30 }`.
- **8**: added operator tracks, spoken orders and inherited voice selections. Older projects have no operator tracks or explicit voice selections.
- **9**: added recording origins, pronunciation overrides and non-destructive operator trims; earlier recordings default to generated origin and zero trims.
- **14**: added `mode` and the VJ grid `vj`. Nothing was reshaped, so there is no migration.
- **16**: added `link` to audio and video assets. A file without it holds its media in the archive, as before, and opens and
  saves that way; there is no migration. See [Linked media](#linked-media-version-16).

## Saving

`save` writes the archive to `<target>.tmp` and renames it over the target, so a failed save leaves the existing file
as it was. An embedded asset is taken from either a file on disk or an existing `.fxtc`, from which the entry (and its
preview) is copied without recompressing. A linked asset writes **no media**: only its preview (the one made in this
session, or the one in the file the project was opened or saved as), and its `link.relative` is worked out again
against the folder of the file being written.

## Linked media (version 16)

`link` is `{ path, relative? }` on an audio asset or a video (or image) asset:

| Field | Notes |
| --- | --- |
| `path` | Absolute, as the system gave it when the file was imported or relinked. |
| `relative` | Relative to the folder of the project file, `/`-separated, written at every save; absent when it cannot be said (another drive). |

- Every import (audio, video, VJ video, image) links the file where it is. Nothing is copied into the project. Generated
  speech (the voice operator) stays embedded.
- **Opening never fails for missing media.** For each linked asset: if `path` exists it is online; otherwise, if
  `relative` resolves against the project file's folder to an existing file, `path` becomes that (the project is not
  marked changed); otherwise the asset is **offline**. An offline asset is skipped alone: its clips draw as offline, play
  nothing and show black, and the rest of the show, the open and the save are unaffected. Its waveform and preview
  entries stay in the archive.
- Offline media is found again with **Media** (File > Media…, or the offline count in the status bar): Relink on one
  item (which also relinks every other offline item whose file name is in that same folder), Search folder (a recursive
  search by file name, case-insensitive, down to 8 folders deep and 50 000 entries, skipping hidden folders), or by
  hand. Each is one undo step. A relinked file must be the same kind of media (audio, video or image) by extension.
- **Collect media** (needs a saved project) copies every linked file, and extracts every embedded one except generated
  speech, into `<project name> Media/` beside the project file (a name that is taken gets ` (2)`, ` (3)`...), and links
  them there with a relative path. Offline items are skipped. It changes the project; the next save leaves the
  embedded entries out. A linked file already in that folder is not copied.
- A preview of linked audio is stamped with the file's size and modification time instead of the CRC of an archive
  entry, so it is regenerated when the file changes.
- A HAP copy of a linked video is written beside it (`<name> (HAP).mov`) and linked; when that folder cannot be written
  the conversion fails.
- The commands are `media_list`, `media_relink`, `media_search_folder` and `media_collect` (`src-tauri/src/media_commands.rs`,
  logic in `flux-project/src/media.rs`). `media_collect` copies without the session locked and reports
  `media-collect-progress` `{ doneBytes, totalBytes }`.

## Autosave and crash recovery

The dedicated `flux-recovery` worker saves dirty projects after two seconds without edits or after the project's
interval during continuous editing. It copies a session snapshot under its mutex, then releases it before serialization
and disk I/O. No recovery work runs on audio callbacks or engine lanes. Unchanged projects are not rewritten.

Recovery copies live under the app data directory's `recovery/`, not next to the user's file. Each session has an opaque
UUID filename. Its version-1 JSON holds `id`, `savedAt`, the document (`project`, original `path` or null, `baseline`,
`active`) and an asset-id map of sources: `{ "kind": "archive" | "disk", "path": "<absolute path>" }`.
The project retains `meta.id`; audio bytes, waveform previews and undo history are never included. Writes flush and sync
the temporary JSON before renaming it over the previous copy. The original `.fxtc` is never touched by autosave.

An OS lock on a companion `.lock` file keeps live sessions out of the recovery list and prevents two processes from
restoring the same copy. A killed process releases the lock automatically. Welcome lists abandoned copies with their
dates and offers Restore, Discard (with confirmation), or Open original. Opening the original removes the recovery only
after successfully opening the file and creating the editor. Pending file-association requests remain available there.

Restoration keeps the unsaved baseline and reads embedded assets from their original archives, newly imported assets
from their original disk paths. Missing sources are reported by the audio-loading path without rejecting the restored
document. Saving still requires the referenced audio to be available. Recovery is not an audio backup.

Successful explicit save, confirmed close, project replacement or restart removes that session's recovery. A failed save
or cancelled close preserves it. Disabling autosave stops new writes but preserves any existing copy. Options are edited
in Preferences > Project, enter undo history and are stored in both the `.fxtc` and recovery JSON.

## File association

`bundle.fileAssociations` in `tauri.conf.json` registers `.flx` (`application/x-flx`) and legacy `.fxtc`
(`application/x-fxtc`) with FLUX in the installed
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
| `project_new` | `name`, `frameRate`, `mode?` | A new `Project` of that kind (`timeline` when absent), which it keeps for good; forgets the previous project's assets. |
| `project_open` | `path` | The `Project` read from that `.fxtc`. |
| `project_import_audio` | `path` | An `AudioAsset` (generated id, file `audio/<id>_<name>`, `link` to `path`) to add to the project. |
| `media_list` | `project` | `{ items: [{ id, name, kind, state, path, uses }], missing }`: every media file, offline first (`kind` `audio`/`video`/`image`, `state` `online`/`offline`/`embedded`). |
| `media_relink` | `project`, `id`, `path` | `{ state, found, missing }`: links the item and the other offline items found by name in that folder. |
| `media_search_folder` | `project`, `folder` | `{ state, found, missing }`: searches the folder and those under it. |
| `media_collect` | `project` | `{ state, copied, skipped }`: copies or extracts the media beside the project file (see Linked media). Emits `media-collect-progress`. |
| `project_save` | `project`, `path` | Writes the archive; afterwards all assets are read from `path`. |
| `project_set_autosave` | `project`, `enabled`, `intervalSeconds` (text) | Validates the interval in Rust and returns `ProjectState` for the undo history. |
| `recovery_list` | | Recovery summaries, newest first; corrupt/incompatible records remain discardable. |
| `recovery_discard` | `id` | Removes an abandoned recovery after confirmation. |
| `window_restore_recovery` | `id` | Installs the recovery in the session and opens the editor. |
| `window_open_original` | `id` | Opens the original and discards the recovery only on success. |
| `project_close` | | Clears the session and its recovery before the editor window is destroyed. |
| `cue_edit` | `project`, `op` | Edits the MIDI and OSC tracks and the OSC destinations. `op` is tagged by `op`: `addTrack`, `addEvent` (a valid default event; the track is picked or created), `setMidi`, `setOsc`, `moveEvent`, `removeEvent`, `setMidiPort`, `setOscDestination`, `addDestination`, `updateDestination`, `removeDestination` (tracks that sent to it lose the destination). The backend validates, keeps events in order and answers with the project, the active timeline and the `id` it created, or the reason it refused; a refusal leaves the document as it was. |

The Live view's commands (`live_cues`, `live_state`) read the project but write nothing, and cues are derived, not
stored, so they add nothing to `project.json`; they are described in [timecode-signals.md](timecode-signals.md).

The frontend edits the `Project` and sends each state of it with `project_sync` (the newest wins while one is being
written). The backend keeps the latest as the `Document`, so that a window that is reloaded asks `project_document` and
picks the project up where it was left, instead of starting over: `baseline` is the project as it was last opened or
saved (as it started, for a new one) and the document is unsaved when `project` differs from it. Undo history, the
selection and the zoom are the window's and do not survive a reload; the waveforms are read again, from the stored
previews or the decoded audio. The reloaded window subscribes to the existing output, so playback and timecode continue.

The editor window opens on, in this order: the document the backend already holds (a reload), the show named by
`?open=` in its address (what the welcome window asks for; the address keeps it on a reload, which is why the backend's
copy, with the edits made since, comes first), or a new project. The editor window is built once per run, so a document
the backend holds is always that window's own.

The backend also remembers where each asset's bytes come from (the `.fxtc` it was opened or saved from, or an imported
file not yet in any archive), so saving a project that references an asset the backend never saw fails with
`MissingAsset`.


## Spoken operator orders (version 8)

`Project.voice` is an optional ElevenLabs voice ID. Operator tracks use
`{ "type": "operator", "voice"?: "voice-id", "orders": [...] }`. Missing voices inherit:
track, then project, then the installation's voice. An installation's API key never travels in an archive.

Each order holds `id`, `at` (timeline-relative frames), `dialogue`, `alignment` (`start` or `end`), and
optional `generated`: `{ asset, dialogue, voice, model, samples }`. `samples` is the decoded duration at
`ENGINE_RATE`, not the MP3 source rate. The asset is bundled under `audio/` like imported audio.
A generation preserves its exact dialogue, voice ID and model (`eleven_multilingual_v2`).

The backend derives `ungenerated`, `synced` or `stale` by comparing the current dialogue and effective
voice with the saved generation. It never stores a status flag that can contradict that comparison.
Changing a voice or dialogue never initiates a billable request. Stale orders keep playing their saved audio.
Moving an order and changing its alignment do not make its speech stale.

The anchor remains at `at`. For start alignment the audio begins at the anchor; for end alignment it ends
at the anchor, subtracting the exact decoded sample count. An end-aligned order that would start before
the timeline origin is visibly invalid and silent; Flux neither truncates it nor moves its anchor.

Version 7 and earlier archives migrate to version 8 without voice overrides or operator tracks. New
operator fields are optional, and old Flux versions reject version 8 archives before parsing.

The operator interface and mono/stereo channels belong to `preferences.json`, alongside the installation's
voice ID. They are resolved into the inherited Operator destination (`OutputConfig.operator`). Format 10
adds per-track destination overrides as described below. An unassigned or missing route stays silent.
The operator bus follows FLUX's clock and uses saved audio offline. Generation can only start while stopped;
a response arriving after playback starts is retained without replacing the order's recording.

## Recording workflow (version 9)

Orders add optional `synthesisText`, with empty UI input represented as absence, and sample counts
`trimStart` / `trimEnd` (default zero). The audible sample count is `generated.samples - trimStart - trimEnd`;
trims must leave at least one sample. An order without a recording cannot have nonzero trims. The anchor
and alignment use the audible duration. The mixer offsets the source by `trimStart` without rewriting it.

Recordings add `origin` (`generated`, default for older files, or `imported`) and optional `synthesisText`.
Generated origin retains `eleven_multilingual_v2`; imported origin uses an empty model and its voice field
is not used for content matching. Imported audio preserves its source format in the audio asset path.
Generated matching compares the visible dialogue, pronunciation text and effective voice. Imported matching
compares the visible dialogue only. Replacing a recording resets order trims.

Version 8 and earlier migrate using serde defaults for these new fields. Saving writes the current version;
changing between `.flx` and legacy `.fxtc` does not change these rules.

## Audio routing (version 10)

`output.audio` is an optional array of portable `{ "id": "<uuid>", "name": "Monitors" }` destinations shared by all timelines. IDs are stable; names are nonempty, at most 64 characters, and unique without case distinctions. The built-in IDs `programme` and `operator` are permanent and are not stored in that array.

Audio and operator tracks may include `audioOutput`:

- `{ "type": "inherit" }` (also the omitted default): audio inherits Programme; operator inherits Operator.
- `{ "type": "silent" }`: no audible destination in either view.
- `{ "type": "output", "id": "<uuid or builtin id>" }`: one explicit destination.

Other track kinds must retain the inherited default. Removing a custom destination retargets its tracks across every timeline to Silent in one undoable edit. Undo restores both the destination ID and its assignments.

Hardware mappings belong to installation preferences: `audioRoutes` maps custom output IDs to an interface name hint and mono/stereo channels. Programme and Operator reuse their existing local preference fields. These mappings are never written into the project. Unmapped or missing outputs remain silent in Live; they never fall back to Programme. Edit with a preview route auditions every assigned track through that route, while Silent remains silent.

Version 9 and earlier open with inherited assignments and no custom outputs, including recovery records. Saving writes the current version; earlier builds reject it. Physical resolved `audioRoutes` and `trackRoutes` are runtime configuration, not fields of portable `ShowOutputs`.

### Version 11: voice order sources

Each operator order stores `mode`: `generated` or `manual`. It selects the authoring controls;
changing it preserves the current audio. Older imported orders migrate to `manual`; other orders default
to `generated`. The existing `generated` audio record additionally accepts `origin: "recorded"` for
microphone takes. Imported and recorded audio play offline without a provider voice or API key.
Accepted microphone takes are bundled WAV assets, using the existing trims and sample-accurate alignment.
Microphone driver, device and mono input channel are local preferences, never project routing.


### Version 12: setlists and timeline show controls

`setlists` is an optional array of `{id, name, entries}`. Each entry contains its own `id`,
`timelineId`, optional display `name`, operator `notes`, `omitted` flag and `transition`.
Transitions are `{type:"wait"}`, `{type:"continue"}` or
`{type:"after", milliseconds:<nonnegative integer>}`. Entries reference existing timelines;
several entries may reference the same timeline. Removing a referenced timeline is refused
until its entries are removed explicitly.

Each timeline may contain ordered `controls`: `{id, name, at, type:"wait"}` or
`{id, name, at, type:"loop", end, turns}`. Positions are relative frames and loops use
`[at,end)`. `turns:null` is indefinite; positive counts include the first turn. Controls must
not overlap, nest or share boundaries. MIDI and OSC events may contain `firstTurnOnly:true`;
absence/false repeats the event on every loop turn.

Older projects open with empty setlists/controls and per-event repetition enabled. Prepared
run state, loop turns, devamp requests, operational level overrides and OSC listener/subscriber
state are never archived. See [setlists.md](setlists.md) and [osc-api.md](osc-api.md).

### Version 13: video tracks

Adds `videos` (the bundled files under `video/`) and the `video` track kind. Nothing is reshaped: an older project
opens with no videos. Saving writes version 13, which builds supporting version 12 or earlier reject before parsing.

### Version 14: VJ grid

Adds `mode` and `vj`. Nothing is reshaped: an older project opens as a timeline project. Saving writes version
14, which builds supporting version 13 or earlier reject before parsing.

#### VJ grid (version 14)

A grid of clips launched by hand, like a media server's clip deck, in a project whose `mode` is `vj`. A project cannot
change its kind. Since version 15 a VJ show has a `project.json` of its own shape, described in
[Version 15](#version-15-a-vj-show-s-own-file): its grid fields are at the top level, without `vj.`. A version 14 VJ show
is read as below and saved in the new shape. A new VJ project starts with one deck of 8 columns and 3 layers. Which clip is playing is not stored: it belongs
to the running show.

| Field | Type | Notes |
| --- | --- | --- |
| `vj.decks[]` | `{ id, name?, columns }` | Sets of columns, 1 to 32, switched by hand while the layers stay. `name` absent is the deck's number. |
| `vj.decks[].columns[]` | `{ id, name? }` | Left first, 1 to 64 per deck. `name` absent is the column's number. |
| `vj.groups[]` | `{ id, name?, master?, opacity?, blend?, bypass?, solo?, collapsed?, effects? }` | Layer groups, bottom first, at most 16; absent when none. The layers of a group are consecutive in `vj.layers` and name it in their `group` field (`vj.layers[].group`, absent for none). A group's layers are mixed first and the result is blended like a layer: `master` and `opacity` are 0 to 1, absent is 1; `blend` as for a layer, absent is `alpha`; `bypass`, `solo` and `collapsed` absent are false; `effects` is the group's effect stack, the same entries as a layer's, applied to the mix of its layers. A group has at least one layer; names are 1 to 64 characters; ids are unique and share the id space of layers and decks. Added without a version bump: an older build reads the file without the groups, plays the layers flat and loses the groups when it saves. |
| `vj.deck` | string | The id of the deck shown. |
| `vj.clock` | `internal` \| `ltc` \| `mtc` | Where the show's timecode clock comes from; absent is `internal`. The show's tempo is the `tempo.bpm` of its one timeline. |
| `vj.link` | `true` | The show is in an Ableton Link session, sharing its tempo and beat phase; absent is off. Added without a version bump. |
| `vj.layers[]` | `{ id, name?, global?, master?, opacity?, volume?, blend?, transition?, transitionEffect?, autopilot?, crossfade?, bypass?, solo?, collapsed?, effects?, modulation?, dashboard?, clips? }` | Drawing order: the first is at the bottom, the last on top. 1 to 16, shared by every deck. A `global` layer keeps its clips when the deck changes and is not launched with a column. `master`, `opacity` (video) and `volume` (audio) are 0 to 1, absent is 1; the picture is `master` × `opacity`. `blend` is how the layer is drawn over the ones below, by its alpha and opacity: `alpha` (absent), `add`, `subtract`, `multiply`, `screen`, `lighten`, `darken`, `difference`, `exclusion`, `overlay`, `softLight`, `hardLight`, `colorDodge`, `colorBurn`, `linearLight`, `vividLight`, `pinLight`, `hue`, `saturation`, `color`, `luminosity`, `differenceInverted`, or the keys and masks `alphaCut`, `lumaKey`, `lumaKeyInverted`, `lumaIsAlpha`, `mask50`, `rgb` and `displace` (see [Video](video.md)); older files only have the first seven. `transition` is how long, 0 to 10 seconds, a launched clip takes to replace the one playing on the layer, absent is 0 (a cut); `transitionEffect` is any `blend` name: `alpha` (absent, the new clip fades in over the old) or a blend mode that mixes the two clips as the transition runs (`add` is one fades out while the other fades in; files written before the other modes existed only have `alpha` or `add`). `autopilot` is what the layer launches when the clip playing ends a pass: `off` (absent), `next`, `previous` or `random`; older files load as `off`. `crossfade` is `a` or `b`, the crossfader side, absent is neither. `bypass` hides the layer; with any `solo` layer only those show. `collapsed` folds the layer's row on the grid to a thin one; it plays as before. `effects` is the layer's effect stack (see below), absent when empty. `modulation` drives `master`, `opacity` or `volume` by itself (see [VJ modulation](#vj-modulation)), absent when empty. `dashboard` is the layer's own dashboard, the same entries as `vj.dashboard[]` below, whose links name numbers of the layer, its effects and its clips (`layerMaster`, `layerVolume`, `layerOpacity`, `layerTransition`, `clipSpeed`, `generatorNumber`, `effectParam`); absent when empty, added without a version bump. Ids of layers, decks and columns are unique within the grid; names are 1 to 64 characters. |
| `vj.layers[].clips[]` | `{ id, column?, slot?, asset?, generator?, name?, playback?, trigger?, speed?, duration?, direction?, tempo?, region?, cues?, fit?, scale?, position?, effects?, modulation?, dashboard? }` | At most one per cell. A clip of a deck layer has `column`, a column id of any deck; a clip of a global layer has `slot`, its position from the left, the same on every deck. `asset` is an entry of `videos`; a generator clip has no `asset` but a `generator`, `{ kind, ... }`, the picture Flux makes for it, which fills the composition: `solid` (`colour`), `strobe` (`colour`, `rate` flashes a second above 0 up to 30, `width` of each period lit above 0 up to 1; between flashes what is below shows), `gradient` (`from`, `to`, `angle` in degrees -360 to 360, 0 left to right and 90 top to bottom), `bars` (colour bars) or `noise` (`colour`, `rate` new snow a second above 0 up to 30) or `liveInput` (`device`, the name of a camera or capture card, absent or empty is none; the clip draws black while it is not connected; `size` `auto` (absent), `1280x720`, `1920x1080` or `3840x2160` and `rate` `auto` (absent), `25`, `30`, `50`, `59.94` or `60`, the format asked of the device) or `text`, a block of text over a clear background (`text` up to 4000 characters, lines split by `\n` and no other control characters, default `Text`; `font` family and `style` face by name, default `Inter` and `Bold`, falling back to the bundled Inter when the machine lacks them; `size` of the font as a fraction of the composition's height, 0.01 to 2, default 0.12; `colour`; `opacity` 0 to 1; `align` `left`, `center` (default) or `right` between the lines; `positionX` and `positionY`, the centre of the block as fractions of the composition, -1 to 2, default 0.5; `rotation` in degrees clockwise, -360 to 360; `tracking` and `leading`, extra space between glyphs and between lines in ems, -0.5 to 2, default 0; `lines` `all` (default) or `one`; `line`, the line shown with `one` (0-based, an empty one goes on to the next that has text); `advance`, 0 (by hand, default), 1, 2, 4, 8 or 16 beats of the grid's clock per step to the next line). Colours are `#rrggbb`; a parameter left out takes its default (white, 8 flashes of half the period, black to white at 0°, 30 a second). Added without a version bump: an older build does not read a `text` or `liveInput` generator. A clip with both or neither is rejected. `id` is unique among every clip of the project. `playback` is what the clip does at the end of its video: `loop` (absent), `once` or `pingPong`. `trigger` is `normal` (absent, plays until something else replaces it) or `piano` (plays while held). The clip's transport, all absent when it has its default (so the format version is unchanged: an older file has the defaults and an older build reads a newer file's other fields as it always did): `speed` is how fast it plays when its tempo is free, 0.05 to 8, absent is 1; `direction` is `forward` (absent), `reverse` or `pause`; `tempo` is `{ "kind": "free" }` (absent) or `{ "kind": "beats", "beats": n }` with `n` one of 1, 2, 4, 8, 16, 32, 64 or 128, where one pass through the region lasts exactly that many beats of the composition's tempo whatever the speed; `region` is `{ start, end }`, the part of the clip that plays and repeats as fractions 0 to 1 of its length with `end - start` at least 0.001, absent is all of it, and `playback` applies inside it; `cues` is 8 entries, each `null` or a fraction 0 to 1 of the clip's length. `duration` is how long a still image or a generator lasts, in seconds, 0.1 to 3600, absent is 10: the length its transport runs over (a video keeps its file's length and ignores it). A generator's transport is otherwise the same as a video's. See [Clip transport](video.md#clip-transport). `fit` as for video clips, absent is `fit`. `scale` draws the clip that many times its fitted size about its centre, 0 to 10, absent is 1 (100%). `position` is `[x, y]`, how far its centre moves right and down from where its fit puts it, as fractions of the composition's width and height, -2 to 2 each, absent is `[0, 0]`. `effects` is the clip's effect stack (see below), absent when empty. `modulation` drives `scale`, `positionX` or `positionY` by itself (see [VJ modulation](#vj-modulation)), absent when empty. `dashboard` is the clip's own dashboard (as a layer's), whose links name numbers of the clip and its effects (`clipSpeed`, `generatorNumber`, `effectParam`); absent when empty, added without a version bump, and it goes with the clip. |
| `vj.mappings[]` | `{ id, source, target, min, max, invert?, mode? }` | What MIDI controls and OSC addresses drive, at most 512; absent when empty. `source` is `{ "type": "midi", port?, channel, kind, number }` (`channel` 1 to 16, `kind` `cc`, `note` or `pitchBend`, `number` 0 to 127, 0 for a bend; `port` the input's name, absent is any) or `{ "type": "osc", address }` (starts with `/`, at most 256 characters), or `{ "type": "artnet", universe, channel, fine? }` (a DMX channel: `universe` the 15-bit Port-Address 0 to 32767 as on the wire, `channel` 1 to 512, `fine` another channel 1 to 512 holding the low byte of a 16-bit value, absent is 8 bits), or `{ "type": "key", code, ctrl?, alt?, shift?, meta? }` (a key of the keyboard by its place, `KeyboardEvent.code` such as `KeyQ` or `Digit5`, letters and digits only, with the modifiers held, each absent when false; the combinations the app uses itself are refused when learned, see [Remote control](remote-control.md#keyboard)). `target` is tagged by `kind`: an action (`clipLaunch` `clip`, `columnLaunch` `column`, `layerStop`, `layerAutopilot` `layer`, `deckSelect` `deck`, `deckNext`, `deckPrev`, `stopAll`, `bpmTap`, `bpmResync`, `bpmPlus`, `bpmMinus`, `bpmDouble`, `bpmHalf`, `bpmNudgeUp`, `bpmNudgeDown`), a switch (`layerBypass`, `layerSolo` `layer`, `effectBypass` `effect`, `bpmPause`) or a level (`layerMaster`, `layerVolume`, `layerOpacity`, `layerTransition` `layer`, `crossfader`, `bpm`, `clipSpeed` `clip`, `effectParam` `effect` `param`, `generatorNumber` `clip` `name`, `dashboard` `knob` 0 to 7, `layerDashboard` `layer` `knob`, `clipDashboard` `clip` `knob`: a knob of a layer's or a clip's dashboard). `min` and `max` are where a level goes with the source at 0 and at 1, in the target's own units and within its range (min may be above max); `invert` turns the source round; `mode` is `trigger` (absent), `toggle` (switches only) or `hold`. `follow` is `id` (absent: the mapping keeps hold of that clip, layer or effect wherever it moves) or `position` (a clip, layer, column, deck or effect target that follows its slot): then `place` holds the slot as a target whose ids are places (see the `.flxmap` format below) and `target` is whatever fills the slot now, rewritten after every edit; `parked` (absent is false) marks a position mapping whose slot is empty, which drives nothing and is not checked against the grid until something fills the slot. Ids are unique, and every target must exist: a file that maps something that is not there is refused, and an edit that removes a target removes its mappings. At most one mapping per target and per source. Added without a version bump: an older build ignores the field and loses the mappings when it saves, and one that does not know the `artnet` or `key` sources refuses a project that holds one. |
| `vj.dashboard[]` | `{ value, name?, links? }` | The eight knobs of the composition dashboard, in order, at most 8; absent is every knob at 0 with no links. `value` is 0 to 1, `name` 1 to 64 characters, `links[]` (at most 16) is `{ target, min, max }`, a level target as above (not a `dashboard` one) with the numbers the knob gives it at 0 and at 1. The links are applied when a knob turns; the stored values of the targets are the ones the knob last set. Added without a version bump. |
| `compositionEffects[]` | `{ id, bypass?, modulation?, effect, ... }` | The top-level `compositionEffects` of a VJ file: the effects of the composition, applied to the final mix of the layers before the output mapping, at most 16, the first applied first. Same entries as `effects[]`, plus the kinds that only exist here (`feedback`, `trails`, `bloom`). Absent when empty. `compositionBypass` (true skips the whole stack, absent is false) sits beside it. Both are optional additions, so the format version is unchanged (an older build reads the file without them and draws no composition effects; one that does not know a kind refuses the project). Effect ids are unique across them and the clips' and layers'. |
| `effects[]` | `{ id, bypass?, modulation?, effect, ... }` | On a clip or a layer, at most 16 each, the first applied first. A clip's stack applies to its picture first and then its layer's, so a layer's effects reach both clips of a transition. `id` is unique among every effect of the project and seeds the effect's randomness; `bypass` skips the effect (absent is false). `effect` names the kind and the rest are its settings, each brought into range by the ops and checked on load (a NaN, a value out of range, a choice that is not one of its options or a bad colour is refused). The kinds, their categories and settings are listed under [VJ effects](#vj-effects). New kinds are new values of `effect`: the field was already optional, so the format version is unchanged (a build that does not know a kind refuses the project, as with any unknown value). `modulation` drives numeric settings by themselves (see [VJ modulation](#vj-modulation)), absent when empty. |

#### VJ effects

The kinds are described once, in `flux_project::vj_effects::catalog()`, which holds each one's category and every
setting's range, step, unit and default; the ops take their defaults and limits from it and `vj_effect_catalog` hands the
same table to the interface, which draws its lists and controls from it. Categories: `colour`, `distort`, `blur`,
`glitch`, `stylise`, `time` and `feedback`. Colour work (levels, posterize, threshold, solarize, noise) happens on a gamma 2.2
scale of the light so that its steps look even; everything else works on linear light.

Effects marked **paced** move with time and carry `sync` (`free` or `beats`), `rate` (cycles a second, 0 to 30, used when
free) and `beats` (the length of one cycle in beats of the show's tempo: 1/4, 1/2, 1, 2, 4, 8 or 16, used when synced).
What a cycle is depends on the effect and is given below; at `rate` 0 a free effect stands still.

| Category | `effect` | Settings (range, default) |
| --- | --- | --- |
| colour | `brightness` | `amount` -1 to 1 (0) |
| colour | `contrast` | `amount` 0 to 4 (1) |
| colour | `saturation` | `amount` 0 to 4 (1) |
| colour | `hue` | `degrees` -180 to 180 (0) |
| colour | `tint` | `colour` (`#ff8800`), `amount` 0 to 1 (0.5) |
| colour | `invert` | `amount` 0 to 1 (1) |
| colour | `levels` | `black` 0 to 1 (0), `white` 0 to 1 (1), `gamma` 0.1 to 4 (1) |
| colour | `colorize` | `dark` (`#1a0b3d`), `light` (`#ffb347`), `amount` 0 to 1 (1): a gradient map of the picture's grey |
| colour | `solarize` | `threshold` 0 to 1 (0.5) |
| distort | `mirror` | `axis` `horizontal`, `vertical` or `both` |
| distort | `zoom` | `amount` 0.25 to 8 (1); outside the picture is transparent |
| distort | `shift` | `x`, `y` -1 to 1 (0), wrapping around |
| distort | `rotate` | paced; `angle` -180 to 180 (0), `spin` -4 to 4 turns a cycle (0); the corners it leaves are transparent |
| distort | `kaleidoscope` | paced; `segments` 2 to 32 (6), `rotation` -180 to 180 (0), `spin` -4 to 4 turns a cycle (0.25), `x`, `y` centre 0 to 1 (0.5) |
| distort | `polar` | `amount` 0 to 1 (1): rectangle to polar |
| distort | `wave` | paced; `amplitude` 0 to 0.25 (0.03), `frequency` 0 to 32 waves across (4), `direction` `horizontal`, `vertical` or `both`; a wave travels one period a cycle |
| distort | `twirl` | `angle` -1440 to 1440 degrees (180), `radius` 0 to 2 picture heights (0.5), `x`, `y` centre |
| distort | `bulge` | `amount` -1 to 1 (0.5; negative pinches), `radius` 0.05 to 2 (0.6), `x`, `y` centre |
| distort | `tile` | `columns`, `rows` 1 to 16 (2), `mirror` (true: alternate tiles are turned over) |
| distort | `pixelate` | `size` 0 to 0.25 of the height (0.02); 0 is none |
| distort | `displace` | paced; `amplitude` 0 to 0.25 (0.03), `scale` 0.5 to 32 blobs across (6); the noise drifts one unit a cycle |
| blur | `blur` | `radius` 0 to 1 of the largest blur (0.1), 5% of the picture's short side |
| blur | `radialBlur` | `amount` 0 to 1 (0.3), `x`, `y` centre |
| blur | `zoomBlur` | `amount` 0 to 1 (0.3), `x`, `y` centre |
| glitch | `shiftGlitch` | paced; `amount` 0 to 1 of the width (0.15), `block` 0.005 to 0.5 band height (0.06), `probability` 0 to 1 (0.35); the bands are drawn again every cycle |
| glitch | `twitch` | paced; `amount` 0 to 1 (0.5), `probability` 0 to 1 (0.5); in a cycle that is picked the picture jumps by up to a quarter of its size, scales between 0.5 and 1.5 and changes light, held until the next cycle |
| glitch | `rgbSplit` | paced; `amount` 0 to 1 (0.3; 1 pulls 5% of the height), `angle` -180 to 180 (0), `animate` (turns the angle once a cycle) |
| stylise | `posterize` | `levels` 2 to 32 (4) |
| stylise | `threshold` | `level` 0 to 1 (0.5), `softness` 0 to 1 (0.05), `dark` (`#000000`), `light` (`#ffffff`) |
| stylise | `edgeDetect` | `strength` 0 to 8 (1), `mode` `edges` or `glow` (added to the picture) |
| stylise | `vignette` | `amount` 0 to 1 (0.6), `softness` 0 to 1 (0.5), `roundness` 0 (the frame) to 1 (an ellipse) (0.5) |
| stylise | `noise` | `amount` 0 to 1 (0.25), `size` 0 to 1 (0.1), `animated` (new grain 24 times a second) |
| stylise | `scanlines` | paced; `count` 2 to 540 (120), `intensity` 0 to 1 (0.5); the lines scroll one line a cycle |
| time | `strobe` | paced; `colour` (`#ffffff`), `duty` 0.02 to 1 of each cycle (0.25), `mode` `flash` or `blackout` |
| time | `slide` | paced; `x`, `y` -4 to 4 pictures a cycle (0.25, 0), `wrap` (true goes round; false lets the picture leave and come back) |
| time | `roll` | paced; `jitter` 0 to 1 (0.2); the picture rolls up one picture a cycle, like a TV out of vertical hold |
| feedback | `feedback` | **composition only**; `amount` 0 to 1 (0.6), `zoom` 0.5 to 2 (1.02), `rotate` -180 to 180 degrees a second (0), `x`, `y` -1 to 1 of the frame a second (0), `hue` -180 to 180 degrees a second (0), `blend` `mix`, `add` or `lighten` |
| feedback | `trails` | **composition only**; `decay` 0 to 1 (0.9), `mode` `blend`, `lighten` or `add` |
| feedback | `bloom` | **composition only**; `threshold` 0 to 1 (0.6), `intensity` 0 to 4 (1), `radius` 0 to 1 (0.5) |

The three **composition only** kinds (the catalogue marks them `compositionOnly`) need the frames before them or the
pixels around them, so a clip's or a layer's stack refuses them (`addEffect` and the file's validation). `amount` and
`decay` are the share of the old frame kept every 60th of a second, and `zoom` is how many times larger it is drawn in
that time; movements and hue are per second. All of them are scaled by the seconds since the last frame, so the look does
not change with the frame rate. See [Composition effects](video.md#composition-effects).

Randomness is never drawn from a generator: it is a hash of the effect's id, the step of time (the whole cycle) and the
pixel or the row, so the same frame is the same wherever it is drawn and a stop and a launch repeat it.

#### VJ modulation

Any numeric setting that can be modulated is driven by an LFO or by the sound, between a minimum and a maximum, as in
Resolume. A modulation is stored with what it moves, in a `modulation` object keyed by the setting; the field is absent
when empty on effects, layers and clips, so it is an optional addition and the format version is unchanged (a project
written before modulation reads as one with none). The setting itself keeps the value the user set: modulation is
evaluated while the grid plays and never written back.

| Where | Keys | Range of `min` and `max` |
| --- | --- | --- |
| `effects[].modulation` | the key of any numeric setting of the effect's kind in the catalogue, except `rate` and `beats` (what an LFO is itself paced by) | the setting's own range |
| `vj.layers[].modulation` | `master`, `opacity`, `volume` | 0 to 1 |
| `vj.layers[].clips[].modulation` | `scale`, `positionX`, `positionY` | 0 to 10; -2 to 2 |
| `vj.layers[].clips[].modulation` of a generator clip | `generator.<name>` for a number its generator has: `size`, `opacity`, `positionX`, `positionY`, `rotation`, `tracking`, `leading` (text), `rate`, `width` (strobe), `angle` (gradient), `rate` (noise) | the generator's own limits above |

```json
{ "source": { "kind": "lfo", "shape": "sine", "pace": { "sync": "beats", "rate": 0.5, "beats": 1 }, "phase": 0 }, "min": -180, "max": 180 }
{ "source": { "kind": "audio", "band": "low", "gain": 1, "attack": 0.02, "release": 0.25 }, "min": 0, "max": 1 }
```

`min` and `max` are in the setting's own units and `min` may be above `max`, which inverts the movement. The value is
`min + (max - min) * s` with `s` from 0 to 1. An `lfo` has a `shape` (`sine`, `triangle`, `sawUp`, `sawDown`, `square`,
`random`, `smoothRandom`), a `pace` as an effect's (`sync` `free` with `rate` in Hz, 0 to 30, or `beats` with `beats` one
of 1/4, 1/2, 1, 2, 4, 8 or 16) and a `phase`, 0 to 1. An `audio` source follows a `band` (`low`, `mid`, `high` or
`level`) times a `gain` (0 to 8), cut at 1 and smoothed by an `attack` and a `release` in seconds (0 to 5 each). Values
out of range are brought into it by the op and refused by the load check; a NaN is refused. The evaluation, its timebase
and the audio analysis are described in [VJ modulation](video.md#modulation).

Commands: `vj_edit` (`project`, `op`) edits the grid of a VJ project; `op` is tagged by `op`: `addLayer` (`global`),
`removeLayer`, `moveLayer`, `renameLayer`, `layerMaster`, `layerOpacity`, `layerVolume`, `layerTransition`,
`layerBlend`, `layerTransitionEffect`, `layerCrossfade`, `layerBypass`, `layerSolo`, `layerCollapse`, `setBpm`, `nudgeBpm`, `scaleBpm`, `tapBpm`, `setClock`, `addDeck`, `removeDeck`,
`moveDeck`, `renameDeck`, `selectDeck`, `addColumn` (to the deck shown), `removeColumn`, `moveColumn`, `renameColumn`,
`placeVideo` (a video of the project into a cell), `placeGenerator` (`generator`, `layer`, `column`: a new generator clip into a cell), `moveClip` (swaps with the clip already there), `removeClip`,
`renameClip`, `clipPlayback`, `clipSpeed` (`value`, brought into 0.05 to 8), `clipDuration` (`value`, brought into 0.1 to 3600 seconds; only a still image or a generator uses it), `clipDirection` (`direction`), `clipTempo` (`tempo`; the beats go to the nearest allowed length), `clipRegion` (`start`, `end`), `clipCue` (`index` 0 to 7, `position` or `null` to clear it), `clipTrigger`, `clipFit`, `clipScale` (`value`), `clipPosition` (`x`, `y`), `addEffect` (`target`: `{ clip }`, `{ layer }` or `"composition"`, `kind`: a kind's name, which takes the settings it starts with; at the end of the stack; the composition-only kinds are refused on a clip or a layer), `compositionBypass` (`bypass`: skips the composition's whole stack without touching the effects), `removeEffect` (`effect`), `moveEffect` (`effect`, `index`: its new place in its stack), `effectParams` (`effect`, `kind`: the effect's kind with its new settings, which must be the kind it already is), `effectBypass` (`effect`, `bypass`), `duplicateEffect` (`effect`: a copy with a new id and the same settings right after it, refused when the stack is full), `resetEffect` (`effect`: its kind's starting settings, and no modulation), `setModulation` (`target`: `{ effect, param }`, which also finds an effect of the composition, `{ layer, field }` or `{ clip, field }`; `modulation`: a modulation, or `null` to take it away; brought into the range of the setting and refused for a setting that cannot be modulated; `vj_preview` takes it too, so a minimum, a maximum or a gain can be dragged live; `vj_preview` also takes `clipSpeed` and `clipRegion`, so a speed or a region handle can be dragged live) and `clipGenerator` (`generator`, the new parameters of a generator clip; a video clip refuses it), which clamp to their limits, and `textLine` (`clip`, `step`: moves a text block's `line` by that many lines, skipping the empty ones and wrapping round; refused for any clip that is not a text block). The mapping ops are `setMappingFollow` (`mapping`, `follow`), `setMappingFine` (`mapping`, `fine`: `null` for 8 bits; DMX mappings only), `addMapping` (`source`, `target`: the latest source wins, an existing mapping of the target keeps its options, and a mapping of the same source is replaced), `removeMapping` (`mapping`), `clearMapping` (`target`), `setMapping` (`mapping`, `min`, `max`, `invert`, `mode`), `dashboardValue` (`knob`, `value`: turns the knob and moves its links; `vj_preview` takes it, so a knob can be dragged live), `dashboardLink` (`knob`, `target`, `min?`, `max?`: a bound left out is the target's own), `dashboardUnlink` (`knob`, `target`) and `renameKnob` (`knob`, `name`); these four take an optional `scope`, `{ "kind": "layer", "layer" }` or `{ "kind": "clip", "clip" }`, for the dashboard of a layer or a clip (absent is the composition's), which only links numbers of its owner. `setModulation` also takes `{ knob }`, `{ layer, knob }` and `{ clip, knob }` for a knob of the composition, a layer or a clip. The last layer, deck and column of a deck cannot be removed.
`project_import_vj_video` (`project`, `path`, `layer`, `column?`) adds a video file from disk into a cell, or into the
layer's first empty cell of the deck shown when `column` is absent. Both answer
with `ProjectState`, and a refusal leaves the document as it was.

### Version 15: a VJ show's own file

A VJ show and a cue show are two apps in one, and their `project.json` files have nothing in common beyond the
`formatVersion`, `mode`, `name`, `meta` and `autosave` every show has. A cue show's file is unchanged. A VJ show's
(`mode: "vj"`) holds:

| Field | Type | Notes |
| --- | --- | --- |
| `videos` | as above | The videos and images the grid plays, linked by path (`link`) or, in an older show, bundled under `video/`. |
| `tempo` | `{ bpm, beatsPerBar, click }` | The show's tempo. |
| `timecode` | `{ frameRate, start }` | The timecode of the show's internal clock. |
| `screens`, `composition` | as `output.screens` / `output.composition` | Where the picture goes (Advanced Output). |
| `layers`, `decks`, `deck`, `clock` | as `vj.*` below | The grid. |

It has no `timelines`, `tracks`, `assets`, `setlists`, `voice` or LTC/MTC/OSC/audio/metronome outputs, and the app
refuses a VJ show that would hold any. In memory the app still keeps the clock's tempo and timecode as one empty
timeline (id `clock`), which the file does not store. A version 14 VJ show (with `timelines` and `vj`) still opens:
whatever cue content an older build left in it (such as a video imported onto its timeline, which the clock then
played) is dropped, and saving writes the current version. Builds supporting version 14 or earlier reject version 15.

## Mappings file (`.flxmap`)

The mappings and dashboard links of a VJ show as a file of their own, to carry them to another show (Mappings tab or
Composition > View > Export / Import mappings). It is JSON, and it is portable because it names targets by **place**
instead of id:

```json
{ "format": "flux-vj-mappings", "version": 1,
  "mappings": [ { "source": { "type": "midi", "channel": 1, "kind": "cc", "number": 7 },
                  "target": { "kind": "layerOpacity", "layer": "1" },
                  "ids": { "kind": "layerOpacity", "layer": "a-uuid" },
                  "follow": "position", "min": 0, "max": 1, "invert": false, "mode": "trigger" } ],
  "dashboard": [ { "name": "Fade", "links": [ { "target": { "kind": "layerMaster", "layer": "2" }, "min": 0, "max": 1 } ] } ] }
```

| Id of | Place (0-based) |
| --- | --- |
| layer, deck | its place from the bottom or the left: `"2"` |
| column | `"deck.column"`: `"0.3"` |
| clip | `"layer/deck.column"`, or `"layer/sN"` for slot N of a global layer: `"2/0.3"`, `"1/s5"` |
| effect | `"owner:slot"` with owner `c` (composition), `lN` (layer N) or `kN/...` (a clip, as above): `"l1:0"` |

Everything else (the source, the range, `invert`, `mode`, global targets, an effect setting's name, a knob's number) is
stored as it is. `ids` holds the same target by id. `version` is 1; a file with a higher one, or another `format`, is
refused. **Import** resolves each mapping's `ids` first when the show has them (reading a file back into its own show) and
its place otherwise; what does not resolve, or does not fit (a slot that is not there, an effect setting the effect in
that slot does not have), is skipped and counted. Commands: `vj_mappings_export(project, path)`,
`vj_mappings_import_preview(project, path)` (what would be taken and skipped, and the conflicts: an incoming mapping with
the same source or the same target as one the show has) and `vj_mappings_import(project, path, choice)` where `choice` is
`{ wipe, keep }` (`wipe` drops the show's mappings and links first; `keep` lists conflicts, by their index in the file,
to leave as they are). It answers with the project (one undo step) and `imported`, `skipped` and `kept`.


## VJ presets and templates

Effect presets and show templates are the user's, not a show's, so they are not in a `.flx`: they live in the app data
folder, as versioned JSON (code: `flux-project/src/vj_presets.rs`, commands in `src-tauri/src/vj_presets_commands.rs`).
A show copies a preset or a template when it uses one and never refers back to it.

| File | Holds |
| --- | --- |
| `vj-presets.json` | `{ version, presets: [{ id, name, effects: [{ settings, modulation?, bypass? }] }] }`. `settings` is the effect as the project stores it (`effect` names the kind), so a preset made before a kind gained a setting still applies: the settings it lacks keep their value, one it does not know is ignored. One effect is a preset of that effect; several are a stack. At most 500 presets, names 1 to 64 characters. |
| `vj-templates/<id>.json` | `{ version, name, ... }`: the VJ show file (the same fields as a VJ `.flx`'s `project.json`) with every layer's `clips` and the `videos` emptied, and the mappings and dashboard links that targeted a clip taken out. At most 100 templates. |

A file with a `version` above the one this build writes (`1`) is refused with an error and left alone, never read half or
overwritten; an unreadable `vj-presets.json` is an error, not an empty library. A show made from a template is new and
unsaved; it keeps the template's ids, which are unique within the show as they were.
