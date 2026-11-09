import { invoke } from "@tauri-apps/api/core";

// Mirrors src-tauri/src/update_commands.rs.

/** The version of a newer release, or `null` when the app is up to date. */
export const updateCheck = () => invoke<string | null>("update_check");
/** Downloads the release `updateCheck` found, installs it and restarts the app. */
export const updateInstall = () => invoke<void>("update_install");
