// The colours of the pads of a MIDI controller, as the Mappings tab edits them: what a pad is sent when its slot is empty,
// holds a clip, or plays. The backend decides what each pad shows (`vj_feedback.rs`) and sends it; this only lets the user
// choose the value of each state, as a colour name where the controller has a colour table and as a number elsewhere.

import { useEffect, useState } from "react";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VjLights, VjTarget } from "../project/types";
import { vjLightsPreset } from "./api";

export type PadState = keyof VjLights;

/** Which states each kind of target shows: a clip or a column is dark, loaded or playing; the rest is off or on. */
const CLIP_STATES: PadState[] = ["off", "loaded", "on"];
const SWITCH_STATES: PadState[] = ["off", "on"];
const SWITCHED = new Set<VjTarget["kind"]>(["layerStop", "deckSelect", "stopAll", "layerBypass", "layerSolo", "groupBypass", "groupSolo", "effectBypass", "bpmPause"]);

/** The states a target's pad shows, or `null` when it has no light. */
export function padStates(target: VjTarget): PadState[] | null {
  if (target.kind === "clipLaunch" || target.kind === "columnLaunch") return CLIP_STATES;
  return SWITCHED.has(target.kind) ? SWITCH_STATES : null;
}

/** The APC40 and APC mini colour table: the velocity picks the colour. */
const APC_VALUES = [0, 1, 2, 3, 4, 5, 6];

type Preset = { lights: VjLights; palette: string | null };
const presets = new Map<string, Promise<Preset>>();

/** The backend's starting colours for the controller of `port`, asked once per port. */
function usePreset(port: string | undefined): Preset | null {
  const [preset, setPreset] = useState<Preset | null>(null);
  useEffect(() => {
    const key = port ?? "";
    let found = presets.get(key);
    if (!found) {
      found = vjLightsPreset(port);
      presets.set(key, found);
      found.catch(() => presets.delete(key));
    }
    let live = true;
    found.then((value) => live && setPreset(value)).catch(() => undefined);
    return () => {
      live = false;
    };
  }, [port]);
  return preset;
}

/** The three (or two) values of a pad. `value` is what is stored, absent while the pad follows its default. */
export function PadColours({ port, states, value, fallback, name, onSet }: {
  port?: string;
  states: PadState[];
  value: VjLights | undefined;
  /** What the pad uses while it has no colours of its own, when that is not the controller's preset. */
  fallback?: VjLights;
  name: string;
  onSet: (lights: VjLights | null) => void;
}) {
  const t = useT();
  const preset = usePreset(port);
  const shown = value ?? fallback ?? preset?.lights;
  if (!shown) return null;
  const apc = preset?.palette === "apc";
  const set = (state: PadState, v: number) => onSet({ ...shown, [state]: v });
  return (
    <span className="vj-lights" role="group" aria-label={`${name}: ${t("vj.lights.title")}`}>
      {states.map((state) => (
        <label key={state} className={`vj-light${apc ? ` c${shown[state]}` : ""}`} title={t(`vj.lights.note.${state}` as MessageKey)}>
          <span>{t(`vj.lights.${state}` as MessageKey)}</span>
          {apc ? (
            <select value={shown[state]} aria-label={`${name}: ${t(`vj.lights.${state}` as MessageKey)}`} onChange={(event) => set(state, Number(event.target.value))}>
              {(APC_VALUES.includes(shown[state]) ? APC_VALUES : [...APC_VALUES, shown[state]]).map((v) => (
                <option key={v} value={v}>{v <= 6 ? t(`vj.lights.apc.${v}` as MessageKey) : String(v)}</option>
              ))}
            </select>
          ) : (
            <input type="number" min={0} max={127} step={1} value={shown[state]} aria-label={`${name}: ${t(`vj.lights.${state}` as MessageKey)}`}
              onChange={(event) => {
                const v = Math.round(Number(event.target.value));
                if (Number.isFinite(v) && v >= 0 && v <= 127) set(state, v);
              }} />
          )}
        </label>
      ))}
      {value && <button type="button" className="vj-mp-inv" title={t("vj.lights.resetNote")} onClick={() => onSet(null)}>{t("vj.lights.reset")}</button>}
    </span>
  );
}
