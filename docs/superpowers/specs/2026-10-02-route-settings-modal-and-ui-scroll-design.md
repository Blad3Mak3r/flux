# Route settings modal and UI scroll design

## Goal

Simplify the Flux desktop surface by removing the left navigation rail, placing route configuration and update availability in the top-right title strip, and presenting configuration and update flows in accessible React modals. Keep Flux's custom window controls functional. Replace operating-system window scrolling with controlled, styled scrolling inside the application UI.

## Scope and constraints

- The main Tauri window remains undecorated, resizable, minimizable, and maximizable.
- React continues to own the custom title strip. The existing Tauri current-window calls remain the implementation for minimize, maximize/restore, and close.
- Close keeps the established Rust close interception: Flux hides to the system tray and continues routing DMX. This work does not add a quit control or change Rust handlers.
- The route configuration UI uses only React state and existing settings commands. No Tauri command or plugin is introduced for modal behavior.
- The main window itself does not scroll. Any required scrolling occurs in explicitly owned UI regions.
- Tauri's signed updater is configured before update checking is enabled. Its public key and update endpoint are app configuration; its corresponding private signing key is only a GitHub Actions secret and is never committed.

## Layout

The title strip keeps the Flux identity and text-labelled bridge condition on the left, followed by a drag region. Its right-side action group is ordered as follows:

1. A compact gear button labelled `Configure route`, with a visible tooltip and accessible name.
2. A conditional update button, labelled with the available version, when the startup check finds a newer signed release.
3. Minimize.
4. Maximize or restore.
5. Close to tray.

Window controls retain their fixed hit areas, hover treatment, keyboard focus indication, and error feedback. The gear button is outside the drag region, as are all other interactive controls.

Remove the complete left navigation rail and collapse the shell to a single workspace column. The route workspace and device inspector keep their current operational roles. The inspector continues to provide access to the DMX monitor, so no route or monitor capability is removed with the rail.

## Route settings modal

Selecting the gear opens a React-controlled dialog over the workspace. The dialog contains the existing route fields, device refresh, explanatory device state, Reset, Cancel, and Apply changes actions.

Opening the modal seeds a fresh draft from the last saved settings. Closing it through the close button, Escape, backdrop click, or Cancel discards the draft and restores it from the saved settings. Reset also restores the saved settings while leaving the modal open. Apply changes remains disabled until the draft is both valid and different from saved settings. After a successful save, Flux updates saved settings, synchronizes the draft, and closes the modal. A failed save leaves the dialog open and surfaces the existing error feedback.

The dialog uses `role="dialog"`, `aria-modal="true"`, and a labelled heading. Focus moves to the dialog on open, is contained while it is open, and returns to the gear button on close. The Escape handler and backdrop interaction must not close the dialog while an apply operation is in progress.

## Application updates

After the React application initializes, it performs one Tauri updater check in the background. A current or failed check does not add a title-strip control. When a newer signed release is available, Flux stores its version and renders the conditional update button beside the configuration gear. The update check does not block route initialization or the 500 ms runtime snapshot loop.

Selecting the update button opens a React-controlled update dialog that identifies the available version and presents `Update now` and `Cancel`. Before update start, the dialog may be dismissed through Cancel, its close control, Escape, or backdrop click. Selecting `Update now` begins download and installation through Tauri's updater API. From this point the dialog cannot be dismissed by close, Escape, or backdrop. The Cancel button remains visible but disabled, rather than being removed, while installation is active.

The dialog reports determinate progress when the updater exposes total content length; otherwise it communicates an active indeterminate download state. When the updater completes successfully, Flux requests the updater's normal relaunch operation. If checking, downloading, or installing fails, the dialog shows an actionable error and presents `Retry` and an enabled `Cancel`. Retry repeats the relevant updater flow; Cancel closes the dialog and leaves the current Flux session and its routing operation running.

Release publication must create signed Tauri updater artifacts and a signed update manifest at the configured endpoint. The implementation must retain the repository's existing protection against embedding or logging signing secrets.

## Application-owned scrolling

The document root, application frame, and outer shell are constrained to the viewport and use hidden overflow. The workspace becomes the primary vertical scroll container below the title strip. It receives a narrow, dark scrollbar with a clearly visible thumb, hover treatment, and nonzero contrast against its track. Standard CSS scrollbar declarations cover Chromium/WebKit and Firefox.

The settings dialog is constrained to the available viewport height and has a separate, identically styled internal scroll region for the form when needed. Opening the modal prevents the workspace beneath it from scrolling. The modal never forces a native Windows window scrollbar. The title strip stays fixed and the three window controls remain reachable at every workspace scroll position.

## Error handling and accessibility

All state conditions continue to use words alongside colour. The gear, dialog close control, Cancel, and window operations have accessible names. Visible focus indicators remain present for the new button, modal controls, and scrollbar-owning regions where keyboard focus applies. Form validation and device detection messaging reuse the existing feedback patterns.

## Files and implementation boundaries

- `ui/src/main.tsx`: remove the navigation rail; add the gear button, reusable settings dialog behavior, focus and dismissal handling, discard-on-close draft reset, and the startup updater check/update dialog state machine.
- `ui/src/style.css`: make the workspace and dialogs the owned scroll containers; add scrollbar styling; style title-strip actions, configuration modal, update modal, and progress/error states.
- `tauri.conf.json`: retain existing window capabilities and add the signed updater endpoint and public key configuration.
- `Cargo.toml`, Rust setup, and Tauri capability configuration: add and initialize the official updater plugin with only the permissions needed for checking, downloading, installing, and relaunching.
- Release workflow: sign release artifacts and publish the updater manifest using a GitHub Actions signing secret.

No modification to Flux's DMX runtime, settings model, tray behavior, or command API is part of this work.

## Verification

1. Build the Vite frontend.
2. Confirm the left rail is absent and route/monitor access remains present.
3. Open the gear dialog and verify focus placement, focus containment, and return focus on close.
4. Edit values, then dismiss with close, Escape, backdrop, and Cancel; reopen each time and confirm values reset to the saved settings.
5. Verify Reset, validation, successful Apply-and-close, and failed-save behavior.
6. At normal and minimum window size, verify workspace scrolling is UI-owned and visually styled, no Windows window scrollbar appears, and modal scrolling does not move the background.
7. Verify minimize, maximize/restore, and close-to-tray; reopen from the tray and confirm routing remains active.
8. With a signed test update manifest, verify no update control for a current build and an available-version control for a newer build.
9. Verify the update dialog's cancellable pre-start state, progress state with visible disabled Cancel, success relaunch, and failure state with error, Retry, and enabled Cancel.
