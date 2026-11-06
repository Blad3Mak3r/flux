import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

// Mirrors src-tauri/src/diagnostics/mod.rs.

/** Opens the folder with the log files in the system's file manager. */
export const openLogDir = () => invoke<void>("diagnostics_open_log_dir");

/**
 * Asks where to save a zip with the logs and a report of the system and the devices, and writes it.
 * Resolves to the path written, or `null` when the dialog is cancelled.
 */
export async function exportDiagnostics(): Promise<string | null> {
  const day = new Date().toISOString().slice(0, 10);
  const path = await save({
    filters: [{ name: "Zip", extensions: ["zip"] }],
    defaultPath: `flux-diagnostics-${day}.zip`,
  });
  if (path === null) return null;
  await invoke<void>("diagnostics_export", { path });
  return path;
}
