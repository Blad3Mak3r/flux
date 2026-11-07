import type { AudioRoutingState } from "./api";
// What the running output reports, plus where the playhead rests when none runs. It changes about
// 30 times a second, so components read one field through a selector and re-render only when that
// field does; the meters read it straight from `getState()` in their own animation loop.

import { create } from "zustand";
import type { OutputInfo, Position, TransportState as Mode } from "./api";

export interface TransportState {
  /** Latest position from the backend, or `null` while no output runs. */
  position: Position | null;
  /** Peak of each audio track in the latest tick, linear, by track id. */
  levels: Record<string, number>;
  /** Frame count to show when no output runs (absolute, including the start offset). */
  parked: number;
  audioRouting: AudioRoutingState | null;
  info: OutputInfo | null;
  /** Files the output could not use (missing or unreadable audio), from starting or from edits. */
  clipWarnings: string[];
  /** The output settings changed while playing; they are applied when playback stops or pauses. */
  pendingReconfigure: boolean;
}

export const useTransport = create<TransportState>()(() => ({
  position: null,
  levels: {},
  parked: 0,
  audioRouting: null,
  info: null,
  clipWarnings: [],
  pendingReconfigure: false,
}));

export const selectFrame = (s: TransportState): number => s.position?.frame ?? s.parked;
export const selectMode = (s: TransportState): Mode => s.position?.state ?? "stopped";
export const currentFrame = (): number => selectFrame(useTransport.getState());
export const isPlaying = (): boolean => selectMode(useTransport.getState()) === "playing";
