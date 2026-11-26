// Mappings as a file (`.flxmap`): export writes the grid's mappings and knob links, import reads them into the show
// shown. The backend reads the file, finds what would be overwritten and applies the choice; this picks the file,
// asks when something would be overwritten, and shows the answer through `apply` (one undo step).

import { open, save } from "@tauri-apps/plugin-dialog";
import { create } from "zustand";
import { errorMessage } from "../project/api";
import { dispatch, useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { vjMappingsExport, vjMappingsImport, vjMappingsImportPreview } from "./api";
import type { MapImportChoice, MapImportPlan } from "./api";

const FILTER = { name: "FLUX mappings", extensions: ["flxmap"] };

interface Pending {
  path: string;
  plan: MapImportPlan;
}

/** The import waiting for the user to decide what to do about the mappings it would overwrite. */
export const useImportAsk = create<{ pending: Pending | null }>()(() => ({ pending: null }));

export async function exportMappings(): Promise<void> {
  const doc = useDoc.getState().project;
  try {
    const path = await save({ filters: [FILTER], defaultPath: `${doc.name}.flxmap` });
    if (!path) return;
    await vjMappingsExport(doc, path);
    say("vj.mapfile.exported");
  } catch (err) {
    sayError(errorMessage(err));
  }
}

/** Picks a file; without conflicts it is imported at once, otherwise the dialog asks. */
export async function importMappings(): Promise<void> {
  try {
    const path = await open({ filters: [FILTER] });
    if (!path || Array.isArray(path)) return;
    const plan = await vjMappingsImportPreview(useDoc.getState().project, path);
    if (plan.conflicts.length === 0) await runImport(path, { wipe: false, keep: [] });
    else useImportAsk.setState({ pending: { path, plan } });
  } catch (err) {
    sayError(errorMessage(err));
  }
}

export async function runImport(path: string, choice: MapImportChoice): Promise<void> {
  useImportAsk.setState({ pending: null });
  try {
    const { project, active, imported, skipped, kept } = await vjMappingsImport(useDoc.getState().project, path, choice);
    dispatch({ type: "apply", project, active });
    say("vj.mapfile.imported", { imported, skipped, kept });
  } catch (err) {
    sayError(errorMessage(err));
  }
}
