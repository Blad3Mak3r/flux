// What the map mode shows about a mapping: how a target is classed, keyed and named. The backend decides what a
// mapping does (`flux-project/src/vj_mapping.rs`); these are only the words and the numbers' units to show it by.

import type { MessageKey, Translate } from "../i18n";
import type { VjEffectCatalog } from "./api";
import type { VideoAsset, VjClip, VjEffect, VjGrid, VjMapping, VjTarget } from "../project/types";
import { targetKey } from "./mapTarget";

export { hasSlot, keyName, linkable, mappable, sourceKind, sourceLabel, targetClass, targetKey } from "./mapTarget";
export type { SourceKind } from "./mapTarget";
export type { TargetClass } from "./mapTarget";
import { effectKey, genericParamKey, paramKey, specOf } from "./effects";
import { clipName, layerName } from "./names";

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
  const stacks = [grid.compositionEffects, ...grid.layers.flatMap((layer) => [layer.effects, ...(layer.clips ?? []).map((clip) => clip.effects)])];
  return stacks.flatMap((stack) => stack ?? []).find((effect) => effect.id === id);
}

const targetName = (kind: VjTarget["kind"]) => `vj.map.target.${kind}` as MessageKey;

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
      return t("vj.dashboard.knobNamed", { n: target.knob + 1, name: grid.dashboard?.[target.knob]?.name ?? "" }).trim();
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
    default:
      return what;
  }
}

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

/** The units of a continuous target; `null` while the catalogue it needs has not loaded. */
export function targetUnits(target: VjTarget, grid: VjGrid, catalog: VjEffectCatalog | null): Units | null {
  switch (target.kind) {
    case "layerMaster":
    case "layerVolume":
    case "layerOpacity":
    case "crossfader":
    case "dashboard":
      return PERCENT;
    case "layerTransition":
      return { min: 0, max: 10, shown: 1, unit: "s", decimals: 1 };
    case "bpm":
      return { min: 20, max: 400, shown: 1, unit: "BPM", decimals: 0 };
    case "clipSpeed":
      return { min: 0.05, max: 8, shown: 1, unit: "×", decimals: 2 };
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
