// What a mappable control is, apart from the words that name it (see `mapModel.ts`): pure, so a test can run it.

import { noteName } from "../control/model.ts";
import type { VjSource, VjTarget } from "../project/types";

export type TargetClass = "continuous" | "switch" | "action";

const SWITCHES = new Set<VjTarget["kind"]>(["layerBypass", "layerSolo", "groupBypass", "groupSolo", "effectBypass", "bpmPause"]);
const CONTINUOUS = new Set<VjTarget["kind"]>([
  "layerMaster", "layerVolume", "layerOpacity", "layerTransition", "groupMaster", "groupOpacity", "crossfader", "bpm",
  "clipSpeed", "effectParam", "generatorNumber", "dashboard", "layerDashboard", "clipDashboard",
]);

/** How a source drives a target: a level, a switch it flips or sets, or an action it runs. */
export const targetClass = (target: VjTarget): TargetClass =>
  CONTINUOUS.has(target.kind) ? "continuous" : SWITCHES.has(target.kind) ? "switch" : "action";

/** A dashboard knob links to a continuous target other than another knob, which would make a loop. */
export const linkable = (target: VjTarget): boolean =>
  targetClass(target) === "continuous" && !/ashboard$/.test(target.kind);

/** Equal for the same target: its kind, then its other fields in name order. */
export function targetKey(target: VjTarget): string {
  const { kind, ...rest } = target as VjTarget & Record<string, unknown>;
  return [kind, ...Object.keys(rest).sort().map((key) => String(rest[key]))].join(":");
}

/** The attributes that make a control mappable: the overlay of the map mode reads the target from them. */
export const mappable = (target: VjTarget | undefined) => (target ? { "data-map": JSON.stringify(target) } : {});

/** The name of a key from its `KeyboardEvent.code`, whatever the layout: `KeyQ` is Q, `Digit1` is 1. */
export function keyName(code: string): string {
  const named = /^Key([A-Z])$/.exec(code) ?? /^Digit(\d)$/.exec(code);
  if (named) return named[1];
  const pad = /^Numpad(.+)$/.exec(code);
  if (pad) return `Num ${pad[1]}`;
  return code.replace(/^Arrow/, "").replace(/([a-z])([A-Z])/g, "$1 $2");
}

/** The kinds of source the Mappings tab can filter by. */
export type SourceKind = "midi" | "osc" | "dmx" | "key";

export const sourceKind = (source: VjSource): SourceKind => (source.type === "artnet" ? "dmx" : source.type === "key" ? "key" : source.type);

/** Whether a target is a clip, layer, group, column, deck or effect, which a mapping can follow by id or by slot. */
export const hasSlot = (target: VjTarget): boolean =>
  /^(clip|column|layer|group|deck|effect|generator)/.test(target.kind);

export function sourceLabel(source: VjSource): string {
  if (source.type === "osc") return source.address;
  if (source.type === "artnet") return `DMX ${source.universe}:${source.channel}${source.fine ? `/${source.fine}` : ""}`;
  if (source.type === "key") {
    const mods = [source.ctrl && "Ctrl", source.alt && "Alt", source.shift && "Shift", source.meta && "Meta"].filter(Boolean);
    return `Key ${[...mods, keyName(source.code)].join("+")}`;
  }
  const channel = `ch ${source.channel}`;
  if (source.kind === "note") return `Note ${noteName(source.number)} · ${channel}`;
  if (source.kind === "cc") return `CC ${source.number} · ${channel}`;
  return `Pitch bend · ${channel}`;
}

