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

`output_start(project, onPosition, onLevels)` opens the devices `project.output` names and starts a stopped transport at
`project.timecode.start`; it replaces a running output. TS wrappers are in `src/transport/api.ts`.

| Command | Arguments | Result |
| --- | --- | --- |
| `output_start` | `project`, `onPosition`, `onLevels` (channels, below) | `OutputInfo`: `device`, `sampleRate`, `channels`, `ltcChannel`, `mtcPort`, `warnings`. |
| `output_stop` | | Closes the devices. |
| `transport_play` / `transport_pause` / `transport_stop` | | Stop goes back to the timeline start. Error if no output runs. |
| `transport_seek` | `frame` | Moves to a timecode given as a frame count. |
| `transport_position` | | `Position` (`state`, `frame`, `positionSamples`, `deviceLost`) or `null` when no output runs. |

While an output runs it reports over two lanes, each with a thread of its own and the `Channel` that
`output_start` was given for it, about 30 times a second each:

| Lane | Thread | Channel (`output_start` argument) | Sends |
| --- | --- | --- | --- |
| Position | `flux-position` | `onPosition`: `Channel<Position>` | The `Position`, only when it moved. |
| Levels | `flux-levels` | `onLevels`: `Channel<[{ id, peak }]>` | The peak of each track since the previous message; silence once after the sound stops, then nothing. |

The lanes are independent, so a slow consumer of the meters never delays the timecode. A channel is ordered,
goes to the window that opened the output and ends with it, which a broadcast event does not; the UI also drops a
message that belongs to an output it has closed since.

### Threads

No command runs on the main thread, which belongs to the window and its event loop. Tauri runs a command without
`async` inline on that thread, so the small ones (`transport_*`, `output_set_track_gain`) are
`#[tauri::command(async)]` and use the command thread pool; the heavy ones (`output_start`, `output_stop`,
`output_set_clips`, `project_*`, `devices_*`, `audio_analyze`) use the blocking pool. The rest:

| Thread | Work |
| --- | --- |
| `flux-audio` | The audio callback: programme, LTC, position. Never allocates or waits. |
| `flux-mtc` | MIDI Timecode quarter frames. |
| `flux-position`, `flux-levels` | The two lanes above. |

Commands that set the transport only enqueue a message for the audio callback, so two of them issued back to back
from the UI are not guaranteed to run in order; the UI awaits the ones that depend on each other (Play waits for the
output to open) and sends dragged seeks and faders one at a time, newest value last.

- One audio device carries everything: the LTC device, or the `output.audio` device when there is no LTC, or the
  system's default output when neither names one (an empty `deviceName`). If both are set they must be the same
  device. Neither LTC nor MTC is needed to play the music. The stream opens at 48 kHz with the fewest channels that cover the highest
  channel used, in 32-bit float, else 32 or 16-bit integer. The programme (below) uses the same stream.
- The audio device's sample clock is the master. The callback owns the `Transport`, drains commands with
  `try_recv`, renders LTC into its channel, advances the position and publishes it with `try_lock`: it never
  allocates or waits. A reader holding the lock only makes it skip one update.
- MTC runs on a thread that polls every millisecond. It reads the published position and extrapolates with the wall
  clock (at most 100 ms), so quarter frames are not quantised to the audio buffer. It sends a full frame whenever the
  transport epoch changes. A missing MIDI port becomes a warning in `OutputInfo`; the audio side still starts.
- The audio device always provides the clock, so an MTC-only project still opens one (the default output).
- On Windows `thread::park_timeout(1 ms)` may sleep for the system timer resolution (about 15 ms), which jitters MTC
  quarter frames; raising the timer resolution is a follow-up.

## Programme audio (`audio/`, `engine/mixer.rs`)

The music is decoded once and mixed in the same callback that renders LTC, on the same clock.

- **Decoding** (`audio/decode.rs`, symphonia: wav, aiff, flac, mp3, ogg): every file becomes stereo, 48 kHz (rubato's
  FFT resampler converts other rates), 16-bit. Mono plays on both sides; channels beyond two are dropped. Sixteen
  bits keeps a three minute song near 35 MB in memory, where 32-bit float would take 70 MB.
- **Peaks** (`audio/peaks.rs`): the min and max of every 10 ms as signed bytes, which is what the timeline draws.
- **Commands**: `audio_analyze(asset)` returns `{ durationSeconds, sourceRate, peaksPerSecond, peaks }`; the UI calls
  it on import and when a project opens, so clip lengths and waveforms are real. It first looks for the preview stored
  in the `.fxtc` (`peaks/`, see [fxtc-format.md](fxtc-format.md)) and only decodes the asset (cached by asset id) when
  there is none or it was made from other audio; the result is kept for the next save. Opening a project whose
  previews are stored decodes nothing. `output_set_clips(project)` sends the current clips to a running output.
  `output_start` decodes whatever has not been decoded yet.
- **Mixing** (`mixer.rs`): clips of tracks that are not muted, placed by `Clock::frames_to_samples(clip.start)`, with
  `gainDb` as `10^(dB/20)`. The callback mixes in chunks of 256 frames held on the stack, adds the clips, clamps to
  -1..1 and writes left and right to `output.audio.channels` (one channel receives the mono sum); LTC then writes its
  own channel. A clip whose audio cannot be read is skipped and reported in `OutputInfo.warnings`, never an error.
- **Track gain and levels** (`mixer.rs`): each track's clips are summed, multiplied by the track's fader (`gainDb`
  absent is 0 dB, `-60` or less is silence) and added to the programme, so a clip sounds at track gain times clip
  gain. The fader lives in an atomic that `output_set_track_gain(trackId, gainDb)` writes without rebuilding the
  programme; the callback ramps from the gain the last chunk ended on to the new one over the next 256-frame chunk, so
  a drag never clicks. After the gain the callback keeps the largest absolute sample of each track in an atomic
  (`fetch_max`, no locks, no allocation). The levels lane reads and resets those peaks every ~33 ms and sends them
  as `[{ id, peak }]` with linear amplitudes (the UI converts to dBFS); a
  muted track is not in the clip set and has no peak. Silence is sent once after the audio stops and not repeated.
- **Updating while running**: the callback receives a new `Arc<ClipSet>` through the same channel as transport
  commands and swaps it in. `Output` keeps every set it ever sent until it stops, so the callback is never the one to
  free memory.
- **Real-time check**: every second the callback compares the audio it rendered with the wall clock; more than 1.5
  times faster sets `Position.clockUnpaced`, and the UI shows the clock lamp as faulty ("Not real time"). ALSA's
  null device ("Discard all samples") does exactly that and made the timecode run ~500 times too fast.

On Linux under WSLg only the null device exists until `libasound2-plugins` is installed, which gives ALSA a `pulse`
and `default` device that reach WSLg's PulseAudio. The cpal PulseAudio host was tried and could not complete the
handshake with WSLg's server, so it is not enabled.
