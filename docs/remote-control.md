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
- **Logging.** The MIDI callback and the socket thread only decode and send; they never log. `flux-control` logs the inputs
  coming and going (`info`, `warn`) and each action (`debug`).

## Not yet

Feedback to the controller (lighting the APC's pads) and OSC replies with the state of the show.


## Setlists and OSC v1

GO, Devamp and Cancel devamp are available as MIDI-learn actions and the simple addresses
`/flux/go`, `/flux/devamp`, `/flux/devamp/cancel`. They operate the prepared setlist rather
than jumping the ordinary timeline playhead.

The same preferences now enable an OSC request/reply and feedback server over UDP and optional
TCP, with an installation-local listen address. The versioned dictionary, identities, framing,
request IDs and subscriptions are documented in [osc-api.md](osc-api.md). That API supports
show operation and state feedback; legacy addresses above retain their original meaning.
The earlier limitation on OSC state replies no longer applies to the versioned API. MIDI
controller light feedback remains unimplemented.
