// The catalogue of effects the backend offers: which kinds there are, in which category, and what each
// setting takes. The interface draws its lists and controls from it and holds no range of its own.

import { fail } from "../state/fail";
import { create } from "zustand";
import { vjEffectCatalog } from "./api";
import type { VjEffectCatalog, VjEffectSpec } from "./api";
import type { MessageKey } from "../i18n";

export const useEffectCatalog = create<{ catalog: VjEffectCatalog | null }>(() => ({ catalog: null }));

let loading: Promise<void> | null = null;

/** Asks the backend for the catalogue once. */
export function loadEffectCatalog(): Promise<void> {
  loading ??= vjEffectCatalog()
    .then((catalog) => useEffectCatalog.setState({ catalog }))
    .catch((err) => {
      loading = null;
      fail(err);
    });
  return loading;
}

export const specOf = (catalog: VjEffectCatalog | null, name: string): VjEffectSpec | undefined =>
  catalog?.effects.find((spec) => spec.name === name);

/** The words of an effect: its name, its short description or one of its settings. */
export const effectKey = (name: string, part?: "hint") =>
  (part ? `vj.effect.${name}.${part}` : `vj.effect.${name}`) as MessageKey;
export const categoryKey = (category: string) => `vj.effect.cat.${category}` as MessageKey;
export const paramKey = (effect: string, key: string) => `vj.effect.${effect}.${key}` as MessageKey;
export const genericParamKey = (key: string) => `vj.effect.param.${key}` as MessageKey;
export const choiceKey = (key: string, option: string) => `vj.effect.param.${key}.${option}` as MessageKey;
