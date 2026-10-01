# Flux DMX monitor window

## Goal

Move detailed DMX channel inspection out of the operational dashboard into a focused auxiliary window, so operators can monitor levels without obscuring route health or configuration.

## Window lifecycle

The Rust UI layer owns one Tauri webview window labelled `dmx-monitor`. The `Open DMX monitor` command creates it on first use and otherwise shows and focuses the existing window. Its close button hides the window, matching the application tray behaviour; it never stops the Flux runtime or creates duplicate monitor windows.

The monitor has a compact, resizable native window with sensible minimum dimensions. It is not shown at application startup and does not appear as an additional application entry point.

## Frontend layout and data

The monitor has its own static HTML entry point and reuses the React runtime snapshot command. It polls the same snapshot at the existing 500 ms cadence. It initially shows active DMX channels, gives their count, and has an explicit toggle for all 512 channels.

The primary dashboard replaces its embedded monitor with an `Open DMX monitor` action. Its route summary, configuration form, and feedback behaviour stay unchanged. If opening the auxiliary window fails, the dashboard shows an actionable error message without changing the route status.

## Packaging

Vite builds both the primary document and the monitor document. Tauri loads the monitor through its bundled application URL in production and through the development server in development, so neither mode depends on an extra localhost service beyond the existing Vite dev server.

## Verification

Run formatting, TypeScript/Vite production build, and Rust checks. In a desktop run, open the monitor repeatedly and confirm it reuses one window; close it and confirm reopening shows it again; confirm active/all channel mode updates as DMX frames arrive; and confirm closing the monitor leaves the main panel and Art-Net/DMX runtime running.
