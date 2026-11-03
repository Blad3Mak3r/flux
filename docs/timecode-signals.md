# Timecode signals

Pure encoders in `src-tauri/src/engine/`. They only build bits, bytes and samples; the transport decides when to
send them. Frame math (`Timecode` <-> frame count, 29.97 drop-frame) is `timecode.rs`.

## LTC (`ltc.rs`)

`frame_bits` builds the 80-bit SMPTE 12M word, `bits[0]` is sent first:

| Bits | Content |
| --- | --- |
| 0-3, 8-9 | Frame units, tens (BCD, least significant bit first) |
| 10 | Drop-frame flag (29.97 only) |
| 11 | Colour frame flag (always 0) |
| 16-19, 24-26 | Seconds units, tens |
| 32-35, 40-42 | Minutes units, tens |
| 48-51, 56-57 | Hours units, tens |
| 4, 12, ... 60 | The eight user-bit nibbles |
| 27 (59 at 25 fps) | Polarity correction: makes the number of ones even, so every frame starts with the same polarity |
| 64-79 | Sync word `0x3FFD` |

`LtcEncoder::encode_frame` appends the biphase-mark samples of one word: a transition at the start of every cell
and another in the middle of a 1. A frame lasts `sample_rate / fps` samples, rarely a whole number, so cell
boundaries are computed from a running half-cell count with exact integer arithmetic and the remainder carries into
the next frame (1000 frames of 29.97 fps at 48 kHz are exactly 1 601 600 samples).

`LtcConfig`: `sample_rate`, `level` (linear peak amplitude) and `edge_samples` (slew limit: samples an edge takes
to swing between polarities, 0 is a hard square wave). `max_frame_samples()` gives the buffer size to reserve so
the audio callback never allocates.

## MTC (`mtc.rs`)

- `quarter_frames` returns the eight `F1 nn` messages; the high nibble of `nn` is the piece number, the low nibble
  is a nibble of the binary frames, seconds, minutes and hours. Piece 7 also carries the rate code (0 = 24,
  1 = 25, 2 = 29.97 drop, 3 = 30).
- A group describes the frame at which **piece 0 is sent**; pieces follow every quarter frame, so it takes two
  frames and receivers add two frames once they have all eight.
- `full_frame` returns `F0 7F 7F 01 01 hh mm ss ff F7` with the rate code in bits 5-6 of `hh`. It is meant for
  start and seek.

- `MtcScheduler` decides when each quarter frame goes out. `resync(position)` returns the full-frame message for a
  start or seek; quarter frames resume at the next group of eight (an even frame), because a receiver needs a
  group to begin at piece 0. `poll(position, send)` calls `send` for every quarter frame whose time has come, and
  skips ahead to the next group if the caller stalls instead of flooding the port with stale pieces.

## Transport (`transport.rs`)

The audio device's sample clock is the master. `Transport` holds the position as a **sample count** since the
timeline origin; the output callback calls `advance(samples)` with what it produced, so playback, LTC and MTC share
one clock and cannot drift apart. Nothing in it allocates or locks.

- `Clock` converts between samples and frames, MTC quarter frames (4 per frame) and LTC half cells (160 per frame).
  Every boundary is the exact fraction rounded to the nearest sample, so 1000 frames of 29.97 fps at 48 kHz are
  exactly 1 601 600 samples and errors never accumulate.
- `Command`: `Play`, `Pause`, `Stop` (back to the start), `Seek(frame)`. A seek takes a frame count, like every
  position in the project; positions before `project.timecode.start` go to the start.
- `epoch()` changes whenever playback starts or the position jumps (start, stop, seek) but not on pause. Generators
  compare it with the last epoch they saw and call `resync`.
- `LtcStream` plays LTC in step with the transport: silence until the next frame boundary, then one frame after
  another from a preallocated buffer, so `fill` is safe in the audio callback.

## Defaults and open decisions

`LtcConfig::default()` is 48 kHz, level 0.5 (-6 dBFS) and a one-sample edge. These can become project settings later.
Whether the LTC word should lead the video frame by some offset is not decided: today frame N starts exactly where
the transport reaches frame N.

## Output and transport commands (`output.rs`, `transport_commands.rs`)

`output_start(project)` opens the devices `project.output` names and starts a stopped transport at
`project.timecode.start`; it replaces a running output. TS wrappers are in `src/transport/api.ts`.

| Command | Arguments | Result |
| --- | --- | --- |
| `output_start` | `project` | `OutputInfo`: `device`, `sampleRate`, `channels`, `ltcChannel`, `mtcPort`, `warnings`. |
| `output_stop` | | Closes the devices. |
| `transport_play` / `transport_pause` / `transport_stop` | | Stop goes back to the timeline start. Error if no output runs. |
| `transport_seek` | `frame` | Moves to a timecode given as a frame count. |
| `transport_position` | | `Position` (`state`, `frame`, `positionSamples`, `deviceLost`) or `null` when no output runs. |

While an output runs, the event `transport-position` carries a `Position` about 30 times a second whenever it
changes.

- One audio device carries everything: the LTC device, or the `output.audio` device when there is no LTC. If both
  are set they must be the same device. The stream opens at 48 kHz with the fewest channels that cover the highest
  channel used, in 32-bit float, else 32 or 16-bit integer. Clip playback (A4) will use the same stream.
- The audio device's sample clock is the master. The callback owns the `Transport`, drains commands with
  `try_recv`, renders LTC into its channel, advances the position and publishes it with `try_lock`: it never
  allocates or waits. A reader holding the lock only makes it skip one update.
- MTC runs on a thread that polls every millisecond. It reads the published position and extrapolates with the wall
  clock (at most 100 ms), so quarter frames are not quantised to the audio buffer. It sends a full frame whenever the
  transport epoch changes. A missing MIDI port becomes a warning in `OutputInfo`; the audio side still starts.
- Only the audio device is required: an MTC-only project cannot start yet, because there is no software clock.
- On Windows `thread::park_timeout(1 ms)` may sleep for the system timer resolution (about 15 ms), which jitters MTC
  quarter frames; raising the timer resolution is a follow-up.
