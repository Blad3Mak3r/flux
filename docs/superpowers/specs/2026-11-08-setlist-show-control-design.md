# Setlist show control

Approved direction: keep the FLUX timeline editor and master clock, and add portable setlists
that reference timelines. Entries can repeat or omit a timeline and have names, operator notes,
and manual, automatic or delayed transitions. GO also resumes timeline waits or requests devamp.
Looping repeats audio, cues and LTC/MTC together; devamp finishes the current turn.

Implementation lives in `flux-project` for model/edits, `flux-engine` for sample-clock execution,
and the app crate for preparation, IPC and OSC. React renders backend state. Automatic transitions
split buffers without added silence or overlapping timelines. Each entry retains its frame rate
and offset; independent audio interfaces retain their existing follower-clock limitations.

Preparation retains all included audio rather than decoding the next song during playback.
Repeated timeline references share prepared clips. Structural edits are locked until Stop;
leaving a prepared setlist returns to Edit and ordinary transport. Existing projects migrate to
format 12 with empty setlists and controls and retain their previous playback behavior.

OSC v1 provides UDP/TCP show operation, stable identities, request/reply, bounded deduplication
and subscribed state feedback. Existing addresses remain supported. Editing remains in Edit.
See `docs/setlists.md` and `docs/osc-api.md` for behavior and wire contracts.

Validation requires archive/recovery tests, exact-sample renderer tests, cue cancellation tests,
OSC framing/deduplication tests, standard frontend/Cargo checks and a separate native/hardware
acceptance pass. No automated test alone certifies real LTC/MTC/MIDI receivers or interface clocks.
