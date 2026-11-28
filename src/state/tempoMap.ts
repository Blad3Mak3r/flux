// The tempo map as the window draws it. The backend derives the stretches (where each starts, how
// long a beat lasts, which bar it starts) from the timeline's tempo and its tempo changes; this
// only asks for them when those change and keeps the answer for the ruler, the magnet and the tempo editor.

import { create } from "zustand";
import { errorMessage, tempoSegments } from "../project/api";
import type { Timeline } from "../project/types";
import type { BeatGrid } from "../timeline/scale";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { sayError } from "./ui";

/** The stretches of the active timeline's tempo map, empty until the backend has answered. */
export const useBeatMap = create<BeatGrid>()(() => ({ segments: [] }));

/** The beat grid of the active timeline. */
export const beatGrid = (): BeatGrid => useBeatMap.getState();

/** What the stretches are made from: when any of it changes, the backend is asked again. */
const keyOf = (timeline: Timeline) =>
  JSON.stringify([timeline.id, timeline.tempo.bpm, timeline.tempo.beatsPerBar, timeline.tempo.beatUnit, timeline.timecode.frameRate, timeline.tempoChanges]);

/** Keeps the stretches of the active timeline up to date with the document. Call once from the root; call the result to stop. */
export function bindTempoMap(): () => void {
  let asked = 0;
  const refresh = () => {
    const doc = useDoc.getState();
    const timeline = activeTimeline(doc);
    const ticket = ++asked;
    tempoSegments(doc.project, timeline.id)
      .then((segments) => {
        if (ticket === asked) useBeatMap.setState({ segments });
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
