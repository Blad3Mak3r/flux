---
title: FLUX Docs
---

# FLUX documentation

FLUX is a desktop app for running cues and mixing visuals at live shows. Choose **File > New cue show** or
**File > New VJ show** when you create the project. Each project opens in its own mode.

| Mode | Workflows |
| --- | --- |
| **CUE** | Lay out audio, video, notes and MIDI or OSC cues in the [timeline](/timeline-editing). Use the [Live view and setlists](/setlists) to run the show with GO and send LTC and MTC from the master clock. |
| **VJ** | [Launch and mix visuals live](/vj-mode) with a clip grid, layers, decks, effects and a crossfader. |

The [landing page](https://blad3mak3r.github.io/flux/) shows both modes. Download installers from the
[latest release](https://github.com/Blad3Mak3r/flux/releases/latest).

## Use FLUX

| Guide | Covers |
| --- | --- |
| [Video and projection mapping](/video-output) | Video clips, output screens, slices, soft edges and masks. |
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
