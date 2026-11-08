import { create } from "zustand";
import type { Viewport } from "./waveform";

export const RULER_H = 34;
export const AUDIO_LANE_H = 120;
export const NOTES_LANE_H = 64;

/** Pointer feedback on the timeline, in frames from the timeline origin. */
export interface Readout {
  hover: number | null;
  spanA: number | null;
  spanB: number | null;
}

export const useReadout = create<Readout>()(() => ({ hover: null, spanA: null, spanB: null }));

/** The pointer reports far more often than the frame under it changes; only a change is news. */
export function patchReadout(patch: Partial<Readout>): void {
  const current = useReadout.getState();
  const changed = (Object.keys(patch) as (keyof Readout)[]).some((key) => patch[key] !== current[key]);
  if (changed) useReadout.setState(patch);
}

/** Frames between the two ends of the measured span, or `null` when none is being measured. */
export const selectSpan = ({ spanA, spanB }: Readout): number | null =>
  spanA !== null && spanB !== null ? Math.abs(spanB - spanA) : null;

/** What of the timeline is on screen, in pixels from its origin; waveforms draw only that. */
export const useViewport = create<Viewport>()(() => ({ left: 0, width: 1200 }));

export function setViewport(patch: Partial<Viewport>): void {
  const current = useViewport.getState();
  if ((Object.keys(patch) as (keyof Viewport)[]).some((key) => patch[key] !== current[key])) useViewport.setState(patch);
}
