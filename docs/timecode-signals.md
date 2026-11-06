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
  position in the project; positions before the active timeline's `timecode.start` go to the start.
- **End of the show.** `set_end(samples)` gives the transport the latest end among all clips and cues. When playing
  reaches it, the transport stops by itself exactly like `Stop`: state `stopped`, back at the start (so the next Play
  runs the show again), epoch changed so LTC and MTC resync; the `transport-position` lane reports it as a state
  change, not as an error. The check runs once per audio buffer, so LTC can run on for less than one buffer past the
  end; there is no extra tail after the last item (the end is exact, and a later configurable tail can be added
  on top). A show with no end (no clips and no cues, or everything at the origin) never stops by itself, so the
  generators can run without a programme. A seek past the end goes to the end (playing from there stops at once).
  The end can change while playing: `output_set_clips` sends a new one with the clips, and an end that moved away
  is simply not reached. A clip's end uses its exact decoded length, shortened by its `length` when it has one; muted tracks count, so muting does not move
  the end of the show. The end comes from `CueList::end_samples` and travels in `ClipSet::with_end`.
- `epoch()` changes whenever playback starts or the position jumps (start, stop, seek) but not on pause. Generators
  compare it with the last epoch they saw and call `resync`.
- `LtcStream` plays LTC in step with the transport: silence until the next frame boundary, then one frame after
  another from a preallocated buffer, so `fill` is safe in the audio callback.

## Timelines

A project can hold several timelines, each with its own frame rate, start and tracks, but **one plays at a time**: the
output, the transport and every LTC and MTC signal run the **active** timeline, which the backend's `Session` keeps.
`live_cues`, `live_state`, `output_start` and `output_set_clips` take the project but not a timeline: the backend picks
the active one. The `timeline_*` commands (`timeline_add`, `timeline_rename`, `timeline_remove`, `timeline_select`,
`timeline_set_timecode`, `timeline_move`) take the project the window shows, make it the document, do the work and answer with
`{ project, active }`. Changing the active timeline, removing one or changing its timecode is refused while the
transport plays, and it closes the running output (the rate and the start are fixed when the output opens), so the
window opens it again: the transport then stands at the start of the new timeline. `timeline_move` (reordering)
is refused while the transport plays too, like changing the view, but it leaves the open output alone.

**In Live, the next timeline is made ready when one ends.** The window opens the output with `advance` set when the
Live view is showing. When the show then runs to its end by itself (a stop from the user does not count: the transport
counts those ends, see `PositionReader::shows_ended`), a lane of the output makes the following timeline the active one
(`Session::next_timeline`) and emits the `timeline-advanced` event with the `{ project, active }` it left. The window
shows it and opens the output again for the new timeline, which is stopped at its start: it is never started for the
user. After the last timeline nothing changes and the show stays where it ended.

## Cues and the Live view (`engine/cues.rs`, `live_commands.rs`)

The Live view runs the timeline on stage, and the backend does all of its thinking; the UI only draws what it is
sent. A **cue** is not stored in the project: every clip of an audio track and every note of a notes track is one,
so the `.fxtc` format does not change. `CueList::build` derives the list, ordered by frame (cues at the same frame keep
the order of their tracks) and numbered from 1. A clip's length comes from its decoded audio, trimmed by its `offset` and `length`; a file that cannot be read
gives a cue without length, never an error. `Cue.frame` is the absolute timecode frame, so the UI seeks to it as it is.

`CueList::state_at(frame)` gives the `LiveState` for a position. Each cue is `pending`, `next` (the first cue still to
come, the one on standby), `running` (a clip from its first frame to the one before its end), `done` or `muted` (its
track is muted, so it never plays or queues). The state also carries `current` (the sounding clip that started last, or
else the last cue done), the countdown to the next cue and, for each running clip, the time elapsed, the time left and
the progress. Every time is already written as timecode (`;` before the frames in drop-frame).

**Notes** are cues of kind `note` that carry their text (`Cue.text`; `Cue.name` is its first line). A note is on show from
its frame for its `length`, or, without one, until the next note of its track that starts later, or to the end of the
show. `LiveState.notes` lists those on show, with the time left of one that has a length, so the Live view can write
them large for the operator. A note with a length also counts as `running` while it is on show. Notes of a muted track
are neither shown nor queued. Nothing is fired by a cue yet: starting video, Art-Net, MIDI or OSC will be new track
kinds.

| Command | Arguments | Result |
| --- | --- | --- |
| `live_cues` | `project` | The `Cue` list. |
| `live_state` | `project`, `frame` | The `LiveState` at that frame, for when no output runs and the playhead is parked in the window. |

With an output running, the `flux-live` lane sends the same `LiveState` over its channel; the output keeps a cue list
that `output_start` and `output_set_clips` rebuild, which the lane swaps in whole like the meters' state.

## Metronome (`engine/metronome.rs`)

The metronome clicks on every beat of the active timeline's `tempo`, counted from the timeline origin; beat 0 of each
bar is the accent (higher pitch). A click is a pure function of the transport position (a 30 ms decaying sine placed at
`round(beat * 60 / bpm * rate)`), so it keeps no state: any buffer holds the clicks the grid puts in it, and a seek lands
on the grid at once. It sounds only while the transport plays.

