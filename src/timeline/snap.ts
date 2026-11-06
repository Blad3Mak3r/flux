// The magnet: moves a position onto the closest vertical line of the grid when it is near one.

/** How close, in pixels, a position must be to a line for the magnet to take it. */
export const SNAP_PX = 8;

/**
 * `frame` moved onto the closest multiple of `spacing` (frames between lines) when that is within
 * `SNAP_PX` on screen, else left as it is. The result is a whole frame, as every position is.
 */
export function snapToSpacing(frame: number, spacing: number, pxPerFrame: number): number {
  if (!(spacing > 0)) return frame;
  const nearest = Math.round(frame / spacing) * spacing;
  return Math.abs(nearest - frame) * pxPerFrame <= SNAP_PX ? Math.round(nearest) : frame;
}

// ---- Snapping onto items, the playhead and the origin, next to the grid ----

/** A fixed step of the grid: it stays the same at every zoom, unlike the ruler's own lines. */
export type FixedStep = "frame" | "second" | "beat" | "bar";

/** Frames between the lines of a fixed grid step. Beats are rarely a whole number of frames. */
export function fixedSpacing(step: FixedStep, tempo: { bpm: number; beatsPerBar: number; fps: number }): number {
  const beat = (60 / tempo.bpm) * tempo.fps;
  switch (step) {
    case "frame":
      return 1;
    case "second":
      return tempo.fps;
    case "beat":
      return beat;
    case "bar":
      return beat * tempo.beatsPerBar;
  }
}

/** What a position can be pulled onto: the grid's lines, the edges of clips and notes, the playhead, frame 0. */
export type SnapSource = "grid" | "items" | "playhead" | "origin";

/** A frame, counted from the timeline origin, that is not on the grid but is worth landing on. */
export interface SnapTarget {
  frame: number;
  source: Exclude<SnapSource, "grid">;
}

/** Where a position landed: `source` is `null` when nothing was in reach and `frame` is as it was. */
export interface Snapped {
  frame: number;
  source: SnapSource | null;
}

/** A clip, a note or an event, as far as snapping is concerned: where it starts and, when it has one, ends. */
export interface SnapItem {
  id: string;
  start: number;
  end?: number;
}

/** Every target, ordered by frame; two on one frame keep the first one's source. */
export function collectTargets(
  items: readonly SnapItem[],
  options: { exclude?: ReadonlySet<string>; playhead?: number | null; sources: ReadonlySet<SnapSource> },
): SnapTarget[] {
  const { exclude, playhead, sources } = options;
  const found: SnapTarget[] = [];
  if (sources.has("origin")) found.push({ frame: 0, source: "origin" });
  if (sources.has("playhead") && playhead !== null && playhead !== undefined && playhead >= 0) {
    found.push({ frame: Math.round(playhead), source: "playhead" });
  }
  if (sources.has("items")) {
    for (const item of items) {
      if (exclude?.has(item.id)) continue;
      found.push({ frame: item.start, source: "items" });
      if (item.end !== undefined && item.end > item.start) found.push({ frame: item.end, source: "items" });
    }
  }
  found.sort((a, b) => a.frame - b.frame);
  return found.filter((target, index) => index === 0 || target.frame !== found[index - 1].frame);
}

/** The target closest to `frame`, with its distance in frames; `null` when there are none. */
export function nearestTarget(frame: number, targets: readonly SnapTarget[]): { target: SnapTarget; distance: number } | null {
  if (targets.length === 0) return null;
  let low = 0;
  let high = targets.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (targets[middle].frame < frame) low = middle + 1;
    else high = middle;
  }
  const after = targets[low];
  const before = targets[low - 1];
  if (!before) return { target: after, distance: Math.abs(after.frame - frame) };
  if (!after) return { target: before, distance: Math.abs(before.frame - frame) };
  return frame - before.frame <= after.frame - frame
    ? { target: before, distance: frame - before.frame }
    : { target: after, distance: after.frame - frame };
}

/**
 * `frame` moved onto whatever is closest within `SNAP_PX` on screen: a target, or a line of the grid
 * (`spacing` frames apart; `null` leaves the grid out). On a tie a target wins, because it is
 * something the user can see and the grid is everywhere.
 */
export function snapPoint(
  frame: number,
  targets: readonly SnapTarget[],
  spacing: number | null,
  pxPerFrame: number,
): Snapped {
  const reach = SNAP_PX / pxPerFrame;
  const near = nearestTarget(frame, targets);
  const item = near && near.distance <= reach ? near : null;
  let line: number | null = null;
  if (spacing !== null && spacing > 0) {
    const nearest = Math.round(frame / spacing) * spacing;
    if (Math.abs(nearest - frame) <= reach) line = Math.round(nearest);
  }
  if (item && (line === null || item.distance <= Math.abs(line - frame))) {
    return { frame: item.target.frame, source: item.target.source };
  }
  return line === null ? { frame, source: null } : { frame: line, source: "grid" };
}

/**
 * A block that starts at `start` and lasts `length` frames, moved so that its start or its end lands
 * on a target or a grid line, whichever of the two is nearer. `guide` is the frame that landed, where
 * the line to show goes; it is `null` when the block did not move onto anything.
 */
export function snapMove(
  start: number,
  length: number,
  targets: readonly SnapTarget[],
  spacing: number | null,
  pxPerFrame: number,
): Snapped & { start: number; guide: number | null } {
  const head = snapPoint(start, targets, spacing, pxPerFrame);
  const tail = length > 0 ? snapPoint(start + length, targets, spacing, pxPerFrame) : { frame: start + length, source: null };
  const headShift = head.source === null ? Infinity : Math.abs(head.frame - start);
  const tailShift = tail.source === null ? Infinity : Math.abs(tail.frame - (start + length));
  if (headShift === Infinity && tailShift === Infinity) return { frame: start, start, source: null, guide: null };
  if (headShift <= tailShift) return { frame: head.frame, start: head.frame, source: head.source, guide: head.frame };
  const moved = tail.frame - length;
  return { frame: moved, start: moved, source: tail.source, guide: tail.frame };
}
