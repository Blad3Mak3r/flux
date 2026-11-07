# `.flx` / `.fxtc` project format (v7)

A `.flx` file is a zip archive. Legacy `.fxtc` files use exactly the same format. Implementation: `src-tauri/crates/flux-project/src/`.

New projects and Save as use `.flx` by default. Opening accepts either extension, case-insensitively. Saving an existing project preserves its path and extension, so legacy projects stay `.fxtc` unless saved as a new `.flx` file. Changing the extension requires no archive migration or format-version bump.

```
project.json          deflated
audio/<file name>     stored (audio is large and usually compressed already)
peaks/<file name>.bin stored, optional: the waveform preview of that audio (see below)
```

- The `audio/` folder is flat: an asset file is `audio/<name>` with no further separators, `.` or `..`.
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
| `formatVersion` | number | `8`. A file with a newer version is rejected before it is parsed further; an older one is migrated on opening (see Versioning). |
| `name` | string | |
| `meta` | object | Identity and history of the project, see below. |
| `autosave` | `{ enabled, intervalSeconds }` | Project-owned recovery preferences. Defaults to enabled, every 30 seconds; supported intervals are 15, 30 and 60 seconds. Also saves after 2 seconds without edits. Older projects receive these defaults on migration. |
| `assets[]` | `{ id, name, file, bpm? }` | `file` is the archive path under `audio/`. Shared by all timelines; an asset no clip plays is dropped when the last clip using it goes. `bpm` is the tempo found in the audio (or typed by the user), 20 to 400; absent when never detected. Added without a version bump. |
| `timelines[]` | `{ id, name, timecode, tempo, tracks }` | Never empty. `id` is unique among the timelines; `name` is free text. |
| `timelines[].tempo` | `{ bpm, beatsPerBar, click }` | The beat grid of the timeline, counted from its origin: `bpm` 20 to 400, `beatsPerBar` 1 to 16 (the beat is a quarter note), `click` whether the metronome plays while this timeline runs. Absent in files written before it existed, read as 120 BPM, 4 beats, click off, without a version bump. Which bars the ruler shows is a view setting of the window and is not stored. |
| `timelines[].timecode.frameRate` | `fps24` \| `fps25` \| `fps2997Drop` \| `fps30` | The four rates MTC can express; LTC supports them too. |
| `timelines[].timecode.start` | number | Frame count of the timeline origin (e.g. 01:00:00:00). |
| `timelines[].tracks[]` | `{ id, name, muted, gainDb?, color?, kind }` | Track, clip and note ids are unique in the **whole project**, not per timeline. `kind` is tagged by `type`, see below. `gainDb` is the track's fader in dB, audio tracks only; absent is 0 dB, and `-60` or less is silence (the fader's "-inf" end). It multiplies each clip's own `gainDb`. Absent in files written before it existed, without a version bump. `color` is the colour the track is drawn in, `#rrggbb` (six hex digits; anything else is rejected on open and save); absent means the default for its kind (phosphor for audio, amber for notes), also without a version bump. |
| `output.metronome` | `{ gainDb }` | Optional logical click output. The project owns its level; each timeline owns tempo and click activation. The computer assigns its interface and channels. |
| `output.ltc` | `[{ slot, level? }]` | Logical LTC outputs. `slot` is a unique positive integer within LTC, preserved when other outputs are removed. Level is linear 0–1; absent is 0.5. Device, channel and latency are local preferences. |
| `output.mtc` | `[{ slot }]` | Logical MTC outputs with unique positive slots, independent of LTC slots. The computer assigns the MIDI port, same-name port index and latency. |
| `output.osc` | `[{ id, name, host, port }]` | The OSC receivers (UDP) the OSC tracks send to, defined once for the project. `id` is unique among them; `host` is a name or an address without spaces, `port` 1 to 65535. A host that does not resolve is reported when the output opens and never blocks the project. Absent is an empty list. Added in version 5. |

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

Other kinds of track (video, Art-Net) will be added as new kinds. Events are kept in order of `at`; event ids share the
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

## Saving

`save` writes the archive to `<target>.tmp` and renames it over the target, so a failed save leaves the existing file
as it was. Each asset is taken from either a file on disk (newly imported) or an existing `.fxtc`, from which the entry
(and its preview) is copied without recompressing.

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

Restoration keeps the unsaved baseline and reads bundled assets from their original archives, newly imported assets
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
| `project_new` | `name`, `frameRate` | A new `Project`; forgets the previous project's assets. |
| `project_open` | `path` | The `Project` read from that `.fxtc`. |
| `project_import_audio` | `path` | An `AudioAsset` (generated id, file `audio/<id>_<name>`) to add to the project. |
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
voice ID. They are resolved into `OutputConfig.operator`. An unassigned or missing route stays silent.
The operator bus follows Flux's clock; no speech generation or network activity runs during playback.
