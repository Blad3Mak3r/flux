# Flux minimal operational UI

## Goal

Make Flux immediately answer whether its Art-Net-to-DMX route is working while retaining configuration and DMX inspection without adding operational noise.

## Scope

This change is limited to the React/Vite frontend. The existing Tauri commands, Rust runtime model, polling interval, settings persistence, and device reconnection behavior remain unchanged.

## Information hierarchy

The application opens on an operational view with three layers:

1. A compact header identifies Flux and gives one text-labelled global condition: operational, waiting for Art-Net, or requires attention.
2. A dominant route card presents Art-Net input and DMX output as a single route. It shows the active universe, source, last-packet age, selected output device, packet/frame rate, and refresh frequency. Input and output retain their own explicit condition labels.
3. Configuration and detailed channel data are secondary, on-demand areas. They do not compete with the initial health check.

The existing decorative hero copy and excess card treatments are removed. The dark technical visual language remains, with text labels accompanying all colour-based states and visible keyboard focus.

## Interactions

### Route condition

The UI continues to request a runtime snapshot every 500 ms. It derives the global condition from the input and output data: it is operational only when Art-Net is being received and DMX is transmitting; it is waiting when there is no recent input; and it requires attention when output reports an error or unavailable device. The last-packet age remains human readable and the visual input state changes after one second without traffic.

### Configuration

A `Configure route` control reveals the settings panel. Its form initially reflects saved settings, shows the selected device's relevant detail, and refreshes devices only on explicit user action. Applying is enabled only when a valid edit differs from the saved settings. A `Reset` action restores the saved draft locally. Success and failure messages are presented near the related action and do not replace the route condition.

### DMX monitor

The DMX monitor is opened from the operational view. It defaults to active channels only, reports the active channel count, and provides an explicit toggle for all 512 channels. It preserves the existing value bars and stays dismissible from the keyboard.

### Recovery

When output is unavailable, the route card exposes the reconnect action. Device discovery failures, settings failures, and snapshot failures remain bounded feedback messages with clear error styling; they do not remove the last known operational state.

## Components and data flow

The frontend separates presentation from the existing Tauri command calls:

- a route summary derives the global state and renders input/output telemetry;
- a settings panel owns its open state and local draft comparison;
- a channel monitor owns its display/filter state;
- a shared feedback area displays action outcomes.

The existing `runtime_snapshot`, `saved_settings`, `available_devices`, `save_settings`, and `reconnect_device` commands continue to be the only backend interface. No Rust interfaces change.

## Verification

Run TypeScript and Vite production builds. Manually exercise these frontend conditions: initial loading, waiting for Art-Net, active route, unavailable output device, device refresh error, a changed and reverted configuration draft, a successful settings apply, and active/all DMX channel monitor modes. Confirm keyboard navigation gives every control a visible focus state.
