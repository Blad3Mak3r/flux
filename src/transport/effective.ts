// What the output is opened with in each view. Auditioning in Edit goes to the preview device and
// sends no timecode; Live always uses the project's own outputs.

import type { OutputConfig } from "../project/types";

export type OutputView = "edit" | "live";

export function effectiveOutput(output: OutputConfig, view: OutputView): OutputConfig {
  if (view === "edit" && output.preview) return { audio: output.preview, ltc: [], mtc: [] };
  return output;
}
