// The user's effect presets and show templates. They live in the backend's app data folder, outside any show, so they
// are listed from it and every change is a command that answers with the new list; this holds that list, the question
// for a name and the preset menus, and shows the project a preset was applied to through `apply` (one undo step).

import { fail } from "../state/fail";
import { create } from "zustand";
import { openMenu } from "../state/ui";
import type { MenuItem } from "../state/ui";
import type { Translate } from "../i18n/messages";
import { dispatch, useDoc } from "../state/store";
import { say } from "../state/ui";
import {
  vjPresetApply,
  vjPresetDelete,
  vjPresetRename,
  vjPresetSave,
  vjPresetsList,
  vjTemplateDelete,
  vjTemplateRename,
  vjTemplateSave,
  vjTemplatesList,
} from "./api";
import type { VjPreset, VjPresetSource, VjTemplate } from "./api";
import { effectKey } from "./effects";

/** What the name question is for: which list it names an entry of, and what the answer does. */
export interface NameAsk {
  title: string;
  value: string;
  confirm: string;
  run: (name: string) => Promise<void>;
}

interface PresetsState {
  presets: VjPreset[];
  templates: VjTemplate[];
  ask: NameAsk | null;
  /** The list being managed, or none. */
  manage: "presets" | "templates" | null;
}

export const usePresets = create<PresetsState>()(() => ({ presets: [], templates: [], ask: null, manage: null }));

/** Reads both lists from the backend. */
export async function loadPresets(): Promise<void> {
  try {
    const [presets, templates] = await Promise.all([vjPresetsList(), vjTemplatesList()]);
    usePresets.setState({ presets, templates });
  } catch (err) {
    fail(err);
  }
}

/** The kind of effect a preset of one effect is for; `null` for a stack. */
export const presetKind = (preset: VjPreset): string | null => (preset.effects.length === 1 ? preset.effects[0].settings.effect : null);

export const askName = (ask: NameAsk) => usePresets.setState({ ask });
export const closeAsk = () => usePresets.setState({ ask: null });
export const managePresets = (manage: PresetsState["manage"]) => usePresets.setState({ manage });

/** Asks for a name, then keeps `source` as a preset of that name. */
export function savePreset(t: Translate, source: VjPresetSource, suggestion: string): void {
  askName({
    title: t("vj.preset.saveTitle"),
    value: suggestion,
    confirm: t("vj.preset.save"),
    run: async (name) => {
      try {
        usePresets.setState({ presets: await vjPresetSave(useDoc.getState().project, source, name) });
        say("vj.preset.saved", { name });
      } catch (err) {
        fail(err);
      }
    },
  });
}

/** Uses a preset on an effect of its kind, or adds its effects to a stack. */
export async function applyPreset(id: string, to: VjPresetSource): Promise<void> {
  try {
    const { project, active } = await vjPresetApply(useDoc.getState().project, id, to);
    dispatch({ type: "apply", project, active });
  } catch (err) {
    fail(err);
  }
}

export function renamePreset(t: Translate, preset: VjPreset): void {
  askName({
    title: t("vj.preset.renameTitle"),
    value: preset.name,
    confirm: t("vj.preset.rename"),
    run: async (name) => {
      try {
        usePresets.setState({ presets: await vjPresetRename(preset.id, name) });
      } catch (err) {
        fail(err);
      }
    },
  });
}

export async function deletePreset(id: string): Promise<void> {
  try {
    usePresets.setState({ presets: await vjPresetDelete(id) });
  } catch (err) {
    fail(err);
  }
}

/** Asks for a name, then keeps the structure of the show, without its clips, as a template. */
export function saveTemplate(t: Translate): void {
  askName({
    title: t("vj.template.saveTitle"),
    value: useDoc.getState().project.name,
    confirm: t("vj.template.save"),
    run: async (name) => {
      try {
        usePresets.setState({ templates: await vjTemplateSave(useDoc.getState().project, name) });
        say("vj.template.saved", { name });
      } catch (err) {
        fail(err);
      }
    },
  });
}

export function renameTemplate(t: Translate, template: VjTemplate): void {
  askName({
    title: t("vj.template.renameTitle"),
    value: template.name,
    confirm: t("vj.preset.rename"),
    run: async (name) => {
      try {
        usePresets.setState({ templates: await vjTemplateRename(template.id, name) });
      } catch (err) {
        fail(err);
      }
    },
  });
}

export async function deleteTemplate(id: string): Promise<void> {
  try {
    usePresets.setState({ templates: await vjTemplateDelete(id) });
  } catch (err) {
    fail(err);
  }
}

/**
 * Opens the menu of presets under `anchor`: the ones that fit (`only` kind of effect, or all of them for a stack) to
 * use on `to`, then saving `source` as a new one and managing the library.
 */
export function presetMenu(t: Translate, anchor: HTMLElement, to: VjPresetSource, source: VjPresetSource, suggestion: string, only?: string, canSave = true): void {
  const { presets } = usePresets.getState();
  const fitting = presets.filter((preset) => only === undefined || presetKind(preset) === only);
  const items: MenuItem[] = [
    { heading: t("vj.preset.menu") },
    ...(fitting.length === 0
      ? [{ label: t("vj.preset.none"), disabled: true, run: () => undefined }]
      : fitting.map((preset): MenuItem => ({ label: presetLabel(t, preset), run: () => void applyPreset(preset.id, to) }))),
    "-",
    { label: t("vj.preset.saveAs"), disabled: !canSave, run: () => savePreset(t, source, suggestion) },
    { label: t("vj.preset.manage"), run: () => managePresets("presets") },
  ];
  const { left, bottom } = anchor.getBoundingClientRect();
  openMenu({ x: left, y: bottom, owner: "presets", items });
}

/** A preset as listed: its name, and what a stack holds. */
export const presetLabel = (t: Translate, preset: VjPreset): string => {
  const kind = presetKind(preset);
  return kind ? preset.name : `${preset.name} (${t("vj.preset.stackOf", { n: preset.effects.length })})`;
};

/** The default name of a preset of the effect `kind`. */
export const suggestionFor = (t: Translate, kind: string) => t(effectKey(kind));
