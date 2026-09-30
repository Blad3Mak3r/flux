# Flux Tauri CI completion

## Goal

Finish the existing `feat/tray-ui` pull request without expanding its product scope. The tray and runtime panel remain part of the change; the branch must compile and pass continuous integration.

## Scope

Validate the full build chain in this order:

1. Rust formatting and compile checks.
2. Vite/TypeScript frontend build.
3. Tauri packaging/build on the CI-supported platforms.
4. Existing headless CLI paths, especially `--list-devices` and `--no-ui`.

Correct only defects that block those checks: incompatible crate or Tauri APIs, invalid build configuration, missing frontend/CI setup, type mismatches, lockfile drift, and platform-specific build failures.

## Architecture and data flow

No new architecture is introduced. `Flux Core` continues to run beneath the Tauri shell, updating the typed runtime snapshot. The Tauri backend exposes that snapshot and the reconnect/quit actions to the Vite panel. The tray opens the hidden panel and preserves the existing headless entry points.

## Error handling

Build failures are fixed at their source, one reproducible error at a time. If a platform cannot support a required Tauri capability, CI should report a clear, actionable failure rather than silently disabling the panel. Runtime failures must not prevent `--list-devices` or `--no-ui` from behaving as before.

## Verification

Run formatter and Rust checks, the frontend build, and every practical Tauri build/test command locally. Compare the resulting workflow configuration with the repository's Windows and Linux CI jobs. The PR is complete only when its CI is green and the scoped commands retain their expected behavior.
