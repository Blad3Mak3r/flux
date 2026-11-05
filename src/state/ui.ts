// What the user is looking at and doing, apart from the document: selection, inspector, zoom, the
// status line and the question waiting for an answer.

import { create } from "zustand";
import type { MessageKey } from "../i18n/messages";
import type { SetupSection } from "../setup/outputs";
import { DEFAULT_PX_PER_FRAME, clampZoom } from "../timeline/scale";

/** The timeline is edited in `edit` and run on stage in `live`. */
export type View = "edit" | "live";

export interface Selected {
  kind: "clip" | "trigger";
  id: string;
}

/** A status line is either a translatable message or raw text from the backend (an error). */
export type Message =
  | { key: MessageKey; values?: Record<string, string | number>; tone: "info" }
  | { text: string; tone: "error" };

/** What waits for the user to decide about unsaved work. */
export type Pending = () => Promise<void> | void;

interface UiState {
  selection: Selected | null;
  collapsed: boolean;
  view: View;
  zoom: number;
  /** A new document starts the timeline over: scrolled to the origin, at the default zoom. */
  docKey: number;
  message: Message;
  pending: Pending | null;
  /** The page of the Setup dialog that is open, or `null` while it is closed. */
  setup: SetupSection | null;
}

const INSPECTOR_KEY = "flux.inspector.collapsed";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(INSPECTOR_KEY) === "1";
  } catch {
    return false;
  }
}

const VIEW_KEY = "flux.view";

function readView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === "live" ? "live" : "edit";
  } catch {
    return "edit";
  }
}

export const useUi = create<UiState>()(() => ({
  selection: null,
  collapsed: readCollapsed(),
  view: readView(),
  zoom: DEFAULT_PX_PER_FRAME,
  docKey: 0,
  message: { key: "status.ready", tone: "info" },
  pending: null,
  setup: null,
}));

const set = useUi.setState;
const get = useUi.getState;

export function select(selection: Selected | null): void {
  set({ selection });
}

export function setCollapsed(collapsed: boolean): void {
  set({ collapsed });
  try {
    localStorage.setItem(INSPECTOR_KEY, collapsed ? "1" : "0");
  } catch {
    // Not remembered, still works.
  }
}

export function setView(view: View): void {
  set({ view });
  try {
    localStorage.setItem(VIEW_KEY, view);
  } catch {
    // Not remembered, still works.
  }
}

export const toggleInspector = () => setCollapsed(!get().collapsed);

/** Opens the Setup dialog on a page, or moves it to another while it is open. */
export const openSetup = (section: SetupSection = "project") => set({ setup: section });
export const closeSetup = () => set({ setup: null });

export const setZoom = (zoom: number) => set({ zoom: clampZoom(zoom) });
export const zoomBy = (factor: number) => setZoom(get().zoom * factor);

/** Back to the origin at the default zoom, nothing selected. */
export const resetView = () =>
  set((ui) => ({ selection: null, zoom: DEFAULT_PX_PER_FRAME, docKey: ui.docKey + 1 }));

export const say = (key: MessageKey, values?: Record<string, string | number>) =>
  set({ message: { key, values, tone: "info" } });
export const sayError = (text: string) => set({ message: { text, tone: "error" } });

export const ask = (pending: Pending | null) => set({ pending });
