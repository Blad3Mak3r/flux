# Remote control

Flux can be run from a MIDI controller (an AKAI APC, any pad or button surface) and from OSC (a Stream Deck, QLab, a
console). Flux stays the **master clock**: remote control starts, stops and moves it, it never follows another timecode.
The one exception is the clock of a VJ show, which can show the LTC or MTC another machine sends; its input is chosen in
**Preferences > Remote control > Timecode input** (see [Video](video.md)), and Flux's own transport still does not chase it.

It is set up in **Preferences > Remote control**. The choices belong to the machine (a pad layout follows the
controller, not the show), so they are **not** in the `.flx`: they live in `control.json` in the app data folder
(`com.fluxtimecode.app`). A missing or unreadable file means the defaults, and a controller that is unplugged is a state
the page shows ("Missing"), never an error.

## Actions

| Action | What it does |
| --- | --- |
| Play, Pause, Play or pause | Moves the transport. Play with no output open lets the window open it and play. |
| Stop | Stops and goes back to the start of the timeline. |
| Next cue, Previous cue | Seeks to the first unmuted cue after the position, or the last before it. "Previous" within one second of a cue goes to the cue before it. |
| Go to cue `n` | Seeks to the cue with that number (from 1, as the Live view numbers them; a muted cue is still a place). |
| Move by frames / seconds | Moves the position by a number, negative goes back, held inside the timeline. |
| Next / previous timeline, Go to timeline `n` | Changes the active timeline, as when a show ends and the next is made ready. Refused while the show plays. |
| Edit view, Live view | Switches the view, as Ctrl+1 / Ctrl+2. Refused while the show plays, and while the show is locked (Edit only). |
| MIDI panic | Stops the transport, then sends All Sound Off, Reset All Controllers and All Notes Off on all 16 channels of every MIDI port the project uses (its MIDI tracks and MTC outputs). It works with no output open and is never refused by the show lock. The window has it as the press-and-hold button of the Live bar, Transport > MIDI panic and **Ctrl+Shift+P** (Cmd+Shift+P on macOS). |

