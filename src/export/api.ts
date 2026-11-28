import { Channel, invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import type { Translate } from "../i18n/messages";
import type { CueAction, CueKind } from "../live/api";
import type { Project } from "../project/types";

// Mirrors src-tauri/src/export_commands.rs and src-tauri/crates/flux-engine/src/cue_sheet.rs. The backend builds
// the sheet from the cues of the Live view; the UI only brings the words in the user's language and the path.

/** Which timelines go into the sheet. */
export type SheetScope = "active" | "all";

const KINDS: CueKind[] = ["operator", "audio", "note", "midi", "osc", "dmx", "video"];
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

export type LtcDepth = "pcm16" | "pcm24" | "float32";

/** What the export dialog sends; mirrors `LtcOptions` in src-tauri/src/export_commands.rs, which checks the ranges. */
export interface LtcOptions {
  sampleRate: number;
  depth: LtcDepth;
  /** Seconds of LTC before the timeline starts. */
  preRollSeconds: number;
}

export const LTC_SAMPLE_RATES = [44100, 48000, 96000];
export const LTC_DEPTHS: LtcDepth[] = ["pcm16", "pcm24", "float32"];
export const LTC_MAX_PRE_ROLL = 60;
export const LTC_DEFAULTS: LtcOptions = { sampleRate: 48000, depth: "pcm24", preRollSeconds: 0 };

/**
 * Asks where to save the LTC of the active timeline and renders it there as a mono WAV file in the format
 * of `options`. `onProgress` hears a fraction from 0 to 1, only when the render takes over a second.
 * Resolves to the frames written, or `null` when the dialog is cancelled.
 */
export async function exportLtc(
  project: Project,
  options: LtcOptions,
  onProgress: (fraction: number) => void,
): Promise<number | null> {
  const path = await save({
    filters: [{ name: "WAV", extensions: ["wav"] }],
    defaultPath: `${project.name} LTC.wav`,
  });
  if (path === null) return null;
  const progress = new Channel<number>();
  progress.onmessage = onProgress;
  return invoke<number>("ltc_export", { project, path, options, onProgress: progress });
}
