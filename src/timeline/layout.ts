import { createStore } from "../store";

export const HEADER_W = 208;
export const RULER_H = 34;
export const AUDIO_LANE_H = 96;
export const EVENTS_LANE_H = 48;

/** Pointer feedback on the timeline, in frames from the timeline origin. */
export interface Readout {
  hover: number | null;
  spanA: number | null;
  spanB: number | null;
}

export const readoutStore = createStore<Readout>({ hover: null, spanA: null, spanB: null });

export function patchReadout(patch: Partial<Readout>): void {
  readoutStore.set({ ...readoutStore.get(), ...patch });
}
