# Provider errors and error dialogs

Approved direction: distinguish the reason for ElevenLabs failures and display operation errors in one consistent modal instead of scattered panel messages.

## Provider boundary

The Rust speech client reads a bounded error response and recognizes provider codes for insufficient credits, subscription restrictions, temporary request limits, unusual activity, credentials and permissions. Unknown or malformed responses retain an HTTP status fallback. Raw response messages, API keys and dialogue are never exposed. No automatic generation retry spends additional credits.

## Presentation

The existing shared `sayError` entry point queues distinct pending errors. A native HTML alert dialog uses the existing instrument styling, a translated heading and an acknowledgement button. Escape dismisses the current error. Pending errors are shown one at a time, and duplicate pending messages are coalesced. The modal preserves the current task and blocks transport shortcuts. Voice operations use this entry point instead of keeping separate panel error strings. Persistent project warnings and preflight findings remain with the affected settings or orders.

## Validation

Local HTTP fixtures exercise provider codes, malformed bodies and status fallbacks without live credentials. Frontend checks cover compilation and existing tests; browser verification covers dismissal, focus, keyboard blocking, translated text, pending errors and appearance above Preferences.
