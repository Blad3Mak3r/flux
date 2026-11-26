// Modulation of the VJ screen, as in Resolume: a numeric setting driven by an LFO or by the sound, between a
// minimum and a maximum. Each modulatable setting has a "~" button that opens a sub-row under it. The backend owns
// the modulation, its limits and its values; this draws what the catalogue says, forwards the user's intent and marks
// on the control where the backend says the modulated value is now. It never computes a wave.

import type { ComponentProps, ReactNode } from "react";
import { ModulationIcon } from "../components/icons";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VjClip, VjLayer, VjLfoShape, VjModSource, VjModulation } from "../project/types";
import { effectModulation } from "../project/types";
import type { ModTarget, VjModulationCatalog } from "./api";
import { Scrub, Segments } from "./controls";
import { useEffectCatalog } from "./effects";
import { edit, preview, showModulation, useVj } from "./store";

/** The key of what a modulation is set on, which the screen keeps its open sub-rows by. */
export function modKey(target: ModTarget): string {
  if ("effect" in target) return `e:${target.effect}:${target.param}`;
  if ("layer" in target) return `l:${target.layer}:${target.field}`;
  return `c:${target.clip}:${target.field}`;
}

/** Whether the inspector shows anything modulated: the clip, its layer or an effect of either. */
export function inspectorModulated(layer: VjLayer | undefined, clip: VjClip | undefined, composition?: VjLayer["effects"]): boolean {
  const modulated = (holder: { modulation?: object; effects?: VjLayer["effects"] } | undefined) =>
    !!holder &&
    (Object.keys(holder.modulation ?? {}).length > 0 ||
      (holder.effects ?? []).some((effect) => Object.keys(effectModulation(effect) ?? {}).length > 0));
  return modulated(layer) || modulated(clip) || modulated({ effects: composition });
}

/** How a setting is counted, for the modulation's minimum and maximum. */
export interface ModRange {
  min: number;
  max: number;
  /** Where a new modulation starts, within the range. */
  start?: [number, number];
  /** What is shown times the value, after the number: `%` is 100. */
  shown: number;
  unit: string;
  decimals: number;
}

/** The modulated value now (from the backend), 0 to 1 along `range`; `undefined` when it is not known. */
export function useModMarker(target: ModTarget, modulation: VjModulation | undefined, range: ModRange): number | undefined {
  const value = useVj((vj) =>
    "effect" in target ? vj.mod.effects[target.effect]?.[target.param]
    : "layer" in target ? vj.mod.layers[target.layer]?.[target.field]
    : vj.mod.clips[target.clip]?.[target.field],
  );
  if (!modulation || value === undefined || range.max === range.min) return undefined;
  return (value - range.min) / (range.max - range.min);
}

const SHAPE_PATHS: Record<VjLfoShape, string> = {
  sine: "M1 8 C3 2 5 2 8 8 C11 14 13 14 15 8",
  triangle: "M1 13 L8 3 L15 13",
  sawUp: "M1 13 L8 3 V13 L15 3",
  sawDown: "M1 3 L8 13 V3 L15 13",
  square: "M1 13 V3 H8 V13 H15",
  random: "M1 9 H4 V4 H7 V12 H10 V6 H13 V10 H15",
  smoothRandom: "M1 9 C3 9 3 4 5.5 4 C8 4 8 12 10.5 12 C13 12 13 6 15 6",
};

/** A shape drawn small, as the waveform it makes over one cycle. */
function ShapeIcon({ shape }: { shape: VjLfoShape }) {
  return (
    <svg className="icon vj-shape" viewBox="0 0 16 16" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
      <path d={SHAPE_PATHS[shape]} />
    </svg>
  );
}

/** A length of a cycle in beats, as a musician writes it: 1/4, 1/2, 1, 2... */
export const beatLabel = (beats: number) => (beats < 1 ? `1/${Math.round(1 / beats)}` : String(beats));

/** The "~" button of a setting: lit while it is modulated. A click starts a modulation and opens its sub-row, or opens and closes it. */
export function ModButton({ target, modulation, range, name }: { target: ModTarget; modulation: VjModulation | undefined; range: ModRange; name: string }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog?.modulation);
  const key = modKey(target);
  const open = useVj((vj) => vj.modOpen[key] === true);
  const start = range.start ?? [range.min, range.max];
  const click = () => {
    if (modulation) {
      showModulation(key);
      return;
    }
    if (!catalog) return;
    showModulation(key, true);
    void edit({ op: "setModulation", target, modulation: { ...catalog.start, min: start[0], max: start[1] } });
  };
  return (
    <button type="button" className={`vj-mod-b${modulation ? " on" : ""}${open ? " open" : ""}`} aria-pressed={!!modulation} aria-expanded={modulation ? open : undefined}
      title={t(modulation ? "vj.mod.edit" : "vj.mod.add")} aria-label={`${name}: ${t(modulation ? "vj.mod.edit" : "vj.mod.add")}`} disabled={!catalog} onClick={click}>
      <ModulationIcon />
    </button>
  );
}