Cue, step and seek actions need an output open (it is the transport's). Without one they are ignored and logged.

## MIDI

Choose the input port, press **Learn** after picking an action, and touch the pad. Notes (velocity above 0), control
changes (value above 0) and program changes trigger; releases and the rest of the traffic a controller sends are ignored.
One pad runs one action: learning a pad that already has one replaces it. Ports are remembered by name, and ports with
the same name are told apart by their place, as the MTC outputs do.

## OSC

Enable it and pick a UDP port (default **9000**). The addresses are fixed:

| Address | Argument |
| --- | --- |
| `/flux/play`, `/flux/pause`, `/flux/toggle`, `/flux/stop` | none |
| `/flux/cue/next`, `/flux/cue/prev` | none |
| `/flux/cue/goto` | cue number, from 1 |
| `/flux/seek/frames`, `/flux/seek/seconds` | number, negative goes back |
| `/flux/timeline/next`, `/flux/timeline/prev` | none |
| `/flux/timeline/select` | timeline number, from 1 |
| `/flux/view/edit`, `/flux/view/live` | none |
| `/flux/panic` | none: MIDI panic |

Numbers can be sent as an integer, a float (rounded) or a string. Actions without a parameter ignore any argument, because
a button usually sends a `1`. Bundles are accepted and their time tags ignored. In a Stream Deck, use an OSC / UDP plugin
pointed at this computer's address and the port, with one of the addresses above on each button.

## VJ shows

In a VJ show the same two inputs drive the grid. The mappings are **part of the show** (unlike the cue bindings above,
which belong to the machine): they are learned in the [map mode](vj-mode.md#map-mode-midi-osc-dmx-and-keys) (Ctrl+Shift+M), listed in
the Mappings tab and saved in the `.flx`. What is heard is checked against the mappings **first**; a message with no
mapping falls through to the cue bindings and the `/flux/...` addresses, so a show without mappings behaves as before.
The `/flux/v1/...` addresses are not mappable.

**MIDI sources.** A note (press above velocity 0, release at 0), a control change (value 0 to 127) or **pitch bend**
(14 bits, 0 to 16383, centre at 8192), on one channel. A source with a port is matched to that port by name; one without
accepts any. A continuous source goes to `min` through `max` of the target; a button is down for any value above 0 (a note with velocity above 0, a control change above 0) and, for pitch bend, past
the centre.

**OSC.** Any address can be learned, and the arguments read are the first number (an integer, a float, a boolean or a
string that parses). The addresses below also work without learning, as in Resolume; they count from **1** by position.
Layers count from the **bottom** of the grid; columns and clips are those of the deck shown (a global layer's clips by
their slot).

| Address | Value | Does |
| --- | --- | --- |
| `/composition/layers/{n}/clips/{m}/connect` | 1 press, 0 release | Launches the clip at layer `n`, column `m` (a piano clip stops on release). No value is a press. |
| `/composition/columns/{m}/connect` | 1 | Launches column `m`. |
| `/composition/layers/{n}/video/opacity` | 0 to 1 | The layer's Video fader. |
| `/composition/layers/{n}/master` | 0 to 1 | The layer's Master. |
| `/composition/layers/{n}/audio/volume` | 0 to 1 | The layer's Audio level. |
| `/composition/layers/{n}/transition/duration` | 0 to 1 | The transition time, scaled over 0 to 10 s. |
| `/composition/layers/{n}/bypassed` | 0 or 1 | Sets the layer's bypass (1 and above 0.5 is on). |
| `/composition/layers/{n}/solo` | 0 or 1 | Sets the layer's solo. |
| `/composition/layers/{n}/clear` | 1 | Stops the layer. |
| `/composition/groups/{g}/video/opacity` | 0 to 1 | The group's Video fader. Groups count from the bottom of the grid. |
| `/composition/groups/{g}/master` | 0 to 1 | The group's Master. |
| `/composition/groups/{g}/bypassed` | 0 or 1 | Sets the group's bypass. |
| `/composition/groups/{g}/solo` | 0 or 1 | Sets the group's solo. |
| `/composition/crossfader/phase` | 0 to 1 | The crossfader, 0 all side A, 1 all side B. |
| `/composition/tempocontroller/tempo` | beats a minute | The tempo, held to 20 to 400. |
| `/composition/tempocontroller/tempotap`, `resync`, `plus`, `minus`, `double`, `half` | 1 | The tempo buttons (tap, resync, +1, -1, ×2, ÷2). |
| `/composition/tempocontroller/pause` | 0 or 1 | Holds the beats while 1. |
| `/composition/decks/{d}/select` | 1 | Shows deck `d`. |
| `/composition/decks/next`, `/composition/decks/prev` | 1 | Next or previous deck. |
| `/composition/disconnectall` | 1 | Stops every layer. |
| `/composition/dashboard/link{k}` | 0 to 1 | Turns dashboard knob `k` (1 to 8). |
| `/composition/layers/{n}/dashboard/link{k}` | 0 to 1 | Turns knob `k` (1 to 8) of layer `n`'s dashboard. |
| `/composition/layers/{n}/clips/{m}/dashboard/link{k}` | 0 to 1 | Turns knob `k` of the dashboard of the clip at column `m` of layer `n`. |

The tempo addresses also work as `/tempocontroller/...`. A message to a layer, column or deck that does not exist is
ignored.

**Execution.** The `flux-control` thread resolves a message to an edit of the show through the same operations the
window uses (`flux_project::vj_mapping::plan`), so a mapped control does exactly what its on-screen twin does. Fast
moves are coalesced: the thread applies every value as it comes but sends the window a state at most every 33 ms
(`vj-control`), and the whole sweep, which ends 500 ms after the last value, is one undo step. A button press is its
own step. Learning is `vj_learn` (the target waited for, `null` to stop); the learned source arrives as `vj-learned`.
Nothing is logged from the MIDI callback or the socket thread.

**Art-Net, keys and files.** DMX channels and keys are mapping sources too, described below; a mapping set can be
moved to another show as a file (see [Mapping files](#mapping-files)).

**Not done.** The pads of a controller are not lit from the state of the grid (the light feedback below is for cue
bindings only).

## Art-Net

A lighting console can drive the VJ grid like a MIDI controller. Enable it in **Preferences > Remote control > Art-Net**
(off by default). Flux listens on **UDP port 6454**, the protocol's own, on every interface (or the one you type;
note that a unicast address does not receive broadcasts), and shares the port with other programs (`SO_REUSEADDR`), so
a console monitor on the same machine does not stop it. The page shows the state (**Listening**, or the reason, such as
a bad interface address) and counts the **packets** received. An optional **universe** keeps only that Port-Address.

Only **ArtDmx** (opcode `0x5000`, Art-Net 4 header `Art-Net\0`, protocol 14 or later) is read; polls, replies, other
opcodes and cut or malformed packets are ignored. A universe is its 15-bit **Port-Address** (net, sub-net, universe),
shown and stored as on the wire: many consoles number universes from 1, so universe 1 there is `0` here, and the
console's second universe is `1`. Up to 512 channels.

**Sources.** A channel is a level (value / 255) or, with a 16-bit pair, (coarse × 256 + fine) / 65535: set the **16-bit**
field of the mapping to the channel of the low byte. As a button a channel is pressed from **128**. A console sends every
channel about 44 times a second, so Flux keeps the last frame of each universe and only a **change** is heard: a held
button presses once, a fader that rests says nothing, and the first frame of a universe is only the state to compare
with, so connecting to a console that is already running presses nothing. The source shows as `DMX 1:37` (universe:channel;
`DMX 1:37/38` with a fine channel).

**Learning.** In the map mode click a control, then move the fader or press the button **on the console**. Because every
channel arrives all the time, the one learned is the channel that **travelled furthest** (the span between the lowest and
the highest value it took since the learn began), when two travel alike the one that changed more often, then the lowest.
A channel counts only after it moved at least 10 steps of 255, so the flicker of a dithering fixture is not taken, and
the choice is made once the console has been still for 350 ms (or 1.5 s after the first channel qualified, if something
keeps moving). Move one control and leave the others alone. While a learn waits, DMX changes do not run mappings.

## Keyboard

The computer's keyboard is a source too, as in Resolume. A **key** source is the key's place (`KeyboardEvent.code`, so it
does not depend on the layout) with Ctrl, Alt, Shift and Cmd as held, shown as `Key Shift+Q`. Trigger, toggle and hold
work as for a pad: the window tells the backend when a key goes down and up (never the repeats of a held key), and the
backend runs the mapping; Hold uses the release, so a Piano clip stops when the key is let go.

- The keys go to mappings only in a VJ show, with no dialog open and while the focus is **not in a text field**, and not
  in map mode (where keys edit). The window forwards a key only when the show has a key mapping (and while a learn waits).
- To learn, click a control in map mode and **press the key**. **Esc** cancels the learn and is never a source.
- **Keys the app uses cannot be learned.** The choice is to refuse rather than let a mapping win only outside map mode,
  because a key that does one thing in map mode and another in the show is a surprise on stage. Refused are
  only the keys Resolume also has by default (see [Keys](vj-mode.md#keys)): **Ctrl or Cmd** with N, O, S, Z, Y, K, L or T,
  **Ctrl or Cmd and Shift** with S, C, K, M, O, X or A, **Escape**, and a modifier on its own.
  The learn says "X is used by FLUX. Press another key." and goes on waiting. Everything else is free: plain keys (Space, Enter, B, S, X, the digits, the arrows...),
  Shift and Alt combinations, other Ctrl combinations, F-keys and symbols.

## Mapping files

A show's mappings and dashboard links can be written to a **`.flxmap`** file and read into another VJ show
(**Mappings tab > Export / Import**, or **Composition > View**). The file names targets by **position** (layer 2, column 3,
effect slot 1 of a layer) instead of id so it fits another project with a different layout, and also keeps the ids, which
a file read back into its own show uses first. Sources stay as they are (MIDI port hint, OSC address, DMX universe and
channel, key). Import resolves positions in the open show and **skips what does not fit** (a layer that is not there, an
effect setting the effect in that slot lacks); the notice says "12 imported, 3 skipped, 0 kept".

When an incoming mapping would overwrite one the show has (the same source, or the same target) a dialog lists every
conflict (source, what it drives now, what it would drive) with a choice for each, **Overwrite** or **Keep mine**, and
four actions: **Overwrite all**, **Keep existing** (import only what does not clash), **Replace all mappings** (wipe the
show's mappings and links, then import) and **Cancel**. Without conflicts it imports at once. The import is one undo
step. The format is in [fxtc-format.md](fxtc-format.md#mappings-file-flxmap).

## How it works

- `flux-engine/src/control.rs` decodes MIDI and OSC, maps them to a `ControlAction` and finds the frame of a cue action. It
  is pure and tested without hardware. `control_in.rs` holds the MIDI input port and the UDP socket.
- `src-tauri/src/control.rs` keeps the preferences, opens the inputs when the app starts or the preferences change, and runs
  the `flux-control` thread that executes the actions. The inputs live as long as the app, not as long as an output: the
  output is closed and reopened on every timeline change.
- What only the window can do (open the output to play, switch the view) is sent to it as the `control-action` event; a
  timeline change reuses `timeline-advanced`. While learning, the next trigger is sent as `control-learned` and not run.
- Commands: `control_get`, `control_set`, `control_learn`.
- The preferences hold `midi.feedback` (off by default) and, per binding, an optional `feedback: { on, off }`.
- **Logging.** The MIDI callback and the socket thread only decode and send; they never log. `flux-control` logs the inputs
  coming and going (`info`, `warn`) and each action (`debug`).

## Light feedback

A controller that does not show state is easy to misread on stage, so Flux can send the state back and light the pads
that were learned. It is **off by default**: turn on **Light the pads with the state of the show** in Preferences >
Remote control. Flux then opens the **output port with the same name** as the input port you chose (a controller has
both) and writes to it, so the port has to be free: another program holding it keeps the lights from working, and Flux
retries every two seconds and says so in `flux.log`.

Only learned pads are written, and only a pad whose state changed. When the lights start, every learned pad is written
once, so the controller begins in step with the screen, and when they stop (feedback turned off, the controller changed,
the app closed) the lit pads are put out.

| Action | Lit when | Blinks |
| --- | --- | --- |
| Play, Play or pause | the transport plays | |
| Pause | the transport is paused | |
| Stop | the transport is stopped | |
| GO | a prepared setlist waits for it (to start, resume or ask for a devamp) | while a devamp is requested |
| Devamp | the show is inside a loop | while a devamp is requested |
| Cancel devamp | a devamp is requested | |
| Next / previous cue, Go to cue `n`, Move by | there is somewhere to go (the output is open) | |
| Next / previous timeline, Go to timeline `n` | that timeline exists and the show is not playing | |
| Edit view, Live view | never written: the view belongs to the window | |
| MIDI panic | never written: it is a one-shot, not a state | |

An action with no output open is out. A **program change** has no "off", so a pad learned as one is never lit; use a
note or a control change.

**What is sent.** A pad learned as a note gets a *Note On* on its channel with the lit value as its velocity, and the
unlit value (usually 0) when it goes out; a control change gets the value as its data. Blinking is done by Flux, which
alternates the two values every half second, so it works on any controller whatever its own blink codes are.

**Values.** Each binding has a *Lit value* and an *Unlit value* (0 to 127) in the page, which fall back to the preset of
the controller when the binding has none:

| Controller (by port name) | Lit | Unlit | Meaning |
| --- | --- | --- | --- |
| AKAI APC (`APC40 mkII`, `APC mini`, …) | 1 | 0 | green, off — the colour table of the APC manuals (3 red, 5 yellow) |
| Any other | 127 | 0 | full on, off — set the colour or brightness your controller expects |

The APC values come from the manufacturers' published tables and **have not been checked here with a real controller
yet**: verify them on yours and correct them per binding. Add the values you confirm for other controllers to this
table.

**How it is derived.** The pads are decided in `flux-engine/src/control_light.rs` from the same facts the Live view draws
(the transport position, the cues and the setlist's tick, read by `transport_commands::light_snapshot`), so a controller
cannot disagree with the screen. The `flux-control-lights` thread looks at that state every 50 ms and writes only what
changed, nothing is sent periodically. It only reads the output's published state: it never blocks or delays the clock,
and it is not in the audio path. A write that fails is counted, logged from that thread (at most once every two seconds) and the port is reopened.

## Not yet

OSC replies for the legacy `/flux/...` addresses (the versioned API in [osc-api.md](osc-api.md) has state feedback).


## Setlists and OSC v1

GO, Devamp and Cancel devamp are available as MIDI-learn actions and the simple addresses
`/flux/go`, `/flux/devamp`, `/flux/devamp/cancel`. They operate the prepared setlist rather
than jumping the ordinary timeline playhead.

The same preferences now enable an OSC request/reply and feedback server over UDP and optional
TCP, with an installation-local listen address. The versioned dictionary, identities, framing,
request IDs and subscriptions are documented in [osc-api.md](osc-api.md). That API supports
show operation and state feedback; legacy addresses above retain their original meaning.
The earlier limitation on OSC state replies no longer applies to the versioned API. MIDI
controller light feedback is described in [Light feedback](#light-feedback).
