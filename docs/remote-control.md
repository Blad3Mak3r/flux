# Remote control

Flux can be run from a MIDI controller (an AKAI APC, any pad or button surface) and from OSC (a Stream Deck, QLab, a
console). Flux stays the **master clock**: remote control starts, stops and moves it, it never follows another timecode.

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
| Edit view, Live view | Switches the view, as Ctrl+1 / Ctrl+2. Refused while the show plays. |

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

Numbers can be sent as an integer, a float (rounded) or a string. Actions without a parameter ignore any argument, because
a button usually sends a `1`. Bundles are accepted and their time tags ignored. In a Stream Deck, use an OSC / UDP plugin
pointed at this computer's address and the port, with one of the addresses above on each button.

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
