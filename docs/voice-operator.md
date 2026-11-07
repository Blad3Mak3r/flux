# Voice operator

Create an operator track in Edit and add an order by double-clicking its lane or choosing Add voice order.
Write the dialogue in the inspector. Choose Start or End to align speech with its timeline anchor.
The inspector shows the effective voice and where it comes from: track, project or FLUX.
Project and track voice selectors can inherit from the next level.

In Preferences > Voice operator, supply your ElevenLabs API key and load the voices on your account.
FLUX stores the key in the system credential store (Windows Credential Manager, macOS Keychain, Linux
Secret Service), or in memory for the current session when explicitly selected. It never includes the key
in a project, preferences JSON or diagnostic export. Delete key removes the stored credential.
Keys need speech generation and voice-reading permissions. Generating and regenerating use the user's
ElevenLabs quota; FLUX neither supplies a key nor forwards requests through another server.

Generate one order at a time with the transport stopped. Generation is explicit: changing text or a voice
never incurs a request automatically. Listen auditions existing audio through the Edit-preview route,
falling back to the programme route, without starting the show's clock, MIDI, OSC or timecode.

- Not generated: the order holds dialogue but no audio.
- Synced: the saved audio matches the dialogue and effective voice.
- Outdated audio: saved audio exists, but the dialogue or effective voice has changed. The old audio plays
  until a successful regeneration. Expand Dialogue used for this audio to inspect what will actually be spoken.

Generating and generation errors appear separately from these content states. A failed request keeps the
previous audio. A successful response is decoded and saved as a durable local job before attachment.
If the order, dialogue, effective voice or open document changes, FLUX keeps that recording for explicit
recovery instead of attaching it automatically. Unrelated edits and anchor/alignment changes do not
invalidate a generation. If saving fails, Retry saving uses the response retained in memory and makes
no new provider request; keep FLUX open until it succeeds. Requests are never retried automatically
because retries may consume more credits.

Start alignment begins the audio at the anchor. End alignment finishes it there. If the complete phrase
would begin before the timeline origin, FLUX keeps the position and audio, displays a warning and silences
the order until it fits. Changing alignment preserves the anchor; regeneration preserves both and recomputes
the interval from the decoded sample duration.

Set the operator interface and mono/stereo channels in Preferences. All operator tracks share this route;
it never falls back to the programme when absent. Missing devices do not prevent opening the project.
Mute and track volume work independently of the programme. Overlapping orders mix together.
Live displays the orders and their content/position/output warnings without editing controls. Its dialogue
is the text in the recording that will actually play, even when a newer draft exists. Generation and
prelisten are available only while stopped; transport commands cancel pending or active prelisten.
Independent operator devices correct small clock drift by gradual resampling; seek resets immediately.

Generated audio is retained under the application's `generated-voice` data directory so recovery and undo
can still find it before a project is saved. Saving bundles it into `.flx` (or the existing legacy `.fxtc` path); other computers need no key or
network to play it. This first version retains generated source files rather than automatically deleting
files that recovery or undo may still need. Jobs include the exact dialogue and voice, never credentials.
Recordings not referenced by the open project appear under Retained recordings, including after restart
or undo; attaching one is an undoable edit and never changes the current dialogue or anchor.

## Check before the show

Open Check voice orders from Transport, Live or Preferences > Voice operator, with the transport stopped.
The backend checks every timeline, excluding muted operator tracks, for ungenerated orders, outdated
recordings, unreadable audio or incorrect sample duration, audio before the timeline origin, overlapping
orders, unavailable operator channels and channels shared with LTC. Issues link to the order or output
settings. Editing the project or preferences marks the report as out of date until checked again.
This check enumerates devices but opens no streams, sends no network requests and plays no sound;
a real sound check is still necessary.

## Verification

Automated tests use local HTTP fixtures and generated sample buffers, never live ElevenLabs credits.
A real provider/audio-interface check needs a user-supplied API key and appropriate hardware.

## Preparing recordings

Preferences > Voice operator includes Test operator output and Stop listening. The test is a bundled
English spoken sample generated locally with the Windows speech synthesizer, so it needs no API key or
network. It opens only the selected operator route, with no fallback, and emits no LTC, MTC or OSC.
Other recording auditions use the Edit preview route. Changing the transport or closing the relevant
panel cancels an audition, including a stream still being opened.

Retained recordings show their date, original voice name or identifier, decoded duration and origin.
Listen before choosing Use this recording. Imported recordings show their source filename. Recordings
remain local until attached; an attached recording is bundled in the project. Stop listening cancels a
pending audition as well as one already playing.

Preferences > Project > Keep inherited voices in this project pins the installation voice into the project
without touching explicit track choices or regenerating audio. A missing inherited voice rejects the change.
This operation enters undo history.

The order inspector's Pronunciation section holds optional text for ElevenLabs, for example a phonetic
spelling of an acronym. The visible dialogue stays readable. A generation records both texts and changing
either makes generated audio outdated. Empty pronunciation text uses the visible dialogue. Live displays
the readable dialogue preserved with the recording.

Import recording attaches a WAV, AIFF, FLAC, MP3 or Ogg file after validating and decoding it. Write the
dialogue first. The source file is copied to durable local storage before attachment, in its original format.
Imported recordings have an explicit origin and do not become outdated when inherited voices or pronunciation
text change; changing their visible dialogue does mark them outdated. The existing audio plays until replaced.

Trim recording removes whole milliseconds from the head or tail without changing the original source or
anchor. Playback, timeline extents, Live and audition all use that same trimmed sample interval. The complete
recording must retain at least one sample. Replacing or regenerating a recording resets trims, with undo
restoring the old recording and trims.

## Resource and close protection

The decoded audio cache retains at most 256 MiB using least-recently-used eviction. Clear decoded audio
cache releases cache ownership only: active playback keeps its own buffers, and source recordings, archives,
undo and recovery are preserved. This bounds retained cache memory, not total process memory or all buffers
required by the currently playing timeline. No source recording is automatically deleted.

New speech decoding is limited to ten minutes at the engine rate and a bounded native sample buffer.
Local recording imports accept at most 128 MiB of compressed/source bytes; ElevenLabs responses retain their
32 MiB download cap. Invalid or oversized files are rejected before replacing an order's existing recording.

Closing, replacing a project or restarting while a recording is being prepared keeps the document open
and explains why. An unsaved response is retried to disk without a second provider request before closing;
a disk failure keeps it in memory and prevents the transition. Backend reservation and document-transition
guards exclude each other, so a delayed generation cannot start after its document closes. Disk writes run
without holding the activity lock. The user must wait for an in-flight request rather than cancelling a
possibly billable operation.

Preflight now counts fully ready orders, distinguishes errors and warnings, and provides previous/next issue
navigation. It verifies source audio itself rather than trusting a saved waveform preview. Playback remains
available when a user intentionally accepts warnings such as overlapping or outdated recordings.

These fields use archive format 9. Format 8 recordings default to generated origin, no pronunciation override
and zero trims. Old local generation jobs also remain readable. Older app builds reject new version 9 files.
