// The magnet as the editor uses it: on or off, and onto the lines the ruler is drawing now, the
// edges of the clips and notes, the playhead and the origin.

import { nominalFps, realFps } from "../timecode";
import { GUESS_SECONDS, clipLength } from "../timeline/clip";
import { collectTargets, snapMove, snapPoint } from "../timeline/snap";
import type { SnapItem, SnapSource, SnapTarget } from "../timeline/snap";
import { lineSpacing } from "../timeline/scale";
import { currentFrame } from "../transport/store";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { useUi } from "./ui";

const SOURCES: ReadonlySet<SnapSource> = new Set(["grid", "items", "playhead", "origin"]);

/**
 * Frames between the lines of the grid on screen: beats and bars in the bars view, timecode ticks
 * in the time view, so a finer zoom snaps finer.
 */
function gridSpacing(): number {
  const { ruler, zoom } = useUi.getState();
  const { timecode, tempo } = activeTimeline(useDoc.getState());
  const grid = ruler === "bars" ? { bpm: tempo.bpm, beatsPerBar: tempo.beatsPerBar, fps: realFps(timecode.frameRate) } : null;
  return lineSpacing(zoom, timecode.frameRate, grid);
}

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
      default:
        return track.kind.events.map((event) => ({ id: event.id, start: event.at }));
    }
  });
}

/**
 * What a drag can land on besides the grid, taken once when the drag starts. `exclude` is the item
 * being moved, which must not snap onto itself; `skip` leaves a source out (the playhead, when it is
 * the playhead being moved). Empty while the magnet is off.
 */
export function snapTargets(exclude: readonly string[] = [], skip: readonly SnapSource[] = []): SnapTarget[] {
  if (!useUi.getState().snap) return [];
  const { timecode } = activeTimeline(useDoc.getState());
  return collectTargets(timelineItems(), {
    exclude: new Set(exclude),
    playhead: currentFrame() - timecode.start,
    sources: new Set([...SOURCES].filter((source) => !skip.includes(source))),
  });
}

/**
 * A position in frames from the timeline origin, moved onto the closest grid line or target when
 * the magnet is on and one is near. Without `targets` only the grid counts.
 */
export function snapFrame(frame: number, targets: readonly SnapTarget[] = []): number {
  if (!useUi.getState().snap) return frame;
  return snapPoint(frame, targets, gridSpacing(), useUi.getState().zoom).frame;
}

/** The start of a block of `length` frames, moved so that its start or its end lands on the grid or a target. */
export function snapBlockStart(start: number, length: number, targets: readonly SnapTarget[] = []): number {
  if (!useUi.getState().snap) return start;
  return snapMove(start, length, targets, gridSpacing(), useUi.getState().zoom).start;
}
