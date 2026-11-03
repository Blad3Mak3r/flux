# Implementation plan — next steps

Hand-off for a fresh Claude Code session (written on Windows, to be continued on Linux). Read `AGENTS.md` first: it is
authoritative for the stack, skills, agents and git workflow. This plan does not repeat it.

## Where things stand

- Stack: Rust + Tauri v2 (`src-tauri/`), React 19 + TypeScript + Vite (`src/`), npm.
- Done and in PR #1 (`feature/fxtc-project-format` → `main`, open, no CI, mergeable):
  - `.fxtc` project format (zip: `project.json` + flat `audio/`), Rust model, archive open/save, 25 tests.
    Spec: `docs/fxtc-format.md`; code: `src-tauri/src/project/`.
  - Tauri commands `project_new`, `project_open`, `project_import_audio`, `project_save`
    (`src-tauri/src/commands.rs`), TS types and wrappers with file dialogs (`src/project/types.ts`, `api.ts`).
- Not done: everything below. `src/App.tsx` and the `greet` command are still the Tauri template demo.
- Decided: Flux is the **master clock, generate only** (LTC out through audio, MTC out through MIDI). Following an
  external timecode (chase/input) is out of scope for now.
- Positions everywhere are **frame counts** at the project frame rate (`fps24 | fps25 | fps2997Drop | fps30`, the four
  rates MTC can express).

## Step 0 — set up the Linux machine and the repo

1. Tauri system packages (Debian/Ubuntu names): `libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev
   libssl-dev libayatana-appindicator3-dev librsvg2-dev`, plus audio/MIDI: `libasound2-dev` (cpal/midir use ALSA;
   `libjack-jackd2-dev` only if the JACK host is wanted). Rust stable, Node 24, `npm install`.
2. Create the symlink (it could not be created on Windows): `ln -s AGENTS.md CLAUDE.md`. Check
   `git config core.symlinks` is `true` and that `.claude/skills/*` links resolve (they point to
   `../../.agents/skills/<name>`; if they show up as plain files, re-checkout them).
3. Add `.gitattributes` with `* text=auto eol=lf`: the Windows checkout had `core.autocrlf=true` and printed CRLF
   warnings on every commit.
4. Review and merge PR #1 (after the user agrees). Branch each next piece from an updated `main`, one PR per piece.
5. Optional, ask the user first: a GitHub Actions workflow (fmt, clippy `-D warnings`, test, `npm run typecheck`, on
   ubuntu + windows + macos). The repo has no CI today, so PRs show no checks.
6. Smoke test the existing work for the first time: `npm run tauri dev` — the native file dialogs and the four project
   commands have never been run end to end, only unit-tested.

## Order of work

`A1 timecode math` → `B devices` → `A2 LTC/MTC encoders` → `A3 transport + output` → `C UI` → `A4 clips and triggers`.
`C` (UI) touches different files from `A`/`B`, so it can run in its own session in parallel once the design direction
exists. Never let two sessions write the same files.

## A. Timecode engine (`src-tauri/src/engine/`)

**A1 — timecode math** (pure, no I/O, the easiest to get exactly right; do it first)
- `Timecode { h, m, s, f }` ↔ frame count for each `FrameRate`, including 29.97 **drop-frame** (frames 0 and 1 are
  skipped at the start of every minute except every tenth; 29.97 DF has 107892 frames per hour).
- Tests with known values: 00:01:00;02 is frame 1800 in DF; 01:00:00;00 DF is frame 107892; round trips over a whole
  day for every rate.

**A2 — encoders** (pure functions, tested without hardware)
- **LTC** (SMPTE 12M): 80-bit biphase-mark frame per timecode frame — BCD digits, user bits, drop-frame flag (bit 10),
  sync word `0x3FFD` in bits 64–79. Generate samples at the device sample rate (usually 48 kHz), carrying the
  fractional samples per frame between frames, with a configurable level and edge shaping. Test by encoding a range of
  frames and decoding them again with a small test-only decoder; check the sync word, parity and the DF flag.
- **MTC**: quarter-frame messages (`0xF1`, piece number in the high nibble, 8 pieces = 2 frames, rate code in piece 7:
  0 = 24, 1 = 25, 2 = 29.97 DF, 3 = 30) and the full-frame SysEx `F0 7F 7F 01 01 hh mm ss ff F7` sent on start/seek.
  Quarter frames describe the position two frames earlier than when they are sent, so compensate. Test the exact byte
  sequences for a few positions.

**A3 — transport and output**
- Crates (add with `cargo add`, pick current versions): `cpal` (audio out), `midir` (MIDI out).
- The **audio device's sample clock is the master**: derive the transport position from a sample counter updated in the
  audio callback, and drive the MTC sender from that same clock so LTC, MTC and playback never drift apart.
