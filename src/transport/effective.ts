// What the output is opened with in each view. Auditioning in Edit goes to the preview device and
// sends no timecode (the metronome keeps its device); Live always uses the project's own outputs.

import type { OutputConfig } from "../project/types";

export type OutputView = "edit" | "live";

export function effectiveOutput(output: OutputConfig, view: OutputView): OutputConfig {
  // The click is for the people on stage, not timecode: it keeps its own device in Edit too.
  if (view === "edit" && output.preview) {
    return { audio: output.preview, ltc: [], mtc: [], ...(output.metronome && { metronome: output.metronome }) };
  }
  return output;
}
