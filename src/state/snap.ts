// The magnet as the editor uses it: on or off, and onto the lines the ruler is drawing now.

import { realFps } from "../timecode";
import { snapToSpacing } from "../timeline/snap";
import { lineSpacing } from "../timeline/scale";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { useUi } from "./ui";

/**
 * A position in frames from the timeline origin, moved onto the closest vertical line when the
 * magnet is on and one is near. The lines are the ones on screen: beats and bars in the bars view,
 * timecode ticks in the time view, so a finer zoom snaps finer.
 */
export function snapFrame(frame: number): number {
  const { snap, ruler, zoom } = useUi.getState();
  if (!snap) return frame;
  const { timecode, tempo } = activeTimeline(useDoc.getState());
  const grid = ruler === "bars" ? { bpm: tempo.bpm, beatsPerBar: tempo.beatsPerBar, fps: realFps(timecode.frameRate) } : null;
  return snapToSpacing(frame, lineSpacing(zoom, timecode.frameRate, grid), zoom);
}
