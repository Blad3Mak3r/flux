# Flux Signal Blueprint and window controls

## Goal

Replace the current card-led desktop panel with the approved Signal Blueprint direction: a minimal, dark operational map that makes the Art-Net-to-ENTTEC bridge the permanent visual priority. Replace Tauri's native title bar with integrated functional window controls.

## Visual system

The working surface uses graphite and near-black backgrounds, ivory text and low-contrast slate dividers. Red is intentionally scarce: it identifies the selected Route destination, the live route connection and the recovery affordance. Green is reserved for verified healthy input/output states. No gradients, glow, glass treatment or nested card grid is introduced.

The application uses a narrow navigation rail, a quiet title strip and a large central route canvas. Input and output appear as explicit endpoints joined by the route. A compact device inspector stays visible at the right; the event/activity readout and telemetry form lower, secondary bands. Configuration and the full channel monitor remain on demand.

## Custom title strip

Tauri window decorations are disabled for the main window. The React shell renders a compact top strip with:

- Flux identity and the text-labelled global bridge condition;
- an empty drag region that moves the native window without stealing input from controls;
- minimize, maximize/restore and close controls with accessible names and tooltips.

The controls use Tauri's current-window API. Minimize calls the platform window minimize operation. Maximize uses the native toggle operation. Close requests the normal Tauri close operation, which continues to be intercepted by the existing Rust handler so Flux hides to the tray and keeps routing DMX. The controls do not introduce a separate quit path.

## Data and behaviour

Existing 500 ms snapshots, configuration draft handling, device refresh, reconnect behaviour and monitor-window commands remain unchanged. The UI only rearranges and restyles their existing data. The global condition remains text-labelled; red styling never becomes the only error or state signal.

The title strip remains keyboard reachable. Drag regions exclude buttons and form controls. Minimize, maximize and close retain visible focus states and standard button behaviour. On smaller permissible window sizes, the route map stacks its endpoints while the window controls retain a fixed hit area.

## Files and scope

- `tauri.conf.json`: disable main-window decorations.
- `ui/src/main.tsx`: add current-window actions and the custom title-strip component; reorganize the existing route, device, activity and configuration surfaces without changing Tauri commands.
- `ui/src/style.css`: implement the Signal Blueprint tokens, layout and title-strip/window-control states.

No Rust runtime, settings, tray menu or command signature changes are needed. The existing close-to-tray handler remains the authority for closing the main window.

## Verification

Build the Vite frontend and run the Tauri desktop application. Verify waiting, live and output-error route states; configuration reveal/apply/reset; reconnect action; monitor opening; keyboard focus; drag in the title strip; and minimize, maximize/restore and close-to-tray on the custom controls. Confirm the main bridge condition remains visible at every state and the tray reopens the hidden window.
