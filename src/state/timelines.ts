// What the timeline strip and Preferences ask for. The backend does the work: it owns which
// timelines exist, which one is active and its timecode, and refuses what cannot be done (changing
// them while the show plays). It answers with the project as it is now, and this only shows it.

import type { Translate } from "../i18n/messages";
import {
  errorMessage,
  timelineAdd,
  timelineRemove,
  timelineRename,
  timelineSelect,
  timelineSetTimecode,
} from "../project/api";
import type { FrameRate, Project, ProjectState } from "../project/types";
import { isPlaying } from "../transport/store";
import { reconnect } from "../transport/playback";
import { activeTimeline } from "./doc";
import { dispatch, useDoc } from "./store";
import { openSetup, resetView, say, sayError } from "./ui";

/**
 * Asks the backend for a change and shows its answer. A change of the active timeline or of its
 * timecode (`retimes`) closes the output in the backend, so it is opened again for the new one.
 */
async function change(ask: (project: Project) => Promise<ProjectState>, retimes: boolean): Promise<void> {
  if (retimes && isPlaying()) return say("timeline.lockedPlaying");
  const before = activeTimeline(useDoc.getState()).id;
  try {
    const { project, active } = await ask(useDoc.getState().project);
    dispatch({ type: "apply", project, active });
    if (active !== before) resetView();
    if (retimes) reconnect(true);
  } catch (error) {
    sayError(errorMessage(error));
  }
}

export const addTimeline = (t: Translate) => change((p) => timelineAdd(p, t("timeline.default")), true);

export const selectTimeline = (id: string) => {
  if (id !== activeTimeline(useDoc.getState()).id) return change((p) => timelineSelect(p, id), true);
};

export const removeTimeline = (id: string) => change((p) => timelineRemove(p, id), true);

export const renameTimeline = (id: string, name: string) => change((p) => timelineRename(p, id, name), false);

export const setTimecode = (frameRate: FrameRate, start: number) =>
  change((p) => timelineSetTimecode(p, frameRate, start), true);

/** Makes the timeline the active one and opens its settings, where its name and timecode are edited. */
export async function openTimelineSettings(id: string): Promise<void> {
  await selectTimeline(id);
  if (activeTimeline(useDoc.getState()).id === id) openSetup("timeline");
}

/** The next timeline (or the previous one, for a negative `step`), wrapping round. */
export function stepTimeline(step: 1 | -1) {
  const doc = useDoc.getState();
  const { timelines } = doc.project;
  const at = timelines.findIndex((timeline) => timeline.id === activeTimeline(doc).id);
  return selectTimeline(timelines[(at + step + timelines.length) % timelines.length].id);
}
