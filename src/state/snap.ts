// The magnet as the editor uses it: on or off, and onto the lines the ruler is drawing now, the
// edges of the clips and notes, the playhead and the origin.

import { collectTargets, fixedSpacing, snapMove, snapPoint } from "../timeline/snap";
import type { GridLines, SnapItem, SnapSource, SnapTarget } from "../timeline/snap";
import { lineSpacing, nearestBeatLine } from "../timeline/scale";
import { realFps } from "../timecode";
import { beatGrid } from "./tempoMap";
import { currentFrame } from "../transport/store";
import { activeTimeline } from "./doc";
import { trackItems } from "./selection";
import { useDoc } from "./store";
import { setSnapGuide, useUi } from "./ui";

/**
 * Frames between the lines of the grid the magnet snaps to, or `null` when the grid is left out.
 * Automatic follows the ruler: beats and bars in the bars view, timecode ticks in the time view, so
 * a finer zoom snaps finer. The other steps stay the same at every zoom.
 */
function gridSpacing(): GridLines | null {
  const { ruler, zoom, gridStep, snapSources } = useUi.getState();
  if (!snapSources.grid) return null;
  const { timecode } = activeTimeline(useDoc.getState());
  const beat = beatGrid();
  // Beats and bars follow the tempo map, so their lines are not a fixed step apart.
  const needsBeats = gridStep === "beat" || gridStep === "bar" || (gridStep === "auto" && ruler === "bars");
  if (needsBeats && !beat.segments.length) return null;
  if (gridStep === "beat" || gridStep === "bar") return (frame) => nearestBeatLine(frame, beat, zoom, gridStep);
  if (gridStep === "auto" && ruler === "bars") return (frame) => nearestBeatLine(frame, beat, zoom, "auto");
  return gridStep === "auto" ? lineSpacing(zoom, timecode.frameRate, null) : fixedSpacing(gridStep, realFps(timecode.frameRate));
}

/** Whether the magnet works for this gesture: it is on, and Alt, held, turns it the other way. */
const active = (alt: boolean) => useUi.getState().snap !== alt;

/** The clips, notes and events of the active timeline, as far as the magnet cares. */
const timelineItems = (): SnapItem[] => activeTimeline(useDoc.getState()).tracks.flatMap(trackItems);

/**
 * What a drag can land on besides the grid, taken once when the drag starts. `exclude` is the item
 * being moved, which must not snap onto itself; `skip` leaves a source out (the playhead, when it is
 * the playhead being moved).
 */
export function snapTargets(exclude: readonly string[] = [], skip: readonly SnapSource[] = []): SnapTarget[] {
  const { items, playhead, markers } = useUi.getState().snapSources;
  const { timecode, markers: marks = [], regions = [] } = activeTimeline(useDoc.getState());
  const sources = new Set<SnapSource>();
  if (items) sources.add("items").add("origin");
  if (playhead) sources.add("playhead");
  if (markers) sources.add("markers");
  for (const source of skip) sources.delete(source);
  // A marker or a region being dragged is left out, so it does not snap onto itself.
  const skipped = new Set(exclude);
  return collectTargets(timelineItems(), {
    exclude: skipped,
    playhead: currentFrame() - timecode.start,
    marks: [
      ...marks.filter((m) => !skipped.has(m.id)).map((m) => m.at),
      ...regions.filter((r) => !skipped.has(r.id)).flatMap((r) => [r.start, r.end]),
    ],
    sources,
  });
}

interface SnapOptions {
  /** Alt is held: the magnet works the other way for this gesture. */
  alt?: boolean;
  /** Show the guide line where the position lands. Off for a readout that only follows the pointer. */
  guide?: boolean;
}

/**
 * A position in frames from the timeline origin, moved onto the closest grid line or target when
 * the magnet is on and one is near. Without `targets` only the grid counts.
 */
export function snapFrame(frame: number, targets: readonly SnapTarget[] = [], options: SnapOptions = {}): number {
  if (!active(options.alt ?? false)) {
    if (options.guide) setSnapGuide(null);
    return frame;
  }
  const snapped = snapPoint(frame, targets, gridSpacing(), useUi.getState().zoom);
  if (options.guide) setSnapGuide(snapped.source === null ? null : { frame: snapped.frame, kind: snapped.source });
  return snapped.frame;
}

/** The start of a block of `length` frames, moved so that its start or its end lands on the grid or a target. */
export function snapBlockStart(
  start: number,
  length: number,
  targets: readonly SnapTarget[] = [],
  options: Pick<SnapOptions, "alt"> = {},
): number {
  if (!active(options.alt ?? false)) {
    setSnapGuide(null);
    return start;
  }
  const moved = snapMove(start, length, targets, gridSpacing(), useUi.getState().zoom);
  setSnapGuide(moved.guide === null || moved.source === null ? null : { frame: moved.guide, kind: moved.source });
  return moved.start;
}
