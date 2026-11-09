import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { Translate } from "../i18n/messages";
import type { CueAction, CueKind } from "../live/api";
import type { Project } from "../project/types";

// Mirrors src-tauri/src/export_commands.rs and src-tauri/crates/flux-engine/src/cue_sheet.rs. The backend builds
// the sheet from the cues of the Live view; the UI only brings the words in the user's language and the path.

/** Which timelines go into the sheet. */
export type SheetScope = "active" | "all";

const KINDS: CueKind[] = ["operator", "audio", "note", "midi", "osc"];
const ACTIONS: CueAction[] = ["noteOn", "noteOff", "controlChange", "programChange", "msc", "sysEx", "osc"];
const RATES = ["fps24", "fps25", "fps2997Drop", "fps30"] as const;

function sheetLabels(t: Translate) {
  return {
    lang: t("cueSheet.lang"),
    title: t("cueSheet.title"),
    timeline: t("cueSheet.timeline"),
    frameRate: t("project.frameRate"),
    empty: t("cueSheet.empty"),
    number: t("cueSheet.number"),
    timecode: t("cueSheet.timecode"),
    name: t("cueSheet.name"),
    kind: t("cueSheet.kind"),
    action: t("cueSheet.action"),
    detail: t("cueSheet.detail"),
    track: t("cueSheet.track"),
    duration: t("cueSheet.duration"),
    kinds: Object.fromEntries(KINDS.map((kind) => [kind, t(`live.kind.${kind}`)])),
    actions: Object.fromEntries(ACTIONS.map((action) => [action, t(`live.action.${action}`)])),
    rates: Object.fromEntries(RATES.map((rate) => [rate, t(`frameRate.${rate}`)])),
  };
}

/**
 * Asks where to save the cue sheet and writes it: a web page for `.html`, a spreadsheet for `.csv`.
 * Resolves to the path written, or `null` when the dialog is cancelled.
 */
export async function exportCueSheet(project: Project, scope: SheetScope, t: Translate): Promise<string | null> {
  const path = await save({
    filters: [
      { name: "CSV", extensions: ["csv"] },
      { name: "HTML", extensions: ["html"] },
    ],
    defaultPath: `${project.name}.csv`,
  });
  if (path === null) return null;
  await invoke<void>("cue_sheet_export", { project, scope, labels: sheetLabels(t), path });
  return path;
}
