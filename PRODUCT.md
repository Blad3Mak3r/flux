# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The UI is React rendered in a Tauri v2 webview on Windows, macOS and Linux. It is a desktop app, not a website, but its design language is web.

## Users

- **Live show technicians** (lighting, video, sound) who sync equipment to timecode during an event, often in a dark booth or on stage, under pressure, reading the screen from a distance.
- **Musicians running backing tracks** who need their tracks and the show's lighting or video cues locked together.
- **Show designers working in a studio** who build the show ahead of time, placing audio and notes on a timeline with time to think.

The same project moves between the studio and the booth, so one interface serves both a calm editing session and a tense live one.

## Product Purpose

FLUX builds timecode tracks from songs, events and cues and plays them out as LTC (through an audio interface) and MTC (through a MIDI port). Success is a show that stays in sync from the first cue to the last, and a project that opens on another machine and just works.

## Positioning

QLab's cue and show-control model combined with a REAPER-style timeline and track interface in one app, where the timecode is generated from the same project that holds the audio and the cues. Neighbouring tools either run cues without a real timeline or give a timeline without show control.

## Operating Context

- A project is a `.flx` file: a zip with `project.json` and the bundled audio, so a show is self-contained and portable. Legacy `.fxtc` files use the same format and remain supported. New projects and Save as default to `.flx`; Save keeps the existing path and extension. Hardware routes are stored per computer and shared by projects; a project carries logical signals and levels. Legacy device hints are imported only after explicit review.
- Flux is the master clock and only generates timecode; chasing an external timecode is out of scope for now.
- The audio device's sample clock drives playback, LTC and MTC. Positions everywhere are frame counts at the project frame rate (24, 25, 29.97 drop-frame, 30).
- Used anywhere from a quiet studio to a dark booth at a venue, on a laptop with an audio interface and a MIDI interface attached.

## Capabilities and Constraints

- Today the app can: open, save and import audio into `.flx` projects (with legacy `.fxtc` support) with several timelines; edit audio and notes tracks on a timeline and play the audio mixed on the audio device's clock; detect the tempo of songs and click a metronome; generate LTC and MTC; run a transport (play, pause, stop, seek); and run the show in a read-only Live view driven by cues the backend derives from the project.
- Outputs are set up in Preferences. The project owns logical LTC/MTC outputs and signal levels; the computer owns the audio driver, programme and Edit-preview interfaces, metronome route, LTC device/channel/latency and MTC port/index/latency. Removing a logical output preserves every other output’s slot and local routing. Missing routes are visible and never substituted for timecode.
- Cue tracks fire from the same clock: MIDI tracks (notes, control and program change, MIDI Show Control, SysEx) to a chosen MIDI port, and OSC tracks (UDP) to destinations defined in Preferences. Seeking never replays the events jumped over.
- Remote control: a MIDI controller (pads and buttons, learned by touching them) and OSC over UDP (fixed `/flux/...` addresses, e.g. from a Stream Deck) can play, pause, stop, move between cues and timelines, step the position and switch the view. It is set up in Preferences and belongs to the machine, not to the project. Flux stays the clock: this moves it, it never follows another.
- Spoken operator tracks hold dialogue and either generated ElevenLabs audio or imported/microphone-recorded audio per order, with per-order start/end alignment and an inherited Operator audio destination. Audio and operator tracks can choose one portable named destination or Silent; each computer maps destinations to interfaces and mono/stereo channels. Edit preview overrides assigned destinations for audition. Live uses per-track routing; changes during playback apply on pause or stop. Voice selection inherits from track to project to installation. Users supply their own API key; archived speech plays offline.
- Setlists reference timelines without copying their content. A prepared show supports repeated/omitted entries, manual or automatic transitions, timeline waits and loops with devamp, and a backend-owned GO action in Live. Audio, cues and LTC/MTC repeat together. Preparation retains included audio and requires valid routes; structural edits wait for Stop. OSC v1 adds UDP/TCP operation, queries, bounded request deduplication and subscribed state feedback while preserving legacy addresses. See `docs/setlists.md` and `docs/osc-api.md`.
- The cue list can be exported as a cue sheet (File → Export): CSV or a self-contained, print-friendly HTML page, for the active timeline or all of them. It is built by the backend from the same cues as the Live view, keeps their numbers, leaves out cues on muted tracks and is written in the app's language. Text that starts like a spreadsheet formula is saved as text.
- A MIDI controller's pads can be lit with the state of the show (transport, GO and devamp, available cues and timelines), derived in the backend from the state the Live view draws. It is off by default. See `docs/remote-control.md`.
- Not built yet: firing video or Art-Net cues and chasing an external timecode.
- Internationalisation is a requirement from the start. English is the default and Spanish is the second language. The language is saved per computer and shared across windows; the selector shows a flag beside each language name. No user-facing string is hard-coded.
- The UI must be legible in the dark and from a distance: the current timecode has to read at a glance.
- Undecided: the LTC edge default beyond the shipped one (the level is set per output), supported audio formats beyond wav, aiff, flac, mp3 and ogg, a non-drop 29.97 rate, which further track kinds come after audio and notes, and release and packaging.

## Brand Commitments

The product name is **FLUX**, always uppercase in user-facing copy. FLUX prepares and runs shows with audio, MIDI/OSC cues and a shared master timecode clock. Timecode is a core capability rather than part of the name. The look stays modern and dark-first, with a REAPER-like timeline and track interface. The modular F symbol lives in `branding/` and `src/assets/`; the app pairs it with the FLUX wordmark.

The rebrand preserves the project archive format and legacy `.fxtc` support, `com.fluxtimecode.app` application identifier, package and crate names, settings paths and `/flux/...` OSC addresses so existing projects, routing and preferences remain compatible. The app registers both `.flx` and `.fxtc` file associations.

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
