// The blend modes and transitions a layer or a clip offers, as the options of a select.

import { useT } from "../i18n";
import type { VjBlend, VjLayer } from "../project/types";
import type { VjOp } from "./api";

// Grouped as Resolume lists them.
const BLEND_GROUPS: { group: "general" | "darken" | "lighten" | "contrast" | "comparative" | "colour" | "keys"; blends: VjBlend[] }[] = [
  { group: "general", blends: ["alpha"] },
  { group: "darken", blends: ["darken", "multiply", "colorBurn"] },
  { group: "lighten", blends: ["lighten", "screen", "colorDodge"] },
  { group: "contrast", blends: ["overlay", "softLight", "hardLight", "linearLight", "vividLight", "pinLight"] },
  { group: "comparative", blends: ["add", "subtract", "difference", "differenceInverted", "exclusion"] },
  { group: "colour", blends: ["hue", "saturation", "color", "luminosity"] },
  { group: "keys", blends: ["alphaCut", "lumaKey", "lumaKeyInverted", "lumaIsAlpha", "mask50", "rgb", "displace"] },
];

/** The blend options, as a call: `Select` reads them as children, so they cannot hide in a component. */
export function blendOptions(t: ReturnType<typeof useT>) {
  return (
    <>
      {BLEND_GROUPS.map(({ group, blends }) => (
        <optgroup key={group} label={t(`vj.blend.group.${group}`)}>
          {blends.map((blend) => <option key={blend} value={blend}>{t(`vj.blend.${blend}`)}</option>)}
        </optgroup>
      ))}
    </>
  );
}

/** The transition mode options: Random, then every blend. */
export function transitionOptions(t: ReturnType<typeof useT>) {
  return (
    <>
      <option value="random">{t("vj.blend.random")}</option>
      {blendOptions(t)}
    </>
  );
}

/** The Select value and the edit for a layer's transition mode, Random being its own entry. */
export const transitionValue = (layer: VjLayer) => (layer.transitionRandom ? "random" : (layer.transitionEffect ?? "alpha"));

export const transitionEdit = (layer: VjLayer, value: string): VjOp =>
  value === "random"
    ? { op: "layerTransitionEffect", layer: layer.id, effect: layer.transitionEffect ?? "alpha", random: true }
    : { op: "layerTransitionEffect", layer: layer.id, effect: value as VjBlend };
