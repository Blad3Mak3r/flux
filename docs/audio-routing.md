# Per-track audio outputs

Audio and operator tracks select one destination in the inspector. Inherit uses Programme for audio and Operator for speech; Silent intentionally disconnects the track. Programme and Operator are permanent destinations. Named destinations have stable UUIDs, unique names, and are shared by all timelines. Voice selection, speech generation state and recorded assets are independent of routing.

## Project and installation

Format 10 stores `output.audio` definitions and optional per-track `audioOutput` assignments. Older projects and recoveries use inherited destinations. Interfaces and channels live only in local preferences, keyed by destination UUID. Removing a destination retargets all its tracks to Silent atomically; applying the returned project enters one undo step. Local mappings remain available for undo and other copies of that project.

Preferences > Audio chooses the driver, Programme clock interface and Edit preview. Preferences > Audio outputs manages named destinations, their local interface/channel assignments, local spoken tests and the built-in Programme/Operator assignments. Changing the global driver clears the local audio mappings, as with existing routes. Supported controls select mono on any channel or standard contiguous stereo pairs. The engine remains at 48 kHz.

With Edit preview configured, assigned tracks audition there, even if their Live destinations are unmapped; Silent remains silent. With no preview, Edit keeps its existing Live routing behavior. The metronome and suppression of timecode/cue outputs during audition keep their established behavior. Normal voice Listen uses Edit preview; Test this output uses that destination and a bundled English phrase, without a network request or timecode.

## Prepared routing and real-time mixing

`flux-project::routing` owns portable assignments, validation, choices and edits. Installation preferences resolve physical routes for the active timeline. `flux-engine::audio_routing` prepares track membership and zero-based channels before starting any callback.

The existing Programme interface remains the master clock even when all audible tracks are routed elsewhere. The output planner groups named routes, LTC and the metronome by canonical device, including aliases, and opens one playback stream per physical interface. Each callback mixes the selected track IDs into prepared stereo lanes, sums contributions on each hardware channel and clamps once. Mono receives the average of the stereo pair. Track mute, gains, envelopes, fades, trims and peak meters remain in the shared mixer; decoded assets are shared.

Secondary interfaces follow the Flux sample clock with the existing bounded resampling cursor. Every lane on a secondary device uses the same cursor position and advances it once per chunk. This corrects gradual clock drift; it does not promise phase alignment between independent hardware clocks or add manual latency compensation.

Missing destinations never fall back to Programme. A bus that shares any LTC channel on the same canonical interface is entirely silenced, while LTC continues. An invalid audio channel silences the affected bus; the stream can still open for supported routes and LTC. Secondary device failures affect that interface alone and retry through the existing follower supervisor. Master loss retains the existing stop/recovery policy.

## Audio drivers

Preferences > Audio lists every driver the system offers. On Windows that is WASAPI, ASIO and JACK; on macOS CoreAudio and JACK; on Linux ALSA, PipeWire and JACK. ASIO is the one most professional interfaces ship, with the lowest latency and all their channels; their own ASIO driver (or Dante Virtual Soundcard, ASIO4ALL) appears as a device under it. WASAPI shared mode may show a multichannel interface as separate stereo pairs. Outputs open in 32-bit float, else 32, 24 or 16-bit integer, because many interfaces only offer 24-bit.

ASIO is built from the Steinberg ASIO SDK 2.3.4 kept in `third_party/asiosdk` (only the `common` and `host` folders cpal compiles, unchanged). Steinberg offers the SDK under its own licence or under the GPL version 3; FLUX uses the GPL option, which is why the repository is GPL-3.0. `.cargo/config.toml` points `CPAL_ASIO_DIR` at it, so the build downloads nothing. Building on Windows needs MSVC and libclang with the MSVC environment loaded (`LIBCLANG_PATH`; the CI action `.github/actions/windows-deps` does both). ASIO is a trademark of Steinberg Media Technologies GmbH; the Steinberg ASIO Usage Guidelines apply wherever FLUX shows the name or logo.

Not tested on hardware: whether two streams on the same ASIO driver both open (many drivers serve one client per process).

## Pending changes and reporting

Routing changes during Play retain the active prepared membership. Ordinary clip edits still reach those active memberships; new track IDs remain silent until the pending routes are applied. The backend prepares the latest pending programme and applies it once Pause/Stop has been consumed by the callback. Play and hardware transitions are serialized, including remote transport commands, so Play waits for pending routing. Reopening restores the exact sample position and paused/stopped state without an intermediate Play. Undoing back to active routing cancels pending changes.

The `onAudioRouting` IPC lane reports active route health, active and desired track destinations and pending status independently of position/meters. Destination health uses stable IDs and atomic device-loss flags, with no 32-output bitmask limit. Statuses are Ready, Missing, LTC Conflict, Lost and Unused. Live renders this backend state read-only. Operator preflight checks each track's effective destination; intentional Silent is a warning.

## Validation

Automated coverage includes inheritance, explicit and Silent assignments, all-timeline removal, atomic refusal of invalid edits, archive/recovery round trips, migration from format 9, undo/redo, preview resolution, local preference persistence, mixing before clamping, mono separation, faders/meters, active membership across clip edits, LTC collisions, device aliases, route loss beyond 32 outputs, per-track preflight and exact sample restoration at every supported frame rate.

Visual review uses browser fixtures for Edit, Audio outputs and Live in English and Spanish, including empty track selection, pending routing and removal confirmation. Those fixtures are removed before delivery. No paid voice generation is required.

Hardware acceptance still requires real multi-interface playback, disconnection/reconnection and driver-specific exclusive-device checks on each supported OS. Automated signal tests and browser fixtures do not certify real interfaces, native dialogs or installer behavior.

On Windows, the app build explicitly requests the Common Controls v6 dependency for unit-test executables as well as the application: the dialog plugin imports TaskDialogIndirect, whose [Microsoft API requirements](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/nf-commctrl-taskdialogindirect) specify Comctl32 v6. This prevents the test executable failing to load before the tests start.

Validation on Windows: `npm run typecheck`, `npm test` (168 passed), `npm run build`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --quiet` (684 passed; one intentionally ignored subprocess helper exercised by its parent recovery test). No real ElevenLabs generation, hardware smoke test or installer/multiplatform validation was run.
