// The map mode of the VJ screen, as in Resolume's "Edit MIDI / OSC": every mappable control shows an overlay, a click
// chooses what to learn, and the next control moved or OSC message heard becomes its source. The backend keeps the
// mappings and does the learning (`src-tauri/src/vj_control.rs`); this keeps only what the screen is doing.

import { create } from "zustand";
import { hear } from "../control/api";
import { dispatch } from "../state/store";
import type { Project } from "../project/types";
import type { VjSource, VjTarget } from "../project/types";
import { vjLearn } from "./api";
import { linkable } from "./mapModel";
import { edit } from "./store";

interface MapState {
  /** Whether the overlays show. */
  on: boolean;
  /** The target waiting for a control or an OSC message. */
  learn: VjTarget | null;
  /** The dashboard knob that the next continuous control clicked is linked to. */
  linking: number | null;
  /** The source of the last mapping learned, for the overlay to confirm it. */
  learned: VjSource | null;
  /** The key the backend refused to learn because the app uses it, until the next try. */
  refused: VjSource | null;
}

export const useMap = create<MapState>()(() => ({ on: false, learn: null, linking: null, learned: null, refused: null }));

const stopLearning = () => void vjLearn(null).catch(() => undefined);

export function setMapMode(on: boolean): void {
  if (on === useMap.getState().on) return;
  if (!on) stopLearning();
  useMap.setState({ on, learn: null, linking: null, learned: null, refused: null });
}

export const toggleMapMode = () => setMapMode(!useMap.getState().on);

/** Escape: leaves what is being done, the learn first, then the linking, then the mode. `false` when the mode is off. */
export function escapeMap(): boolean {
  const { on, learn, linking } = useMap.getState();
  if (!on) return false;
  if (learn) {
    stopLearning();
    useMap.setState({ learn: null });
  } else if (linking !== null) useMap.setState({ linking: null });
  else setMapMode(false);
  return true;
}

/** A click on the overlay of a control: with a knob being linked it adds a link, otherwise it waits for a source. */
export function pickTarget(target: VjTarget): void {
  const { linking } = useMap.getState();
  if (linking !== null) {
    if (linkable(target)) void edit({ op: "dashboardLink", knob: linking, target });
    return;
  }
  useMap.setState({ learn: target, learned: null, refused: null });
  void vjLearn(target).catch(() => undefined);
}

export const clearMapping = (target: VjTarget) => void edit({ op: "clearMapping", target });

/** Starts or stops linking a dashboard knob; the mode turns on, since the links are picked from the overlays. */
export function linkKnob(knob: number | null): void {
  if (knob !== null && !useMap.getState().on) useMap.setState({ on: true });
  if (knob !== null) stopLearning();
  useMap.setState({ linking: knob, learn: null });
}

/** Takes what the backend does for a controller (the states it sends while a control moves, the learned source). */
export function bindVjControl(): () => void {
  const offs = [
    hear<{ project: Project; active: string; commit: boolean }>("vj-control", (state) =>
      dispatch({ type: "controlled", project: state.project, active: state.active, commit: state.commit })),
    hear<VjSource>("vj-learned", (source) => useMap.setState({ learn: null, learned: source, refused: null })),
    hear<VjSource>("vj-learn-refused", (source) => useMap.setState({ refused: source })),
  ];
  return () => {
    offs.forEach((off) => off());
    setMapMode(false);
  };
}
