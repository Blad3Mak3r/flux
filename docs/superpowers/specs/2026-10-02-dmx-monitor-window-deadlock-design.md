# DMX monitor window deadlock fix

## Goal

Open the DMX monitor as a separate React-backed Tauri webview on Windows without blocking Flux.

## Root cause

`open_dmx_monitor` is a synchronous Tauri command that calls `WebviewWindowBuilder::build`. Tauri documents a Windows WebView2 deadlock when a window is created from a synchronous command or event handler.

## Design

Keep the monitor as an on-demand native window rather than creating a hidden window at application startup. The Rust command becomes asynchronous. It first obtains the window labelled `dmx-monitor`: an existing window is shown and focused; otherwise a new resizable `WebviewWindow` is created with `WebviewUrl::App("monitor.html")`.

`monitor.html` remains a separate Vite entry point and renders the existing `MonitorApp` React tree. The monitor continues to use only `runtime_snapshot`; no additional frontend window-creation permission is granted and no capability configuration is needed while the current default command exposure remains in use.

## Error handling

Window creation, showing, and focusing continue to return their Tauri errors as command errors. The primary React application already presents those errors as actionable feedback. The monitor never terminates Flux when it is closed because the shared window-event handler prevents close and hides it.

## Verification

Run Rust tests and checks plus the TypeScript/Vite production build. On Windows, repeatedly open, close, and reopen the monitor; confirm that it reuses one window, updates its snapshot, and leaves the primary panel and Art-Net/DMX runtime responsive.