/** The sub-row of a modulated setting: its source, shape or band, pace, and the minimum and maximum it moves between. */
export function ModPanel({ target, modulation, range }: { target: ModTarget; modulation: VjModulation | undefined; range: ModRange }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog?.modulation);
  const key = modKey(target);
  const open = useVj((vj) => vj.modOpen[key] === true);
  if (!open || !modulation || !catalog) return null;
  const { source } = modulation;
  // What a double click on a number puts back: the backend's starting LFO and audio source.
  const lfo0 = catalog.start.source.kind === "lfo" ? catalog.start.source : null;
  const audio0 = catalog.audio.kind === "audio" ? catalog.audio : null;
  const send = (next: VjModulation | null) => void edit({ op: "setModulation", target, modulation: next });
  const move = (next: VjModulation) => preview({ op: "setModulation", target, modulation: next });
  const withSource = (patch: Partial<VjModSource>) => ({ ...modulation, source: { ...source, ...patch } }) as VjModulation;
  const field = (name: string, control: ReactNode) => (
    <div className="vj-f"><span>{name}</span>{control}</div>
  );
  const seconds = (label: string, name: string, value: number, reset: number, patch: (v: number) => Partial<VjModSource>) => (
    <Scrub label={label} name={name} value={value} shown={1} reset={reset} unit="s" decimals={2} perPixel={catalog.maxSmoothing / 200} step={0.01}
      onMove={(v) => move(withSource(patch(v)))} onSet={(v) => send(withSource(patch(v)))} />
  );
  const extent = Math.abs(range.max - range.min);
  const bound = (which: "min" | "max") => (
    <Scrub label={t(which === "min" ? "vj.mod.min" : "vj.mod.max")} name={t(which === "min" ? "vj.mod.minName" : "vj.mod.maxName")} value={modulation[which]}
      shown={range.shown} reset={(range.start ?? [range.min, range.max])[which === "min" ? 0 : 1]} unit={range.unit} decimals={range.decimals}
      perPixel={extent / 160} step={extent / 100} onMove={(v) => move({ ...modulation, [which]: v })} onSet={(v) => send({ ...modulation, [which]: v })} />
  );
  return (
    <div className="vj-mod" role="group" aria-label={t("vj.mod.label")}>
      {field(t("vj.mod.source"),
        <Segments options={["lfo", "audio", "off"] as const} value={source.kind}
          label={(o) => t(`vj.mod.source.${o}` as MessageKey)}
          onPick={(o) => {
            if (o === "off") {
              showModulation(key, false);
              send(null);
            } else send({ ...modulation, source: o === "lfo" ? catalog.start.source : catalog.audio });
          }} />)}
      {source.kind === "lfo" ? (
        <>
          {field(t("vj.mod.shape"),
            <Segments options={catalog.shapes} value={source.shape} label={(o) => <ShapeIcon shape={o} />}
              title={(o) => t(`vj.mod.shape.${o}` as MessageKey)} onPick={(shape) => send(withSource({ shape }))} />)}
          {field(t("vj.mod.pace"),
            <Segments options={["free", "beats"] as const} value={source.pace.sync} label={(o) => t(`vj.effect.param.sync.${o}` as MessageKey)}
              onPick={(sync) => send(withSource({ pace: { ...source.pace, sync } }))} />)}
          {source.pace.sync === "free"
            ? field(t("vj.mod.rate"),
              <div className="vj-scrubs one">
                <Scrub label="↔" name={t("vj.mod.rate")} value={source.pace.rate} shown={1} reset={lfo0?.pace.rate ?? source.pace.rate} unit="Hz" decimals={2} perPixel={catalog.maxRate / 300} step={0.01}
                  onMove={(v) => move(withSource({ pace: { ...source.pace, rate: v } }))} onSet={(v) => send(withSource({ pace: { ...source.pace, rate: v } }))} />
              </div>)
            : field(t("vj.mod.beats"),
              <Segments options={catalog.beats.map(String)} value={String(source.pace.beats)} label={(o) => beatLabel(Number(o))}
                onPick={(o) => send(withSource({ pace: { ...source.pace, beats: Number(o) } }))} />)}
          {field(t("vj.mod.phase"),
            <div className="vj-scrubs one">
              <Scrub label="↔" name={t("vj.mod.phase")} value={source.phase} shown={100} reset={lfo0?.phase ?? source.phase} unit="%" perPixel={1 / 100} step={0.01}
                onMove={(v) => move(withSource({ phase: v }))} onSet={(v) => send(withSource({ phase: v }))} />
            </div>)}
        </>
      ) : (
        <>
          {field(t("vj.mod.band"),
            <Segments options={catalog.bands} value={source.band} label={(o) => t(`vj.mod.band.${o}` as MessageKey)}
              onPick={(band) => send(withSource({ band }))} />)}
          {field(t("vj.mod.gain"),
            <div className="vj-scrubs one">
              <Scrub label="×" name={t("vj.mod.gain")} value={source.gain} shown={1} reset={audio0?.gain ?? source.gain} unit="" decimals={2} perPixel={catalog.maxGain / 200} step={0.05}
                onMove={(v) => move(withSource({ gain: v }))} onSet={(v) => send(withSource({ gain: v }))} />
            </div>)}
          {field(t("vj.mod.smoothing"),
            <div className="vj-scrubs">
              {seconds(t("vj.mod.attack"), t("vj.mod.attackName"), source.attack, audio0?.attack ?? source.attack, (attack) => ({ attack }))}
              {seconds(t("vj.mod.release"), t("vj.mod.releaseName"), source.release, audio0?.release ?? source.release, (release) => ({ release }))}
            </div>)}
        </>
      )}
      {field(t("vj.mod.between"), <div className="vj-scrubs">{bound("min")}{bound("max")}</div>)}
    </div>
  );
}

