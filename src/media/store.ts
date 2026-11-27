// What the window knows of the project's media: the backend's list (it looks at the files), which
// of it is offline, and the page that shows it. The backend decides everything; this keeps its answer.

import { create } from "zustand";
import { useDoc } from "../state/store";
import { mediaList } from "./api";
import type { CollectProgress, MediaItem } from "./api";

const NO_ITEMS: MediaItem[] = [];
const NONE: ReadonlySet<string> = new Set();

interface MediaStore {
  items: MediaItem[];
  missing: number;
  /** Ids of the media whose file is gone. */
  offline: ReadonlySet<string>;
  /** The media page is open. */
  open: boolean;
  /** The asset the page was opened for: its row is selected and scrolled into view. */
  focus: string | null;
  /** Something is running on the backend (a search, a relink or a copy). */
  busy: boolean;
  /** The copy of a collect, while it runs. */
  progress: CollectProgress | null;
}

export const useMedia = create<MediaStore>(() => ({
  items: NO_ITEMS,
  missing: 0,
  offline: NONE,
  open: false,
  focus: null,
  busy: false,
  progress: null,
}));

export const openMedia = () => useMedia.setState({ open: true, focus: null });
/** Opens the page with one asset's row selected, so Relink… acts on it. */
export const focusMedia = (asset: string) => useMedia.setState({ open: true, focus: asset });
export const closeMedia = () => useMedia.setState({ open: false, focus: null });

let asked = 0;

/** Asks the backend which media is offline, and keeps the answer. */
export async function refreshMedia(): Promise<void> {
  const ask = ++asked;
  try {
    const list = await mediaList(useDoc.getState().project);
    if (ask !== asked) return;
    const offline = new Set(list.items.filter((item) => item.state === "offline").map((item) => item.id));
    useMedia.setState({ items: list.items, missing: list.missing, offline });
  } catch {
    // Without the backend there is nothing to show; the next change asks again.
  }
}

/** The assets whose file is there: reading an offline one only fails. */
export async function withoutOffline<T extends { id: string }>(assets: T[]): Promise<T[]> {
  if (assets.length === 0) return assets;
  await refreshMedia();
  const { offline } = useMedia.getState();
  return assets.filter((asset) => !offline.has(asset.id));
}

/** Refreshes the list when the project's media changes and when the window gets the focus back. */
export function bindMedia(): () => void {
  let timer: number | undefined;
  const later = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void refreshMedia(), 150);
  };
  const stop = useDoc.subscribe((doc, before) => {
    if (doc.project.assets !== before.project.assets || doc.project.videos !== before.project.videos) later();
  });
  window.addEventListener("focus", later);
  later();
  return () => {
    stop();
    window.removeEventListener("focus", later);
    window.clearTimeout(timer);
  };
}
