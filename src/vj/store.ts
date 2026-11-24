// What the VJ screen shows besides the project: what the grid plays, which clip is selected and a
// fader being moved. The backend owns the grid and what plays; this only keeps its last answer.

import { create } from "zustand";
import { errorMessage } from "../project/api";
import { dispatch, useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { probeVideos } from "../state/video";
import {
  vjCrossfader,
  vjEdit,
  vjLaunch,
  vjLaunchColumn,
  vjPlayState,
  vjRelease,
  vjImport,
  vjStop,
} from "./api";
import type { GridState, VjOp } from "./api";

/** A video file being copied into the project, shown where it will land until it is there. */
export interface Importing {
  key: number;
  /** The file's name. */
  name: string;
  layer: string;
  /** The cell's column, or `null` for the layer's first empty cell. */
  column: string | null;
}

interface VjState {
  play: GridState;
  importing: Importing[];
  /** The clip the inspector shows. */
  clip: string | null;
  /** The layer the inspector shows. */
  layer: string | null;
  tab: "clip" | "layer";
  /** Which media the browser lists. */
  media: MediaKind;
}

export type MediaKind = "all" | "video" | "image";

export const useVj = create<VjState>()(() => ({
  play: { playing: [], column: null, crossfader: 0.5 },
  importing: [],
  clip: null,
  layer: null,
  tab: "clip",
  media: "all",
}));

const fail = (err: unknown) => sayError(errorMessage(err));

/** Applies an edit of the grid in the backend; the answer enters the undo history. */
export async function edit(op: VjOp): Promise<void> {
  try {
    const { project, active } = await vjEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project, active });
  } catch (err) {
    fail(err);
  }
}

let imports = 0;
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/**
 * Adds video files to `layer`, the first in the cell of `column` (or each in the layer's first empty
 * cell with `null`), then reads them: the clips show their state meanwhile.
 */
export async function importVideos(paths: string[], layer: string, column: string | null): Promise<void> {
  for (const [at, path] of paths.entries()) {
    const key = ++imports;
    const name = fileName(path);
    const cell = at === 0 ? column : null;
    useVj.setState((vj) => ({ importing: [...vj.importing, { key, name, layer, column: cell }] }));
    try {
      const before = useDoc.getState().project.videos ?? [];
      const { project, active } = await vjImport(useDoc.getState().project, path, layer, cell);
      dispatch({ type: "apply", project, active });
      const added = (project.videos ?? []).filter((video) => !before.some((old) => old.id === video.id));
      say("status.imported", { name });
      void probeVideos(added);
    } catch (err) {
      fail(err);
    } finally {
      useVj.setState((vj) => ({ importing: vj.importing.filter((item) => item.key !== key) }));
    }
  }
}

/**
 * Sends a level while its fader moves, so the output follows the hand: the backend takes it at
 * once, and only the last one, sent with {@link edit} when the fader is let go, enters the history.
 */
let moving: Promise<unknown> = Promise.resolve();
export function preview(op: VjOp): void {
  moving = moving.then(() => vjEdit(useDoc.getState().project, op)).catch(() => undefined);
}

const played = (run: () => Promise<GridState>) =>
  run()
    .then((play) => useVj.setState({ play }))
    .catch(fail);

export const launch = (clip: string, layer: string) => {
  useVj.setState({ clip, layer, tab: "clip" });
  return played(() => vjLaunch(clip));
};
export const release = (clip: string) => played(() => vjRelease(clip));
export const launchColumn = (column: string) => played(() => vjLaunchColumn(column));
export const stopLayer = (layer: string | null) => played(() => vjStop(layer));
export const moveCrossfader = (position: number) => played(() => vjCrossfader(position));

export const selectClip = (clip: string, layer: string) => useVj.setState({ clip, layer, tab: "clip" });
export const selectLayer = (layer: string) => useVj.setState({ layer, tab: "layer" });
export const showTab = (tab: VjState["tab"]) => useVj.setState({ tab });
export const showMedia = (media: MediaKind) => useVj.setState({ media });

/** Asks the backend what plays, ten times a second, while the VJ screen is open. */
export function bindPlayState(): () => void {
  let stopped = false;
  const tick = () => {
    if (stopped) return;
    vjPlayState()
      .then((play) => {
        if (!stopped) useVj.setState({ play });
      })
      .catch(() => undefined)
      .finally(() => {
        if (!stopped) window.setTimeout(tick, 100);
      });
  };
  tick();
  return () => {
    stopped = true;
  };
}