- The audio callback must not allocate or lock: shared state (playing, position, rate) through atomics or a lock-free
  queue; control commands go through a channel.
- Transport: stopped / playing / paused, `play`, `pause`, `stop`, `seek(frame)`; start offset from
  `project.timecode.start`.
- Tauri commands `transport_play|pause|stop|seek`, `output_start|stop`, and a throttled event (~30 Hz) with the current
  position for the UI. Keep errors as messages like the project commands.
- Outputs read `project.output`: `ltc { deviceName, channel }` writes the LTC signal to one channel of the chosen
  interface, `mtc { portName }` to a MIDI port.

**A4 — clips and triggers** (after the UI exists)
- Decode audio with `symphonia` and mix the project's audio clips into the same audio callback (start in frames,
  `gainDb`). Fire `events` triggers when the transport crosses them: `marker` goes to the UI, `midi { bytes }` is sent
  to a MIDI port.

## B. Audio and MIDI devices (`src-tauri/src/devices.rs`)

- Commands `devices_list_audio` (every cpal host available — ALSA/JACK on Linux, WASAPI/ASIO on Windows, CoreAudio on
  macOS — with device name, output channel count, supported sample rates) and `devices_list_midi` (midir output ports).
  Provide a refresh so hot-plugged devices show up.
- Projects store **name hints**, never OS ids. Add a resolver: given a hint, find the device or report "missing" so the
  UI can ask for a replacement. A missing device must never make opening a project fail.
- **Model gap to fix first:** `project.output` has LTC and MTC but no device for playing the project's audio. Add
  `output.audio` (device name hint plus channel routing, e.g. LTC on channel 8 and programme on 1–2), update
  `model.rs`, the fixture test, `types.ts` and `docs/fxtc-format.md`. `formatVersion` can stay `1` because no release
  has shipped; if that stops being true, bump it and write a migration.
- Testing on Linux without hardware: load `snd-aloop` (loopback sound card) and `snd-virmidi` (virtual MIDI ports);
  `aseqdump -p <port>` shows the MTC bytes, `amidi -l` lists ports, and `ltcdump` (libltc tools) can decode the LTC
  signal recorded from the loopback. `midir` can also create virtual ports on Linux and macOS (not on Windows).

## C. User interface (`src/`)

Do **not** start coding the UI before the design direction exists. Use the `impeccable` skill: run `init` to write
`PRODUCT.md`, then its shape/new-work flow and `document` to write `DESIGN.md`. Then build with
`vercel-react-best-practices` (ignore Next.js-only advice).

Layout to aim for (REAPER-like, modern, dark-first): transport bar with a large timecode display; track headers on
the left (name, mute, type); timeline on the right with a timecode ruler, audio clips with waveforms, and event/cue
lanes; an inspector for the selected clip or trigger; menus and a settings dialog for devices and timecode (frame
rate, start offset).

- Wire it to `src/project/api.ts` (new / open / save / save as / import audio). Track the dirty state and ask before
  discarding unsaved changes. The `Project` is plain JSON, so immutable updates give undo/redo cheaply.
- Draw the timeline and waveforms on canvas (performance); keep headers and inspector in the DOM. Compute waveform
  peaks in Rust at import time and send downsampled peaks to the UI; decide whether to cache them in the `.fxtc`.
- Keyboard first: space = play/pause, Ctrl/Cmd+S/O/N, arrows to nudge by frames.
- Remove the template demo (`greet` command, demo `App.tsx`, `App.css`, `src/assets/react.svg`) when the real shell
  replaces it.
- Keep dependencies few; pick a state library only if plain React state stops being enough.

## Working rules for every piece

- Skills: `ponytail` (smallest change that solves it) on every coding task, `rust-best-practices` for Rust
  (`thiserror`, no `unwrap` outside tests, descriptive test names), `vercel-react-best-practices` for React.
- Pure logic first and unit-tested (timecode math, encoders, resolvers); hardware-facing code stays thin.
- Checks before every PR: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` from
  `src-tauri/`; `npm run typecheck` from the root; `npm run tauri dev` for anything touching the UI or commands.
- Update `AGENTS.md` (structure, commands) and the relevant doc in `docs/` as each piece lands.
- All git writes and PRs go through the `git-operator` agent: English, no AI attribution, branch
  `<type>/<description>`, incremental commits, never straight to `main`.

## Open decisions (ask the user, do not assume)

- Sample rate policy (48 kHz default?) and LTC level / edge shaping defaults.
- Supported audio formats beyond wav/aiff/flac/mp3/ogg, and whether to convert on import.
- Whether to add a non-drop 29.97 rate (MTC cannot express it, LTC can).
- Chase/input of external LTC/MTC (explicitly deferred).
- Trigger actions beyond `marker` and `midi` (OSC, scripts, …).
- CI and release/packaging (installers per OS, versioning of the app vs. `formatVersion`).
