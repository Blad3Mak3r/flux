// What the map mode shows about a mapping: how a target is classed, keyed and named. The backend decides what a
// mapping does (`flux-project/src/vj_mapping.rs`); these are only the words and the numbers' units to show it by.

import type { MessageKey, Translate } from "../i18n";
import type { VjEffectCatalog } from "./api";
import type { VideoAsset, VjClip, VjEffect, VjGrid, VjKnob, VjMapping, VjTarget } from "../project/types";
import type { DashScope } from "./api";
import { targetKey } from "./mapTarget";
import type { TargetClass } from "./mapTarget";

export { hasSlot, keyName, mappable, sourceKind, sourceLabel, targetKey } from "./mapTarget";
export type { SourceKind } from "./mapTarget";
export type { TargetClass } from "./mapTarget";
import { effectKey, genericParamKey, paramKey, specOf } from "./effects";
import { clipName, groupName, layerName } from "./names";

/** The mappings of a grid by the key of their target. */
export function mappingsByTarget(grid: VjGrid): Map<string, VjMapping[]> {
  const found = new Map<string, VjMapping[]>();
  for (const mapping of grid.mappings ?? []) {
    const key = targetKey(mapping.target);
    found.set(key, [...(found.get(key) ?? []), mapping]);
  }
  return found;
}

const clipOf = (grid: VjGrid, id: string): VjClip | undefined =>
  grid.layers.flatMap((layer) => layer.clips ?? []).find((clip) => clip.id === id);

function effectOf(grid: VjGrid, id: string): VjEffect | undefined {
  const stacks = [grid.compositionEffects, ...(grid.groups ?? []).map((group) => group.effects), ...grid.layers.flatMap((layer) => [layer.effects, ...(layer.clips ?? []).map((clip) => clip.effects)])];
  return stacks.flatMap((stack) => stack ?? []).find((effect) => effect.id === id);
}

const targetName = (kind: VjTarget["kind"]) => `vj.map.target.${kind}` as MessageKey;

/** The knobs of the dashboard of `scope` (the composition's when absent). */
export function knobsOf(grid: VjGrid, scope?: DashScope): VjKnob[] | undefined {
  if (!scope) return grid.dashboard;
  return scope.kind === "layer" ? grid.layers.find((l) => l.id === scope.layer)?.dashboard : clipOf(grid, scope.clip)?.dashboard;
}

/** A knob as it is called: its number, then its name if it has one. */
const knobName = (t: Translate, knobs: VjKnob[] | undefined, knob: number) =>
  t("vj.dashboard.knobNamed", { n: knob + 1, name: knobs?.[knob]?.name ?? "" }).trim();

/** What a target is called in the mappings list: where it is, then what of it. */
export function targetLabel(t: Translate, target: VjTarget, grid: VjGrid, videos: VideoAsset[]): string {
  const what = t(targetName(target.kind));
  switch (target.kind) {
    case "clipLaunch":
    case "clipSpeed":
    case "generatorNumber": {
      const clip = clipOf(grid, target.clip);
      const layer = grid.layers.find((l) => l.clips?.some((c) => c.id === target.clip));
      const name = clip ? clipName(t, clip, videos) : "?";
      const where = layer ? `${layerName(t, grid, layer)} · ${name}` : name;
      if (target.kind === "generatorNumber") return `${where} · ${t(`vj.gen.${target.name}` as MessageKey)}`;
      return target.kind === "clipSpeed" ? `${where} · ${what}` : where;
    }
    case "columnLaunch": {
      const columns = grid.decks.flatMap((deck) => deck.columns);
      const at = columns.findIndex((column) => column.id === target.column);
      return `${what} ${columns[at]?.name ?? at + 1}`;
    }
    case "deckSelect": {
      const at = grid.decks.findIndex((deck) => deck.id === target.deck);
      return `${what} ${grid.decks[at]?.name ?? at + 1}`;
    }
    case "effectBypass":
    case "effectParam": {
      const effect = effectOf(grid, target.effect);
      const name = effect ? t(effectKey(effect.effect)) : "?";
      if (target.kind === "effectBypass") return `${name} · ${what}`;
      return `${name} · ${t(paramKey(effect?.effect ?? "", target.param)) ?? t(genericParamKey(target.param))}`;
    }
    case "dashboard":
      return knobName(t, grid.dashboard, target.knob);
    case "layerDashboard": {
      const layer = grid.layers.find((l) => l.id === target.layer);
      return `${layer ? layerName(t, grid, layer) : "?"} · ${knobName(t, layer?.dashboard, target.knob)}`;
    }
    case "clipDashboard": {
      const clip = clipOf(grid, target.clip);
      const layer = grid.layers.find((l) => l.clips?.some((c) => c.id === target.clip));
      const name = clip ? clipName(t, clip, videos) : "?";
      return `${layer ? `${layerName(t, grid, layer)} · ` : ""}${name} · ${knobName(t, clip?.dashboard, target.knob)}`;
    }
    case "layerStop":
    case "layerAutopilot":
    case "layerBypass":
    case "layerSolo":
    case "layerMaster":
    case "layerVolume":
    case "layerOpacity":
    case "layerTransition": {
      const layer = grid.layers.find((l) => l.id === target.layer);
      return `${layer ? layerName(t, grid, layer) : "?"} · ${what}`;
    }
    case "groupBypass":
    case "groupSolo":
    case "groupMaster":
    case "groupOpacity": {
      const group = grid.groups?.find((g) => g.id === target.group);
      return `${group ? groupName(t, grid, group) : "?"} · ${what}`;
    }
    default:
      return what;
  }
}