- **Own output.** `output.metronome` names a device, channels and level apart from the programme's (see
  [fxtc-format.md](fxtc-format.md)). The tempo and the click switch live in atomics (`SharedMetronome`) that
  `output_set_tempo(tempo)` writes while the show runs, so an edit of the BPM, the beats per bar or the click switch is
  heard at once; the level and the device are fixed when the output opens (changing them reopens it).
- **Which stream plays it.** No device is opened twice: on the programme's device the click is mixed in by the
  `Renderer` (a metronome channel the programme also uses gets the click added; one it does not use carries the click
  alone); on the device of an own LTC stream it is written by that stream's `LtcFollower`; on any other device a stream
  of its own (`flux-click`) is a follower with no LTC outputs. If the device cannot be opened, the output still starts
  and `warnings` says so; `OutputInfo.metronome` reports `{ device, ok }`. A click stream that fails while running is
  only logged, there is no lamp for it yet.
- **Following the master.** A follower reads the master's published position like LTC does, but starts the click over
  from it only when 10 ms or more away (an LTC frame, 40 ms, would be too coarse for a click). The two devices have
  independent clocks and no latency compensation, so the click can sit a few milliseconds off the music.
- A click and LTC on the same channel of one device: LTC wins.

## Tempo detection (`audio/tempo.rs`)

`audio_detect_bpm(asset)` returns the BPM of an asset, or `null` when it is too short (under 4 s), silent, or shows no
steady pulse. It uses the decoded audio when the cache already holds it, and otherwise decodes the file without keeping
it, so asking for every song of a big show does not fill the memory. The UI asks for it after importing audio, and
in the background, one song at a time, for the assets of an opened project that have none; the result is stored on the
asset (`assets[].bpm`), so a project that was saved with it decodes nothing. A tempo found for an opened file is not
an edit: it does not enter the undo history nor mark the document unsaved, and is written with the next save. The inspector can detect
again, type another value, or copy it to the timeline's tempo.

The method: loudness every 512 samples, its rises (onset strength), a blur, the autocorrelation over 55 to 210 BPM
weighted towards 120 BPM, and a refinement that slides a comb of beats over the onsets at 0.1 % steps. It reads at most
120 s from the middle of the audio. It finds the pulse of music that has one; the choice between a tempo and its double
is made by the weighting towards 120 BPM, so an evenly spaced pulse at 174 BPM reads as 87. It was checked on synthetic
drum patterns, not on a library of real recordings.

## Defaults and open decisions

`LtcConfig::default()` is 48 kHz, level 0.5 (-6 dBFS) and a one-sample edge. These can become project settings later.
With no offset set, frame N starts exactly where the transport reaches frame N. Each LTC and MTC output can run ahead
of or behind the transport by an offset of its own (below).

## Output and transport commands (`output.rs`, `transport_commands.rs`)

`output_start(project, onPosition, onLevels, onLive)` opens the devices `project.output` names and starts a stopped transport at
`project.timecode.start`; it replaces a running output. TS wrappers are in `src/transport/api.ts`.
The UI decides what `project.output` the backend sees: in the Edit view, a project with `output.preview` is sent with that
device as its only output (no LTC or MTC), so auditioning never emits timecode; Live sends the project's outputs as they are.

| Command | Arguments | Result |
| --- | --- | --- |
| `output_start` | `project` (its active timeline plays), `advance` (hand over to the next timeline when the show ends), `onPosition`, `onLevels`, `onLive` (channels, below) | `OutputInfo`: `device`, `sampleRate`, `channels`, `ltcChannel`, `ltcDevice` (only when LTC has a device of its own), `mtcPort`, `warnings`. |
| `output_stop` | | Closes the devices. |
| `transport_play` / `transport_pause` / `transport_stop` | | Stop goes back to the timeline start. Error if no output runs. |
| `transport_seek` | `frame` | Moves to a timecode given as a frame count. |
| `transport_position` | | `Position` (`state`, `frame`, `positionSamples`, `deviceLost`, `ltcLost`, `clockUnpaced`) or `null` when no output runs. |

While an output runs it reports over three lanes, each with a thread of its own and the `Channel` that
`output_start` was given for it, about 30 times a second each:

| Lane | Thread | Channel (`output_start` argument) | Sends |
| --- | --- | --- | --- |
| Position | `flux-position` | `onPosition`: `Channel<Position>` | The `Position`, only when it moved. |
| Live | `flux-live` | `onLive`: `Channel<LiveState>` | Where the show stands among its cues (see Cues and the Live view), only when it changed. |
| Levels | `flux-levels` | `onLevels`: `Channel<[{ id, peak }]>` | The peak of each track since the previous message; silence once after the sound stops, then nothing. |

The lanes are independent, so a slow consumer of the meters never delays the timecode. A channel is ordered,
goes to the window that opened the output and ends with it, which a broadcast event does not; the UI also drops a
message that belongs to an output it has closed since.

### Threads

