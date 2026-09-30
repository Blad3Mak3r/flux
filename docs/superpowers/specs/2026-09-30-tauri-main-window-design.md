# Flux Tauri main-window setup

## Goal

Prevent Flux from panicking at startup because the Tauri webview labeled `main` is created twice.

## Design

`tauri.conf.json` remains the single source of truth for the `main` window's label and visual properties. Tauri creates that configured window before the application setup hook runs. The setup hook obtains the existing `main` window through the app handle and passes it to the tray installer.

The runtime window builder is removed. No window labels, UI behavior, Art-Net behavior, or DMX reconnect behavior changes.

The positioner plugin is initialized before the tray icon is installed, so its tray-event handler has the state it requires.

## Error handling

If the configured `main` window is absent, setup returns a clear Tauri error instead of silently continuing without a tray. The existing hardware warning for an unavailable FTDI device remains non-fatal and continues to retry.

## Verification

Run formatting and Rust checks, build the Vite frontend, and build the Tauri application. Confirm the setup path no longer calls `WebviewWindowBuilder` with the `main` label.