/** A number that can be modulated: its box carries the "~" button and a mark where the modulated value is now. */
export function ModScrub({ target, modulation, range, ...scrub }: ComponentProps<typeof Scrub> & { target: ModTarget; modulation: VjModulation | undefined; range: ModRange }) {
  const marker = useModMarker(target, modulation, range);
  return <Scrub {...scrub} marker={marker} trailing={<ModButton target={target} modulation={modulation} range={range} name={scrub.name} />} />;
}

/** How a number of a generator is counted: the backend's range and start, the control's own units. */
export function generatorRange(catalog: VjModulationCatalog | undefined, name: string, units: Pick<ModRange, "shown" | "unit" | "decimals">): ModRange | undefined {
  const spec = catalog?.generator[name];
  return spec && { min: spec.min, max: spec.max, start: spec.start, ...units };
}

/** A number of a generator clip: its scrub, with the "~" button when the backend lists the number as modulatable. */
export function GeneratorScrub({ clip, modulation, name, ...scrub }: ComponentProps<typeof Scrub> & { clip: string; modulation: VjClip["modulation"]; name: string; field: string }) {
  const catalog = useEffectCatalog((state) => state.catalog?.modulation);
  const { field, ...rest } = scrub;
  const range = generatorRange(catalog, field, { shown: rest.shown, unit: rest.unit ?? "%", decimals: rest.decimals ?? 0 });
  const target = { clip, field: `generator.${field}` };
  return range ? <ModScrub {...rest} name={name} target={target} modulation={modulation?.[target.field]} range={range} /> : <Scrub {...rest} name={name} />;
}

/** The sub-row of the modulation of a generator number, under its control while it is open. */
export function GeneratorModPanel({ clip, modulation, field, shown, unit = "%", decimals = 0 }: { clip: string; modulation: VjClip["modulation"]; field: string; shown: number; unit?: string; decimals?: number }) {
  const catalog = useEffectCatalog((state) => state.catalog?.modulation);
  const range = generatorRange(catalog, field, { shown, unit, decimals });
  const target = { clip, field: `generator.${field}` };
  return range ? <ModPanel target={target} modulation={modulation?.[target.field]} range={range} /> : null;
}

/** How a layer's or a clip's modulatable field is counted (a percent), from the backend's catalogue; `undefined` until it has loaded. */
export function fieldRange(catalog: VjModulationCatalog | undefined, holder: "layer" | "clip", field: string): ModRange | undefined {
  const spec = catalog?.[holder][field];
  return spec && { min: spec.min, max: spec.max, start: spec.start, shown: 100, unit: "%", decimals: 0 };
}
