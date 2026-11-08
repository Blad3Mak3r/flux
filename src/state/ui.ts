// What the user is looking at and doing, apart from the document: selection, inspector, zoom, the
// status line and the question waiting for an answer.

import { create } from "zustand";
import type { MessageKey } from "../i18n/messages";
import type { SetupSection } from "../setup/outputs";
import { DEFAULT_PX_PER_FRAME, clampZoom } from "../timeline/scale";
import { isPlaying } from "../transport/store";

/** The timeline is edited in `edit` and run on stage in `live`. */
export type View = "edit" | "live";

/** What the ruler counts: timecode, or bars and beats from the timeline's tempo. */
export type RulerMode = "time" | "bars";

/** A track name or a clip name being typed in place. */
export interface Renaming {
  kind: "track" | "clip";
  id: string;
}

/** How far apart the lines the magnet snaps to are: those on the ruler at this zoom, or a fixed step. */
export type GridStep = "auto" | "frame" | "second" | "beat" | "bar";

export const GRID_STEPS: readonly GridStep[] = ["auto", "frame", "second", "beat", "bar"];

/** What the magnet pulls onto: the grid, the edges of clips and notes (and the origin), the playhead. */
export interface SnapSources {
  grid: boolean;
  items: boolean;
  playhead: boolean;
}

/** Where the magnet is holding an item or a point right now, for the guide line; `kind` is what it holds on to. */
export interface SnapGuide {
  frame: number;
  kind: "grid" | "items" | "playhead" | "origin";
}

export interface Selected {
  kind: "clip" | "note" | "event" | "operator" | "track";
  id: string;
}

/** A status line is either a translatable message or raw text from the backend (an error). */
export type Message =
  | { key: MessageKey; values?: Record<string, string | number>; tone: "info" }
  | { text: string; tone: "error" };

/** What waits for the user to decide about unsaved work. */
export type Pending = () => Promise<void> | void;

interface MenuRow {
  label: string;
  shortcut?: string;
  checked?: boolean;
  disabled?: boolean;
}

/** One row of a menu: a context menu or a menu of the menu bar. `"-"` is a separator; a row with a `submenu` opens it beside it. */
export type MenuItem = "-" | (MenuRow & { run: () => void }) | (MenuRow & { submenu: MenuItem[] });

/** The menu on show, at a point of the window; `owner` is the menu bar title it hangs from. */
export interface OpenMenu {
  x: number;
  y: number;
  items: MenuItem[];
  owner?: string;
}

interface UiState {
  selection: Selected | null;
  /** The name being edited in place, if any. */
  renaming: Renaming | null;
  collapsed: boolean;
  view: View;
  ruler: RulerMode;
  /** Clips, notes and the playhead snap onto the vertical lines of the grid when dragged near one. */
  snap: boolean;
  snapSources: SnapSources;
  gridStep: GridStep;
  /** Set while a drag is held by the magnet; the guide line follows it. */
  snapGuide: SnapGuide | null;
  /** The timeline scrolls to keep the playhead in view while the show plays. */
  follow: boolean;
  /** The user scrolled by hand while following: it stays put until they say otherwise. */
  detached: boolean;
  zoom: number;
  /** A new document starts the timeline over: scrolled to the origin, at the default zoom. */
  docKey: number;
  message: Message;
  errors: string[];
  pending: Pending | null;
  /** The page of the Setup dialog that is open, or `null` while it is closed. */
  setup: SetupSection | null;
  showCheck: boolean;
  menu: OpenMenu | null;
  /** The timeline waiting for the user to confirm that it is removed. */
  removing: string | null;
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
const RULER_KEY = "flux.ruler";
const SNAP_KEY = "flux.snap";
const SNAP_SOURCES_KEY = "flux.snap.sources";
const GRID_STEP_KEY = "flux.snap.grid";

function readSnapSources(): SnapSources {
  const all = { grid: true, items: true, playhead: true };
  try {
    const stored = JSON.parse(localStorage.getItem(SNAP_SOURCES_KEY) ?? "null");
    return stored && typeof stored === "object"
      ? {
          grid: stored.grid !== false,
          items: stored.items !== false,
          playhead: stored.playhead !== false,
        }
      : all;
  } catch {
    return all;
  }
}

function readGridStep(): GridStep {
  try {
    const stored = localStorage.getItem(GRID_STEP_KEY);
    return GRID_STEPS.find((step) => step === stored) ?? "auto";
  } catch {
    return "auto";
  }
}

function readSnap(): boolean {
  try {
    return localStorage.getItem(SNAP_KEY) === "1";
  } catch {
    return false;
  }
}

function readRuler(): RulerMode {
  try {
    return localStorage.getItem(RULER_KEY) === "bars" ? "bars" : "time";
  } catch {
    return "time";
  }
}

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
  renaming: null,
  collapsed: readCollapsed(),
  view: readView(),
  ruler: readRuler(),
  snap: readSnap(),
  snapSources: readSnapSources(),
  gridStep: readGridStep(),
  snapGuide: null,
  follow: readFollow(),
  detached: false,
  zoom: DEFAULT_PX_PER_FRAME,
  docKey: 0,
  message: { key: "status.ready", tone: "info" },
  errors: [],
  pending: null,
  setup: null,
  showCheck: false,
  menu: null,
  removing: null,
}));

