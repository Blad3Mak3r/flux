# Route settings modal and UI scroll design

## Goal

Simplify the Flux desktop surface by removing the left navigation rail, placing route configuration in the top-right title strip, and presenting the configuration form in an accessible React modal. Keep Flux's custom window controls functional. Replace operating-system window scrolling with controlled, styled scrolling inside the application UI.

## Scope and constraints

- The main Tauri window remains undecorated, resizable, minimizable, and maximizable.
- React continues to own the custom title strip. The existing Tauri current-window calls remain the implementation for minimize, maximize/restore, and close.
- Close keeps the established Rust close interception: Flux hides to the system tray and continues routing DMX. This work does not add a quit control or change Rust handlers.
- The route configuration UI uses only React state and existing settings commands. No Tauri command or plugin is introduced for modal behavior.
- The main window itself does not scroll. Any required scrolling occurs in explicitly owned UI regions.

## Layout

The title strip keeps the Flux identity and text-labelled bridge condition on the left, followed by a drag region. Its right-side action group is ordered as follows:

1. A compact gear button labelled `Configure route`, with a visible tooltip and accessible name.
2. Minimize.
3. Maximize or restore.
4. Close to tray.

Window controls retain their fixed hit areas, hover treatment, keyboard focus indication, and error feedback. The gear button is outside the drag region, as are all other interactive controls.

Remove the complete left navigation rail and collapse the shell to a single workspace column. The route workspace and device inspector keep their current operational roles. The inspector continues to provide access to the DMX monitor, so no route or monitor capability is removed with the rail.

## Route settings modal

Selecting the gear opens a React-controlled dialog over the workspace. The dialog contains the existing route fields, device refresh, explanatory device state, Reset, Cancel, and Apply changes actions.

Opening the modal seeds a fresh draft from the last saved settings. Closing it through the close button, Escape, backdrop click, or Cancel discards the draft and restores it from the saved settings. Reset also restores the saved settings while leaving the modal open. Apply changes remains disabled until the draft is both valid and different from saved settings. After a successful save, Flux updates saved settings, synchronizes the draft, and closes the modal. A failed save leaves the dialog open and surfaces the existing error feedback.

The dialog uses `role="dialog"`, `aria-modal="true"`, and a labelled heading. Focus moves to the dialog on open, is contained while it is open, and returns to the gear button on close. The Escape handler and backdrop interaction must not close the dialog while an apply operation is in progress.

## Application-owned scrolling

The document root, application frame, and outer shell are constrained to the viewport and use hidden overflow. The workspace becomes the primary vertical scroll container below the title strip. It receives a narrow, dark scrollbar with a clearly visible thumb, hover treatment, and nonzero contrast against its track. Standard CSS scrollbar declarations cover Chromium/WebKit and Firefox.

The settings dialog is constrained to the available viewport height and has a separate, identically styled internal scroll region for the form when needed. Opening the modal prevents the workspace beneath it from scrolling. The modal never forces a native Windows window scrollbar. The title strip stays fixed and the three window controls remain reachable at every workspace scroll position.

## Error handling and accessibility

All state conditions continue to use words alongside colour. The gear, dialog close control, Cancel, and window operations have accessible names. Visible focus indicators remain present for the new button, modal controls, and scrollbar-owning regions where keyboard focus applies. Form validation and device detection messaging reuse the existing feedback patterns.

## Files and implementation boundaries

- `ui/src/main.tsx`: remove the navigation rail; add the gear button, reusable settings dialog behavior, focus and dismissal handling, and discard-on-close draft reset.
- `ui/src/style.css`: make the workspace and dialog the owned scroll containers; add scrollbar styling; style the title-strip configuration action and modal.
- `tauri.conf.json`: retain the existing undecorated, resizable, minimizable, and maximizable settings; no configuration change is expected unless validation proves a setting differs.

No Rust runtime, settings, tray, or command API modification is part of this work.

## Verification

1. Build the Vite frontend.
2. Confirm the left rail is absent and route/monitor access remains present.
3. Open the gear dialog and verify focus placement, focus containment, and return focus on close.
4. Edit values, then dismiss with close, Escape, backdrop, and Cancel; reopen each time and confirm values reset to the saved settings.
5. Verify Reset, validation, successful Apply-and-close, and failed-save behavior.
6. At normal and minimum window size, verify workspace scrolling is UI-owned and visually styled, no Windows window scrollbar appears, and modal scrolling does not move the background.
7. Verify minimize, maximize/restore, and close-to-tray; reopen from the tray and confirm routing remains active.
