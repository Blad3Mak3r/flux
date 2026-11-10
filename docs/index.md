---
title: FLUX Docs
---

# FLUX documentation

FLUX is a desktop app that turns songs, events and cues into one project with a master timecode clock: QLab's cue
model on a REAPER-style timeline. It generates LTC (through an audio interface) and MTC (through a MIDI port), fires
MIDI and OSC cues from the same clock, and has a Live view for running the show from the booth.

New here? The [landing page](https://blad3mak3r.github.io/flux/) shows what FLUX does, and the latest installers are on the
[Releases page](https://github.com/Blad3Mak3r/flux/releases/latest). FLUX is in early development, so these guides
describe what the app does today.

## Use FLUX

| Guide | Covers |
| --- | --- |
| [Setlists and show operation](/setlists) | Preparing and running a setlist, GO and transitions. |
| [Remote control](/remote-control) | MIDI controllers and OSC as a remote. |
| [Per-track audio outputs](/audio-routing) | Per-track destinations and per-computer interfaces. |
| [Voice operator](/voice-operator) | Spoken operator tracks. |
| [Logging and diagnostics](/diagnostics) | Logs and the diagnostics export. |

## Integrate

| Guide | Covers |
| --- | --- |
| [OSC operation API](/osc-api) | The `/flux/...` addresses, queries and feedback. |
| [Project file format](/fxtc-format) | The `.flx` / `.fxtc` archive: layout, fields and versioning. |
| [Timecode signals](/timecode-signals) | How LTC and MTC are encoded and sent. |

## Contribute

The source is at [github.com/Blad3Mak3r/flux](https://github.com/Blad3Mak3r/flux). Start with its `AGENTS.md`: it
covers the architecture rules, conventions and the git workflow.
