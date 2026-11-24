// What the VJ screen shows besides the project: what the grid plays, which clip is selected and a
// fader being moved. The backend owns the grid and what plays; this only keeps its last answer.

import { create } from "zustand";
import { errorMessage } from "../project/api";
import { dispatch, useDoc } from "../state/store";
import { sayError } from "../state/ui";
import {
  vjCrossfader,
  vjEdit,
  vjLaunch,
  vjLaunchColumn,
  vjPlayState,
  vjRelease,
  vjStop,
} from "./api";
import type { GridState, VjOp } from "./api";

interface VjState {
  play: GridState;
  /** The clip the inspector shows. */
  clip: string | null;
  /** The layer the inspector shows. */
  layer: string | null;
  tab: "clip" | "layer";
}

export const useVj = create<VjState>()(() => ({
  play: { playing: [], column: null, crossfader: 0.5 },
  clip: null,
  layer: null,
  tab: "clip",
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
