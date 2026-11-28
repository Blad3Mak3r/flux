// The tempo map as the window draws it. The backend derives the stretches (where each starts, how
// long a beat lasts, which bar it starts) from the timeline's tempo and its tempo changes; this
// only asks for them when those change and keeps the answer for the ruler and the magnet.

import { create } from "zustand";
import { errorMessage, tempoSegments } from "../project/api";
import type { BeatSegment, Timeline } from "../project/types";
import { realFps } from "../timecode";
import type { BeatGrid } from "../timeline/scale";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { sayError } from "./ui";

/** The stretches after the first, for the active timeline; empty while it has no tempo changes. */
export const useBeatMap = create<{ later: BeatSegment[] }>()(() => ({ later: [] }));

const EMPTY: BeatSegment[] = [];

/** The beat grid of a timeline, with its tempo changes when it has them. */
export function beatGrid(timeline: Timeline, later: readonly BeatSegment[] = useBeatMap.getState().later): BeatGrid {
  const { bpm, beatsPerBar, beatUnit } = timeline.tempo;
  return {
    bpm,
    beatsPerBar,
    beatUnit,
    fps: realFps(timeline.timecode.frameRate),
    later: timeline.tempoChanges?.length ? later : undefined,
  };
}

/** What the stretches are made from: when any of it changes, the backend is asked again. */
const keyOf = (timeline: Timeline) =>
  JSON.stringify([timeline.id, timeline.tempo.bpm, timeline.tempo.beatsPerBar, timeline.tempo.beatUnit, timeline.timecode.frameRate, timeline.tempoChanges]);

/** Keeps the stretches of the active timeline up to date with the document. Call once from the root; call the result to stop. */
export function bindTempoMap(): () => void {
  let asked = 0;
  const refresh = () => {
    const doc = useDoc.getState();
    const timeline = activeTimeline(doc);
    if (!timeline.tempoChanges?.length) {
      asked++;
      if (useBeatMap.getState().later.length) useBeatMap.setState({ later: EMPTY });
      return;
    }
    const ticket = ++asked;
    tempoSegments(doc.project, timeline.id)
      .then((segments) => {
        if (ticket === asked) useBeatMap.setState({ later: segments.slice(1) });
      })
      .catch((error) => sayError(errorMessage(error)));
  };
  let key = keyOf(activeTimeline(useDoc.getState()));
  refresh();
  return useDoc.subscribe((doc) => {
    const next = keyOf(activeTimeline(doc));
    if (next === key) return;
    key = next;
    refresh();
  });
}
