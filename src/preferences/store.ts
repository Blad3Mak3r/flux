import { create } from "zustand";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Locale } from "../i18n/messages";
import type { OutputConfig, Project } from "../project/types";
import { errorMessage } from "../project/api";

export interface AudioChoice { id: string; name: string | null; users: number }

export const UNASSIGNED = "__flux_unassigned__";
export const EMPTY_OUTPUT: OutputConfig = { audio: null, ltc: [], mtc: [] };
export interface Preferences {
  audioRoutes: Record<string, OutputConfig["audio"]>;
  voice: string | null;
  operator: OutputConfig["audio"];
  locale: Locale | null;
  driver: string | null;
  audio: OutputConfig["audio"];
  preview: OutputConfig["audio"];
  metronome: OutputConfig["audio"];
  ltc: Record<string, OutputConfig["ltc"][number]>;
  mtc: Record<string, OutputConfig["mtc"][number]>;
  revision: number;
  warning?: string | null;
}
const defaults: Preferences = {
  audioRoutes: {},
  voice: null,
  operator: null,
  locale: null,
  driver: null,
  audio: null,
  preview: null,
  metronome: null,
  ltc: {},
  mtc: {},
  revision: 0,
};
export const usePreferences = create<{
  saved: Preferences;
  ready: boolean;
  error: string | null;
  output: OutputConfig;
  editOutput: OutputConfig;
  audition: boolean;
  choices: AudioChoice[];
}>(() => ({
  saved: defaults,
  ready: !isTauri(),
  error: null,
  output: EMPTY_OUTPUT,
  editOutput: EMPTY_OUTPUT,
  audition: false,
  choices: [],
}));

function accept(saved: Preferences) {
  if (saved.revision >= usePreferences.getState().saved.revision)
    usePreferences.setState({ saved, error: saved.warning ?? null });
}
let startup: Promise<void> | undefined;
export function initPreferences(): Promise<void> {
  return (startup ??= (async () => {
    if (!isTauri()) return;
    try {
      await listen<Preferences>("preferences-changed", ({ payload }) =>
        accept(payload),
      );
      accept(await invoke<Preferences>("preferences_get"));
      const legacy = localStorage.getItem("flux.locale");
      if (legacy === "en" || legacy === "es") {
        await changePreferences({
          op: "locale",
          locale: legacy,
          onlyIfUnset: true,
        });
        localStorage.removeItem("flux.locale");
      }
    } catch (err) {
      usePreferences.setState({ error: errorMessage(err) });
    } finally {
      usePreferences.setState({ ready: true });
    }
  })());
}

export async function changePreferences(change: object): Promise<void> {
  try {
    accept(await invoke<Preferences>("preferences_change", { change }));
  } catch (err) {
    usePreferences.setState({ error: errorMessage(err) });
    throw err;
  }
}

/** UI events carry intent; validation, channel parsing and defaults belong to Rust. */
export const routeEdit = (
  kind: string,
  field: string,
  text: string,
  slot = 0,
) =>
  void changePreferences({ op: "route", kind, field, text, slot }).catch(
    () => undefined,
  );

let request = 0;
export async function resolveOutput(project: Project): Promise<OutputConfig> {
  await initPreferences();
  if (!isTauri()) return EMPTY_OUTPUT;
  const id = ++request;
  const result = await invoke<{
    choices: AudioChoice[];
    live: OutputConfig;
    edit: OutputConfig;
    audition: boolean;
  }>("preferences_resolve", { project });
  if (id === request)
    usePreferences.setState((before) => ({
      choices: JSON.stringify(before.choices) === JSON.stringify(result.choices) ? before.choices : result.choices,
      output: result.live,
      editOutput: result.edit,
      audition: result.audition,
    }));
  return result.live;
}
