# Voice operator

Create an operator track in Edit and add an order by double-clicking its lane or choosing Add voice order.
Write the dialogue in the inspector. Choose Start or End to align speech with its timeline anchor.
The inspector shows the effective voice and where it comes from: track, project or Flux.
Project and track voice selectors can inherit from the next level.

In Preferences > Voice operator, supply your ElevenLabs API key and load the voices on your account.
Flux stores the key in the system credential store (Windows Credential Manager, macOS Keychain, Linux
Secret Service), or in memory for the current session when explicitly selected. It never includes the key
in a project, preferences JSON or diagnostic export. Delete key removes the stored credential.
Keys need speech generation and voice-reading permissions. Generating and regenerating use the user's
ElevenLabs quota; Flux neither supplies a key nor forwards requests through another server.

Generate one order at a time with the transport stopped. Generation is explicit: changing text or a voice
never incurs a request automatically. Listen auditions existing audio through the Edit-preview route,
falling back to the programme route, without starting the show's clock, MIDI, OSC or timecode.

- Not generated: the order holds dialogue but no audio.
- Synced: the saved audio matches the dialogue and effective voice.
- Outdated audio: saved audio exists, but the dialogue or effective voice has changed. The old audio plays
  until a successful regeneration. Expand Dialogue used for this audio to inspect what will actually be spoken.

Generating and generation errors appear separately from these content states. A failed request keeps the
previous audio. A result is discarded when its order, dialogue, effective voice or open document no longer
matches. Requests are not retried automatically because retries may consume more credits.

Start alignment begins the audio at the anchor. End alignment finishes it there. If the complete phrase
would begin before the timeline origin, Flux keeps the position and audio, displays a warning and silences
the order until it fits. Changing alignment preserves the anchor; regeneration preserves both and recomputes
the interval from the decoded sample duration.

Set the operator interface and mono/stereo channels in Preferences. All operator tracks share this route;
it never falls back to the programme when absent. Missing devices do not prevent opening the project.
Mute and track volume work independently of the programme. Overlapping orders mix together.
Live displays the orders and their content/position/output warnings without editing controls.

Generated audio is retained under the application's `generated-voice` data directory so recovery and undo
can still find it before a project is saved. Saving bundles it into `.fxtc`; other computers need no key or
network to play it. This first version retains generated source files rather than automatically deleting
files that recovery or undo may still need.

## Verification

Automated tests use local HTTP fixtures and generated sample buffers, never live ElevenLabs credits.
A real provider/audio-interface check needs a user-supplied API key and appropriate hardware.
