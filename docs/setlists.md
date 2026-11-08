# Setlists and show operation

A setlist is an ordered collection of references to timelines. It does not copy their audio,
tracks or timecode settings. The same timeline can appear several times, and entries can be
omitted without removing their timelines. Projects without a prepared setlist keep the existing
timeline transport and end-of-timeline behavior.

## Preparing and running

Edit > Setlists opens a dedicated workspace with the running order on the left and the selected
entry's properties on the right. New setlist creates a show; Add entry references an existing timeline.
Open timeline returns to that entry's source for detailed editing. Each entry has its own optional name,
operator notes and end action: prepare the next entry and wait for GO (default), continue, or
continue after a wait in milliseconds. Live > Prepare show decodes the included timelines and
opens their required routes. Unavailable audio, conflicting routes and unavailable MIDI/OSC
destinations fail preparation with an error. A timeline needs a nonzero duration, including
its control regions.

Rows summarize the exact transition wait, manual timeline waits, loops until GO and counted loops.
An aliased entry also shows its source timeline. Select an entry and use the sticky Move up, Move down
or Move to controls above the running order. Alt+Up/Down moves the focused entry row. The selected
entry stays selected as its position changes; backend edits retain normal undo/redo behavior.

The prepared run retains all included audio, sharing clip sets for repeated timeline references.
Preparation trades memory for predictable playback: it does not stream/decode the next song
inside an audio callback. Leave and prepare again after editing the project or changing hardware
routes. Prepared audio must fit in available memory.

Live shows the current entry, the next entry, operator notes, state, loop turn and the next GO
action. A prepared show's numbered running order replaces the project timeline tabs. It distinguishes
current, next, passed, upcoming and omitted entries, preserving repeated references and display names.
While stopped, selecting an included entry prepares it; omitted entries remain unavailable. Show options
expands the leave action. Unprepared Live keeps ordinary project timeline navigation.
Enter in Live runs GO, except in editable fields, buttons or dialogs; held keys do not
repeat it. GO, Devamp and Cancel devamp can also be learned from MIDI in Preferences.

| State | GO |
| --- | --- |
| Ready | Start the prepared entry |
| Paused or waiting at a timeline control | Resume |
| Looping | Finish this turn, then continue beyond the loop |
| Loop exit pending, normal playback, transition wait, completed or blocked | No extra advance |

Pause keeps the position, loop turn and pending exit. Stop returns to the current entry's start
and cancels its pending controls. Preparing a different entry requires Stop. Leave setlist also
requires Stop and returns to ordinary timeline operation. Structural edits are locked during a
run, including paused/manual waits; stop and leave the setlist to return to Edit.

## Timeline controls

Controls use positions relative to the timeline origin. They appear in their own rail below the
ruler. Click a control, or Loops & waits at the left of that rail, to open its inspector. Add wait
and Add loop create a control at the playhead; a new loop initially lasts one second. Position
and end use the same timecode fields as other timeline properties:

- **Wait for GO** pauses the entire timeline at its frame, including timecode.
- **Loop** repeats `[start, end)`. Choose Until GO for indefinite repetition or a fixed number
  of total turns, including the first turn. Cancel devamp withdraws the requested exit.

The selected loop's highlighted rail spans its full start-to-end range. The inspector lists relative
timecode ranges and repeat mode for every control. Its persistent note distinguishes ordinary Edit
audition from prepared-setlist execution, so controls are not mistaken for Edit transport automation.

Controls cannot overlap, nest or share boundaries. Waits inside a loop are rejected. Finite loops
have a calculated duration; manual waits and indefinite loops show an indeterminate duration.
Controls belong to their timeline and execute in prepared setlist mode. Ordinary Edit audition
keeps its existing free transport.

Every loop repeats audio, operator audio, metronome, MIDI/OSC and LTC/MTC together. Receivers
must support the backward timecode jump. MIDI/OSC events fire once per crossed interval; the
event at a loop's end belongs to the following section. Each event's inspector can limit it to
the first loop turn. A manual seek does not fire skipped events. Pending events are invalidated
on manual stop, pause, seek and entry preparation, and held MIDI notes are released.

## Timing and faults

The master audio callback splits buffers at controls and transitions, carrying remaining samples
into the next timeline. Each entry retains its frame rate and timecode start. Outputs are kept
open; no window, UI timer or network packet schedules an automatic transition. There is no audio
crossfade or overlap between timelines.

Secondary audio interfaces follow the master's published position with the existing bounded
resampling cursor. Independent hardware clocks are not phase locked. MTC and MIDI/OSC remain
worker-thread outputs and cannot promise hard real-time delivery from a desktop OS. A bounded
render journal preserves every loop interval for cue workers, including several turns within
one audio callback. Overflow or output failure blocks the run rather than silently dropping
required events. Device recovery never resumes the show automatically; stop, leave and prepare
again after fixing the route.

## Validation

Automated tests cover archive/recovery persistence, rejected edits, finite/indefinite controls,
exact waits, devamp, consecutive samples across timeline/frame-rate changes, multiple loop
intervals per buffer, legacy OSC commands and TCP framing. Standard frontend and Cargo checks
also apply. Actual interface timing, MIDI/LTC receiver behavior and visual review are separate
acceptance checks; automated tests do not certify those devices or the native UI.


Validation in this Windows worktree: `npm run typecheck`, `npm test` (169 passed),
`npm run build`, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -j 1 -- -D warnings`, and
`cargo test --workspace -j 1 --quiet` (706 passed, one intentionally ignored existing
subprocess helper exercised by its parent recovery test). Socket tests required execution
outside the restricted sandbox. The revised setlist workspace, control inspector and Live layout
were visually reviewed in a browser using actual components with synthetic show data, in English
and Spanish at desktop widths down to 960px. The control-creation addition passes all 238
`flux-project` tests. No real interface, LTC/MTC receiver or external-controller acceptance
run was performed. No dependency was added.

The follow-up workflow pass adds backend coverage for repeated/aliased/omitted entry summaries,
exact delays, control counts, movement positions and current/next/completed running-order state.
`cargo test -p flux-project -p flux-engine --quiet` passes 239 project tests and 327 engine tests;
UDP tests required loopback access outside the restricted sandbox. Frontend typecheck/tests/build,
workspace Clippy and Rust formatting also pass. Actual components were visually checked with
synthetic data and simulated IPC at 960x760, 1280x900 and 1440x1080, including Move down, Move to
and Alt+Up interactions. English and Spanish Live were checked. Following entries/cues is confined
to their list so it does not automatically scroll the surrounding show out of view. A full keyboard,
contrast, screen-reader and hardware acceptance run remains separate from this four-issue pass.
