# Flux Windows console behavior

## Goal

Run the normal Flux desktop application without creating a terminal window on Windows, while retaining terminal output for headless invocations such as `flux --no-ui`.

## Design

Windows release builds use the graphical subsystem. Launching `flux.exe` from Explorer therefore creates only the Tauri application and tray icon.

At process startup, Flux attempts to attach to its parent console. The operation is harmless when launched from Explorer because no parent console exists. When invoked from PowerShell or another terminal, the same executable attaches before parsing arguments or initializing logging, so CLI output, including `--no-ui`, `--list-devices`, help, and errors, remains available to that terminal.

Linux and debug builds keep their current behavior. No new console is allocated, and normal GUI launches do not gain a transient console window.

## Error handling

Failure to attach a parent console is expected for graphical launches and is ignored. Flux still starts its UI or headless runtime normally. It does not allocate a new console.

## Verification

Build the Windows release executable, confirm its PE subsystem is GUI, and start it in `--no-ui --dry-run` mode from an existing console to verify it remains running and writes its startup status. Run formatting, Rust tests, and the frontend build.
