import { useVoice } from "../voice/store";
// The magnet as the editor uses it: on or off, and onto the lines the ruler is drawing now, the
// edges of the clips and notes, the playhead and the origin.

import { nominalFps, realFps } from "../timecode";
import { GUESS_SECONDS, clipLength } from "../timeline/clip";
import { collectTargets, fixedSpacing, snapMove, snapPoint } from "../timeline/snap";
import type { SnapItem, SnapSource, SnapTarget } from "../timeline/snap";
import { lineSpacing } from "../timeline/scale";
import { currentFrame } from "../transport/store";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { setSnapGuide, useUi } from "./ui";

/**
 * Frames between the lines of the grid the magnet snaps to, or `null` when the grid is left out.
 * Automatic follows the ruler: beats and bars in the bars view, timecode ticks in the time view, so
 * a finer zoom snaps finer. The other steps stay the same at every zoom.
 */
function gridSpacing(): number | null {
  const { ruler, zoom, gridStep, snapSources } = useUi.getState();
  if (!snapSources.grid) return null;
  const { timecode, tempo } = activeTimeline(useDoc.getState());
  const fps = realFps(timecode.frameRate);
  const beat = { bpm: tempo.bpm, beatsPerBar: tempo.beatsPerBar, fps };
  return gridStep === "auto"
    ? lineSpacing(zoom, timecode.frameRate, ruler === "bars" ? beat : null)
    : fixedSpacing(gridStep, beat);
}

/** Whether the magnet works for this gesture: it is on, and Alt, held, turns it the other way. */
const active = (alt: boolean) => useUi.getState().snap !== alt;

/** The clips, notes and events of the active timeline, as far as the magnet cares. */
function timelineItems(): SnapItem[] {
  const doc = useDoc.getState();
  const { tracks, timecode } = activeTimeline(doc);
  const fps = realFps(timecode.frameRate);
  const guess = GUESS_SECONDS * nominalFps(timecode.frameRate);
  return tracks.flatMap((track): SnapItem[] => {
    switch (track.kind.type) {
      case "audio":
        return track.kind.clips.map((clip) => ({
          id: clip.id,
          start: clip.start,
          end: clip.start + clipLength(clip, doc.audio[clip.asset], fps, guess),
        }));
      case "notes":
        return track.kind.notes.map((note) => ({
          id: note.id,
          start: note.at,
          end: note.length === undefined ? undefined : note.at + note.length,
        }));
      case "operator":
        return track.kind.orders.map((order) => {
          const view = useVoice.getState().views[order.id];
          return { id: order.id, start: view?.start ?? order.at, end: view?.start != null && view.length != null ? view.start + view.length : undefined };
        });
      default:
        return track.kind.events.map((event) => ({ id: event.id, start: event.at }));
    }
  });
}

/**
 * What a drag can land on besides the grid, taken once when the drag starts. `exclude` is the item
 * being moved, which must not snap onto itself; `skip` leaves a source out (the playhead, when it is
 * the playhead being moved).
 */
export function snapTargets(exclude: readonly string[] = [], skip: readonly SnapSource[] = []): SnapTarget[] {
  const { items, playhead } = useUi.getState().snapSources;
  const { timecode } = activeTimeline(useDoc.getState());
  const sources = new Set<SnapSource>();
  if (items) sources.add("items").add("origin");
  if (playhead) sources.add("playhead");
  for (const source of skip) sources.delete(source);
  return collectTargets(timelineItems(), {
    exclude: new Set(exclude),
    playhead: currentFrame() - timecode.start,
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
