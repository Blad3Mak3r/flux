// What the user is looking at and doing, apart from the document: selection, inspector, zoom, the
// status line and the question waiting for an answer.

import { create } from "zustand";
import type { MessageKey } from "../i18n/messages";
import { DEFAULT_PX_PER_FRAME, clampZoom } from "../timeline/scale";

export type InspectorTab = "selection" | "project" | "output";

/** The timeline is edited in `edit` and run on stage in `live`. */
export type View = "edit" | "live";

export interface Selected {
  kind: "clip" | "note";
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
  tab: InspectorTab;
  collapsed: boolean;
  view: View;
  /** The timeline scrolls to keep the playhead in view while the show plays. */
  follow: boolean;
  /** The user scrolled by hand while following: it stays put until they say otherwise. */
  detached: boolean;
  zoom: number;
  /** A new document starts the timeline over: scrolled to the origin, at the default zoom. */
  docKey: number;
  message: Message;
  pending: Pending | null;
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
const FOLLOW_KEY = "flux.follow";

function readFollow(): boolean {
  try {
    return localStorage.getItem(FOLLOW_KEY) !== "0";
  } catch {
    return true;
  }
}

function readView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === "live" ? "live" : "edit";
  } catch {
    return "edit";
  }
}

export const useUi = create<UiState>()(() => ({
  selection: null,
  tab: "selection",
  collapsed: readCollapsed(),
  view: readView(),
  follow: readFollow(),
  detached: false,
  zoom: DEFAULT_PX_PER_FRAME,
  docKey: 0,
  message: { key: "status.ready", tone: "info" },
  pending: null,
}));

const set = useUi.setState;
const get = useUi.getState;

export function select(selection: Selected | null): void {
  set(selection ? { selection, tab: "selection" } : { selection });
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

/** The Follow control: it turns following on, or off; after a manual scroll it takes the view back. */
export function toggleFollow(): void {
  const { follow, detached } = get();
  const next = detached ? true : !follow;
  set({ follow: next, detached: false });
  try {
    localStorage.setItem(FOLLOW_KEY, next ? "1" : "0");
  } catch {
    // Not remembered, still works.
  }
}

export const setDetached = (detached: boolean) => set({ detached });

export const toggleInspector = () => setCollapsed(!get().collapsed);

/** Picks a tab; from the collapsed rail it also opens the sidebar. */
export function openTab(tab: InspectorTab): void {
  set({ tab });
  setCollapsed(false);
}

export const setZoom = (zoom: number) => set({ zoom: clampZoom(zoom) });
export const zoomBy = (factor: number) => setZoom(get().zoom * factor);

/** Back to the origin at the default zoom, nothing selected. */
export const resetView = () =>
  set((ui) => ({ selection: null, zoom: DEFAULT_PX_PER_FRAME, docKey: ui.docKey + 1 }));

export const say = (key: MessageKey, values?: Record<string, string | number>) =>
  set({ message: { key, values, tone: "info" } });
export const sayError = (text: string) => set({ message: { text, tone: "error" } });

export const ask = (pending: Pending | null) => set({ pending });