No command runs on the main thread, which belongs to the window and its event loop. Tauri runs a command without
`async` inline on that thread, so the small ones (`transport_*`, `output_set_track_gain`, `output_set_tempo`) are
`#[tauri::command(async)]` and use the command thread pool; the heavy ones (`output_start`, `output_stop`,
`output_set_clips`, `project_*`, `devices_*`, `audio_analyze`) use the blocking pool. The rest:

| Thread | Work |
| --- | --- |
| `flux-audio` | The audio callback: programme, LTC when it shares the device, position. Never allocates or waits. |
| `flux-ltc` | The LTC callback, when LTC plays on a device other than the programme's (it carries the metronome too when that device is the metronome's). Never allocates or waits. |
| `flux-click` | The metronome callback, when the metronome plays on a device nothing else uses. Never allocates or waits. |
| `flux-mtc` | MIDI Timecode quarter frames. |
| `flux-position`, `flux-live`, `flux-levels` | The three lanes above. |

Commands that set the transport only enqueue a message for the audio callback, so two of them issued back to back
from the UI are not guaranteed to run in order; the UI awaits the ones that depend on each other (Play waits for the
output to open) and sends dragged seeks and faders one at a time, newest value last.

- The programme device is `output.audio`'s, or the system's default output when there is none (an empty
  `deviceName`); a project written before `output.audio` existed has it on the LTC device. It is the master: it
  carries the programme and the clock, and LTC too when `output.ltc` names the same device (compared by the device
  each hint opens, so an empty name and the default device's name are the same). Neither LTC nor MTC is needed to
  play the music. The stream opens at 48 kHz with the fewest channels that cover the highest channel used, in 32-bit
  float, else 32 or 16-bit integer. The programme (below) uses the same stream.
- **LTC on its own device**: when `output.ltc` names another device, a second stream (`flux-ltc`) plays only LTC.
  Two devices have two sample clocks that drift apart, so `LtcFollower` does not count samples against the
  transport: each callback reads the master's published position (`try_load`, never waiting; extrapolated with the
  wall clock like MTC) and plays LTC from there. It starts the LTC stream over at the master's position when the
  transport seeks, stops or resumes, or when the two differ by a frame or more, which at 50 ppm of clock difference
  is about every 13 minutes at 25 fps. That start-over costs one LTC frame of silence and a jump of a frame.
  The two devices' output delays are not measured, so LTC and the music can be offset by the difference between them
  (ms); the output's `offsetMs` (see Latency offset) compensates it by hand. If the LTC device is missing or
  cannot be opened, the output still starts without LTC and `warnings` says so (like MTC); if it fails while
  running, `Position.ltcLost` is set.
- The audio device's sample clock is the master. The callback owns the `Transport`, drains commands with
  `try_recv`, renders LTC into its channel, advances the position and publishes it with `try_lock`: it never
  allocates or waits. A reader holding the lock only makes it skip one update.
- MTC runs on a thread that parks until shortly before each quarter frame is due (1 ms at most per park) and spins for the
  last 200 µs, so the message goes out when it falls due and not up to a timer tick later. It reads the published position
  and extrapolates with the wall clock (at most 100 ms), so quarter frames are not quantised to the audio buffer. It sends
  a full frame whenever the transport epoch changes. A missing MIDI port becomes a warning in `OutputInfo`; the audio side
  still starts. Every MTC output has a thread of its own; two ports with the same name are told apart by `portIndex`.
- The audio device always provides the clock, so an MTC-only project still opens one (the default output).
- A park can still overshoot by the system's timer resolution (about 15 ms on Windows unless it is raised), and a quarter
  frame that falls due in that time goes out late; the spin only removes the jitter of the last 200 µs.
- Each LTC output has its own level (`output.ltc[].level`): one generator runs at full scale and every channel
  it is written to applies its output's level.
- **Latency offset.** Each LTC and MTC output has an optional `offsetMs` (whole ms, within ±1000, positive = emitted
  earlier). What goes out when the transport is at position `p` is the timecode of `p + offset`; the displayed time and
  the transport are untouched, and 0 or absent behaves as before. `LtcStream::with_offset` and
  `MtcScheduler::with_offset` apply it inside the generator, so seeks, stops and resumes (which all `resync`) and the
  follower's tracking need no special case. LTC before the timeline origin (a negative offset near 0) is silent until the
  timecode reaches it. A stream has one generator per distinct offset, so LTC outputs that share an offset share one;
  a device and channel listed twice plays once with the first output's level and offset, and a repeated MIDI port keeps
  the first entry's offset. The Setup dialog sends the typed text to the `output_set_offset` command (kind, index, text);
  the backend parses it, checks the range, stores 0 as no offset and answers with the project, which the UI shows.

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
  own channel. A clip plays from `Clock::frames_to_samples(start)` up to its `length` (or the end of its audio),
  reading its audio from the `offset`; `mixer::span` gives that range and the cues use it too, so both agree. A
  `fadeIn` / `fadeOut` is a linear gain per sample: silent on the first sample of a fade in and the last of a fade out.
  A clip whose audio cannot be read is skipped and reported in `OutputInfo.warnings`, never an error.
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
