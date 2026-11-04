import type { Position } from "./api";

/**
 * The position right after Stop: stopped at the timeline start. The UI shows it at once instead of
 * waiting for the backend's next tick, so the readout and the playhead never
 * keep the old position.
 */
export function stoppedAt(last: Position, start: number): Position {
  return { ...last, state: "stopped", frame: start, positionSamples: 0 };
}
