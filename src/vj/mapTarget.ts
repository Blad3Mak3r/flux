// What a mappable control is, apart from the words that name it (see `mapModel.ts`): pure, so a test can run it.

import type { VjTarget } from "../project/types";

export type TargetClass = "continuous" | "switch" | "action";

const SWITCHES = new Set<VjTarget["kind"]>(["layerBypass", "layerSolo", "effectBypass", "bpmPause"]);
const CONTINUOUS = new Set<VjTarget["kind"]>([
  "layerMaster", "layerVolume", "layerOpacity", "layerTransition", "crossfader", "bpm",
  "clipSpeed", "effectParam", "generatorNumber", "dashboard",
]);

/** How a source drives a target: a level, a switch it flips or sets, or an action it runs. */
export const targetClass = (target: VjTarget): TargetClass =>
  CONTINUOUS.has(target.kind) ? "continuous" : SWITCHES.has(target.kind) ? "switch" : "action";

/** A dashboard knob links to a continuous target other than the crossfader-free ones that make a loop. */
export const linkable = (target: VjTarget): boolean =>
  targetClass(target) === "continuous" && target.kind !== "dashboard";

/** Equal for the same target: its kind, then its other fields in name order. */
export function targetKey(target: VjTarget): string {
  const { kind, ...rest } = target as VjTarget & Record<string, unknown>;
  return [kind, ...Object.keys(rest).sort().map((key) => String(rest[key]))].join(":");
}

/** The attributes that make a control mappable: the overlay of the map mode reads the target from them. */
export const mappable = (target: VjTarget | undefined) => (target ? { "data-map": JSON.stringify(target) } : {});