const kindOf = (catalog: VjEffectCatalog | null, target: VjTarget) => catalog?.targets.find((k) => k.kind === target.kind);

/** How a source drives a target: a level, a switch it flips or sets, or an action it runs (the backend's say). */
export const targetClass = (catalog: VjEffectCatalog | null, target: VjTarget): TargetClass => kindOf(catalog, target)?.class ?? "action";

/** Whether a dashboard knob can link the target (the backend's say). */
export const linkable = (catalog: VjEffectCatalog | null, target: VjTarget): boolean => kindOf(catalog, target)?.linkable ?? false;

/** How a continuous target's number is counted: its range in its own units, and the way it is shown. */
export interface Units {
  min: number;
  max: number;
  /** The shown number is the value times this. */
  shown: number;
  unit: string;
  decimals: number;
}

const PERCENT: Units = { min: 0, max: 1, shown: 100, unit: "%", decimals: 0 };

/** The units of a target whose range the backend gives. */
function fixed(catalog: VjEffectCatalog | null, target: VjTarget, how: Omit<Units, "min" | "max">): Units | null {
  const range = kindOf(catalog, target)?.range;
  return range ? { min: range[0], max: range[1], ...how } : null;
}

/** The units of a continuous target; `null` while the catalogue it needs has not loaded. */
export function targetUnits(target: VjTarget, grid: VjGrid, catalog: VjEffectCatalog | null): Units | null {
  switch (target.kind) {
    case "layerMaster":
    case "layerVolume":
    case "layerOpacity":
    case "groupMaster":
    case "groupOpacity":
    case "crossfader":
    case "dashboard":
    case "layerDashboard":
    case "clipDashboard":
      return PERCENT;
    case "layerTransition":
      return fixed(catalog, target, { shown: 1, unit: "s", decimals: 1 });
    case "bpm":
      return fixed(catalog, target, { shown: 1, unit: "BPM", decimals: 0 });
    case "clipSpeed":
      return fixed(catalog, target, { shown: 1, unit: "×", decimals: 2 });
    case "generatorNumber": {
      const spec = catalog?.modulation.generator[target.name];
      if (!spec) return null;
      const percent = target.name === "opacity" || target.name === "width";
      return { min: spec.min, max: spec.max, shown: percent ? 100 : 1, unit: percent ? "%" : target.name === "rate" ? "Hz" : "", decimals: percent ? 0 : 2 };
    }
    case "effectParam": {
      const effect = effectOf(grid, target.effect);
      const param = effect ? specOf(catalog, effect.effect)?.params.find((p) => p.key === target.param) : undefined;
      if (!param || param.type !== "number") return null;
      const percent = param.unit === "%";
      return { min: param.min, max: param.max, shown: percent ? 100 : 1, unit: param.unit, decimals: percent || param.whole ? 0 : param.step >= 0.1 ? 1 : 2 };
    }
    default:
      return null;
  }
}
