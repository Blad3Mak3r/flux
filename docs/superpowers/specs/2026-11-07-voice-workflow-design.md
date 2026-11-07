# Voice workflow and production controls

Approved direction: extend the existing operator workflow using the current Bench Instrument controls.

- A stopped-only local output test plays a bundled spoken sample directly on the operator route, never the show clock or provider. Retained recordings can be auditioned on the Edit preview route and identified by local date, voice and backend-formatted duration.
- Voice work exposes backend generation/saving/unsaved activity. Closing or replacing a document waits for an in-flight paid request and attempts to persist any unsaved response; failure leaves the document open with recovery instructions. The backend independently refuses document transitions while a request or unsaved response exists.
- Bound decoded cache retention with LRU eviction; active playback owns its buffers independently. Limit voice decode duration and compressed input before allocation growth. Retained source recordings are never automatically deleted, since undo and crash recovery can still reference them. Expose cache size and clearing of decoded buffers separately from source recordings.
- The existing preflight report remains read-only and offers previous/next issue navigation alongside direct links. Counts distinguish errors and warnings.
- An undoable backend action pins the installation voice to the project, keeping explicit track choices. Missing effective voices reject this operation atomically.
- Each order may have an optional synthesis text for pronunciations. Display dialogue remains readable; each recording preserves both texts. Editing either makes generated speech stale. Imported human recordings compare the display dialogue only and are independent of inherited voices.
- Non-destructive head/tail trims are stored as engine samples, entered in whole milliseconds. They preserve the chosen anchor and never alter the source asset. Both alignments, preview, Live and playback use the trimmed interval. Replacing a recording resets trims explicitly and the UI explains that behavior.
- Import a local supported audio file into a selected order. Decode before attachment, stage a durable job first, keep the original source and format, and preserve dialogue/anchor/alignment. An imported recording has an explicit origin rather than pretending to be an ElevenLabs generation.
- Optional order/recording fields and imported origin require format 9; versions 8 and earlier default to generated origin, no pronunciation override and zero trims. Preserve .flx/.fxtc compatibility.

Validation: unit tests for cache eviction and lifetime, decoder bounds, transition protection, metadata/recovery, inherited voice pinning, trim timing/mixing and model migrations; mock provider tests only. Run frontend checks and Rust format/Clippy/workspace tests. Review UI in English and Spanish. Real speech generation and device sound checks require user credentials/hardware and are not automated.
