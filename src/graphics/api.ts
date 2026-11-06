import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/graphics.rs. The webview reads these once, when it starts, so a change is
// saved at once and applies after a restart.

export type GraphicsApi = "auto" | "d3d11" | "d3d12" | "openGl" | "vulkan";

export interface GraphicsPrefs {
  hardwareAcceleration: boolean;
  api: GraphicsApi;
}

export interface GraphicsState {
  /** What is saved, which starts the webview the next time. */
  saved: GraphicsPrefs;
  /** What this run started it with. */
  effective: GraphicsPrefs;
  /** This run ignored the saved preferences (`--safe-graphics`). */
  safeMode: boolean;
  /** `false` where the system always draws the same way (macOS). */
  accelerationConfigurable: boolean;
  /** The APIs this system lets the app pick; `auto` alone means there is no choice. */
  apis: GraphicsApi[];
  /** Saved and running differ: a restart shows the change. */
  restartNeeded: boolean;
}

export const DEFAULT_GRAPHICS: GraphicsPrefs = { hardwareAcceleration: true, api: "auto" };

export const graphicsGet = () => invoke<GraphicsState>("graphics_get");
export const graphicsSet = (prefs: GraphicsPrefs) => invoke<GraphicsState>("graphics_set", { prefs });
export const graphicsRestart = () => invoke<void>("graphics_restart");
