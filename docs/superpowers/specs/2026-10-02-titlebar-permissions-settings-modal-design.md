# Title-bar permissions and route-settings modal

## Goal

Repair Flux's custom native window controls, remove the obsolete left navigation rail, and move route configuration into a modal opened from the top-right title strip.

## Scope and constraints

- The main Tauri window remains undecorated, resizable, minimizable, and maximizable.
- The custom title strip remains the sole UI for native window controls.
- Closing the main window continues to use the existing Rust close interception, which hides Flux to the system tray and leaves DMX routing active. This work does not add a quit action or alter Rust tray handling.
- Existing runtime snapshots, settings commands, reconnect action, and DMX monitor command remain unchanged.

## Window controls and dragging

The default Tauri capability explicitly grants the main window access to `core:window:allow-minimize`, `core:window:allow-toggle-maximize`, and `core:window:allow-close`. The existing React handlers keep calling the current-window API and continue reporting failures through application feedback.

The top strip keeps Flux identity and bridge status on the left. A dedicated empty region in the middle uses Tauri's drag-region attribute so the user can move the window freely. Buttons, including the configuration control and native controls, remain outside that region so they accept clicks and keyboard focus normally.

The right action group is ordered: `Configure route`, minimize, maximize/restore, and close to tray. Native control hit areas, tooltips, accessible names, focus styling, and close hover treatment remain intact.

## Layout

The left navigation rail is removed entirely. The application shell becomes one workspace column beneath the title strip. The route map and output-device inspector remain visible, and the inspector's existing `Open DMX monitor` action preserves access to the monitor after navigation removal.

The configuration section no longer occupies space in the workspace. Its title-strip gear button is the sole entry point to configuration.

## Route-settings modal

Selecting `Configure route` opens a React-owned dialog above the workspace. It contains the existing listen address, universe, DMX channel count, refresh rate, FTDI device selector, device refresh action, device status hint, Reset, Cancel, and Apply changes controls.

Opening the dialog seeds a new draft from saved settings. Closing through the close control, Escape, backdrop click, or Cancel discards the draft. Reset restores saved values while leaving the dialog open. Apply stays disabled until the draft is valid and differs from saved settings. A successful save updates the saved settings, synchronizes the draft, and closes the dialog; an unsuccessful save leaves it open and shows the existing error feedback.

The dialog is labelled and modal (`role=\"dialog\"`, `aria-modal=\"true\"`). Focus moves into it on open, cycles within it while open, and returns to the title-strip configuration button on close. The dialog may not be dismissed while a save is in progress.

## Verification

1. Build the frontend and the Tauri application.
2. Verify minimize, maximize/restore, and close-to-tray work without permission errors; reopen Flux from the tray and confirm routing continues.
3. Drag the empty title-strip region and confirm buttons and form controls are not treated as drag areas.
4. Confirm the sidebar is absent and the DMX monitor still opens from the workspace.
5. Verify the configuration dialog's field validation, device refresh, reset, apply, cancel, close button, Escape, backdrop dismissal, focus handling, and draft discard behavior.
