# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The UI is React rendered in a Tauri v2 webview on Windows, macOS and Linux. It is a desktop app, not a website, but its design language is web.

## Users

- **Live show technicians** (lighting, video, sound) who sync equipment to timecode during an event, often in a dark booth or on stage, under pressure, reading the screen from a distance.
- **Musicians running backing tracks** who need their tracks and the show's lighting or video cues locked together.
- **Show designers working in a studio** who build the show ahead of time, placing audio, cues and events on a timeline with time to think.

The same project moves between the studio and the booth, so one interface serves both a calm editing session and a tense live one.

## Product Purpose

Flux Timecode builds timecode tracks from songs, events and cues and plays them out as LTC (through an audio interface) and MTC (through a MIDI port). Success is a show that stays in sync from the first cue to the last, and a project that opens on another machine and just works.

## Positioning

QLab's cue and show-control model combined with a REAPER-style timeline and track interface in one app, where the timecode is generated from the same project that holds the audio and the cues. Neighbouring tools either run cues without a real timeline or give a timeline without show control.

## Operating Context

- A project is a `.fxtc` file: a zip with `project.json` and the bundled audio, so a show is self-contained and portable. Devices are machine-specific, so a project stores a name hint and the UI asks for a replacement when a device is missing.
- Flux is the master clock and only generates timecode; chasing an external timecode is out of scope for now.
- The audio device's sample clock drives playback, LTC and MTC. Positions everywhere are frame counts at the project frame rate (24, 25, 29.97 drop-frame, 30).
- Used anywhere from a quiet studio to a dark booth at a venue, on a laptop with an audio interface and a MIDI interface attached.

## Capabilities and Constraints

- Today the backend can: open, save and import audio into `.fxtc` projects; list audio devices and MIDI ports and resolve stored name hints; encode LTC and MTC; run a transport (play, pause, stop, seek) with a position stream to the UI.
- Not built yet: clip playback and mixing, trigger firing (`marker`, `midi`), waveform peaks, and any real UI. `src/App.tsx` is still the Tauri template demo.
- Internationalisation is a requirement from the start. English is the default and Spanish is the second language. No user-facing string is hard-coded.
- The UI must be legible in the dark and from a distance: the current timecode has to read at a glance.
- Undecided: LTC level and edge defaults beyond the shipped defaults, supported audio formats beyond wav, aiff, flac, mp3 and ogg, a non-drop 29.97 rate, trigger actions beyond `marker` and `midi`, and release and packaging.

## Brand Commitments

The product name is Flux Timecode. The look should be modern and dark-first, with a REAPER-like timeline and track interface. No logo or brand assets exist yet.

## Evidence on Hand

- Project documentation: `docs/fxtc-format.md`, `docs/timecode-signals.md`, `docs/implementation-plan.md` (on its own branch).
- No screenshots, customer quotes, benchmarks or sample shows exist. Future work must not invent any.

## Product Principles

1. **Trust the clock.** Anything that could make the show drift or lie about the time is worse than a missing feature. The displayed time and the emitted time come from the same source.
2. **The same project in the studio and in the booth.** Editing should feel deliberate; running a show should feel calm and unambiguous.
3. **Never fail because of hardware.** A missing device is a normal state the UI explains and lets the user fix; it never blocks opening a project.
4. **Every string is translatable.** Layout must survive longer text; English is the default, Spanish ships alongside.
5. **Keep the user's work portable.** What is in the project stays in the project.

## Accessibility & Inclusion

Legible in low light and at a distance: strong contrast for the timecode and transport state, and state never conveyed by colour alone. Windows, macOS and Linux are all first-class.