const set = useUi.setState;
const get = useUi.getState;

export function select(selection: Selected | null): void {
  set({ selection });
}

/** Opens the name of a track or a clip for typing; `null` closes it. */
export function rename(renaming: Renaming | null): void {
  set({ renaming });
}

export function setCollapsed(collapsed: boolean): void {
  set({ collapsed });
  try {
    localStorage.setItem(INSPECTOR_KEY, collapsed ? "1" : "0");
  } catch {
    // Not remembered, still works.
  }
}

/** The view cannot change while playing: Edit and Live may use different devices. Stop first. */
export function setView(view: View): void {
  if (view === get().view) return;
  if (isPlaying()) return say("view.lockedPlaying");
  set({ view });
  try {
    localStorage.setItem(VIEW_KEY, view);
  } catch {
    // Not remembered, still works.
  }
}

export function setRulerMode(ruler: RulerMode): void {
  set({ ruler });
  try {
    localStorage.setItem(RULER_KEY, ruler);
  } catch {
    // Not remembered, still works.
  }
}

/** The magnet: items and the playhead land on the grid lines when dragged near one. */
export function toggleSnap(): void {
  const snap = !get().snap;
  set({ snap });
  try {
    localStorage.setItem(SNAP_KEY, snap ? "1" : "0");
  } catch {
    // Not remembered, still works.
  }
}

/** Turns one of the things the magnet pulls onto on or off. */
export function setSnapSource(source: keyof SnapSources, on: boolean): void {
  const snapSources = { ...get().snapSources, [source]: on };
  set({ snapSources });
  try {
    localStorage.setItem(SNAP_SOURCES_KEY, JSON.stringify(snapSources));
  } catch {
    // Not remembered, still works.
  }
}

export function setGridStep(gridStep: GridStep): void {
  set({ gridStep });
  try {
    localStorage.setItem(GRID_STEP_KEY, gridStep);
  } catch {
    // Not remembered, still works.
  }
}

/** Shows the guide line at `guide`, or hides it. A guide that has not moved is not written again. */
export function setSnapGuide(guide: SnapGuide | null): void {
  const current = get().snapGuide;
  if (current === guide || (current && guide && current.frame === guide.frame && current.kind === guide.kind)) return;
  set({ snapGuide: guide });
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
export const sayError = (text: string) => set((ui) => ({
  errors: ui.errors.includes(text) ? ui.errors : [...ui.errors, text],
}));
export const dismissError = () => set((ui) => ({ errors: ui.errors.slice(1) }));

// The element that had the focus when a menu opened gets it back when the menu closes.
let returnFocus: Element | null = null;

export function openMenu(menu: OpenMenu): void {
  if (!get().menu) returnFocus = document.activeElement;
  set({ menu });
}

export function closeMenu(): void {
  if (!get().menu) return;
  set({ menu: null });
  if (returnFocus instanceof HTMLElement) returnFocus.focus();
  returnFocus = null;
}

/** Opens `items` at the pointer of a right click, and keeps the click from reaching the webview's own menu. */
export function menuAtPointer(
  event: { clientX: number; clientY: number; preventDefault(): void; stopPropagation(): void },
  items: MenuItem[],
): void {
  event.preventDefault();
  event.stopPropagation();
  openMenu({ x: event.clientX, y: event.clientY, items });
}

export const askRemoveTimeline = (id: string) => set({ removing: id });
export const clearRemoving = () => set({ removing: null });

export const ask = (pending: Pending | null) => set({ pending });
