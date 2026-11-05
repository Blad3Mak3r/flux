// What the user is looking at and doing, apart from the document: selection, inspector, zoom, the
// status line and the question waiting for an answer.

import { create } from "zustand";
import type { MessageKey } from "../i18n/messages";
import type { SetupSection } from "../setup/outputs";
import { DEFAULT_PX_PER_FRAME, clampZoom } from "../timeline/scale";
import { isPlaying } from "../transport/store";

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
  /** The page of the Setup dialog that is open, or `null` while it is closed. */
  setup: SetupSection | null;
  menu: OpenMenu | null;
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
  collapsed: readCollapsed(),
  view: readView(),
  follow: readFollow(),
  detached: false,
  zoom: DEFAULT_PX_PER_FRAME,
  docKey: 0,
  message: { key: "status.ready", tone: "info" },
  pending: null,
  setup: null,
  menu: null,
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
export const sayError = (text: string) => set({ message: { text, tone: "error" } });

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

export const ask = (pending: Pending | null) => set({ pending });
