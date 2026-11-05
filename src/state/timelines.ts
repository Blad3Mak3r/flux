// What the timeline strip and Preferences ask for. The backend does the work: it owns which
// timelines exist, which one is active and its timecode, and refuses what cannot be done (changing
// them while the show plays). It answers with the project as it is now, and this only shows it.

import { listen } from "@tauri-apps/api/event";
import type { Translate } from "../i18n/messages";
import {
  errorMessage,
  timelineAdd,
  timelineMove,
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
 * Shows the project as the backend answered. A change of the active timeline or of its timecode
 * (`reopens`) leaves the output open for the old one, so it is opened again for the new one.
 */
function show({ project, active }: ProjectState, reopens: boolean): void {
  const before = activeTimeline(useDoc.getState()).id;
  dispatch({ type: "apply", project, active });
  if (active !== before) resetView();
  if (reopens) reconnect(true);
}

/** What a change needs: the show stopped (the backend refuses otherwise), and the output reopened. */
interface Kind {
  locks: boolean;
  reopens: boolean;
}

/** The active timeline or its timecode changes: the output is reopened for it. */
const RETIME: Kind = { locks: true, reopens: true };
/** The order changes: the output stays, but not while the show plays. */
const REORDER: Kind = { locks: true, reopens: false };
/** A name changes: it can happen at any time. */
const RENAME: Kind = { locks: false, reopens: false };

/** Asks the backend for a change and shows its answer. */
async function change(ask: (project: Project) => Promise<ProjectState>, kind: Kind): Promise<void> {
  if (kind.locks && isPlaying()) return say("timeline.lockedPlaying");
  try {
    show(await ask(useDoc.getState().project), kind.reopens);
  } catch (error) {
    sayError(errorMessage(error));
  }
}

export const addTimeline = (t: Translate) => change((p) => timelineAdd(p, t("timeline.default")), RETIME);

export const selectTimeline = (id: string) => {
  if (id !== activeTimeline(useDoc.getState()).id) return change((p) => timelineSelect(p, id), RETIME);
};

export const removeTimeline = (id: string) => change((p) => timelineRemove(p, id), RETIME);

export const renameTimeline = (id: string, name: string) => change((p) => timelineRename(p, id, name), RENAME);

/** Reordering is locked while the show plays, like changing the timeline or the view. */
export const moveTimeline = (id: string, to: number) => change((p) => timelineMove(p, id, to), REORDER);

/** Moves a timeline one place left (`-1`) or right (`1`). */
export function nudgeTimeline(id: string, step: 1 | -1) {
  const index = useDoc.getState().project.timelines.findIndex((timeline) => timeline.id === id);
  if (index !== -1) return moveTimeline(id, index + step);
}

export const setTimecode = (frameRate: FrameRate, start: number) =>
  change((p) => timelineSetTimecode(p, frameRate, start), RETIME);

/** Makes the timeline the active one and opens its settings, where its name and timecode are edited. */
export async function openTimelineSettings(id: string): Promise<void> {
  await selectTimeline(id);
  if (activeTimeline(useDoc.getState()).id === id) openSetup("timeline");
}

/**
 * In Live, when the show runs to its end the backend makes the next timeline the active one,
 * stopped at its start, and says so with this event. Call once from the root; call the result to
 * stop. Outside the desktop app there is no backend to hear from.
 */
export function bindTimelineAdvance(): () => void {
  const listening = listen<ProjectState>("timeline-advanced", ({ payload }) => {
    show(payload, true);
    const name = payload.project.timelines.find((timeline) => timeline.id === payload.active)?.name ?? "";
    say("timeline.advanced", { name });
  }).catch(() => undefined);
  return () => void listening.then((unlisten) => unlisten?.());
}

/** The next timeline (or the previous one, for a negative `step`), wrapping round. */
export function stepTimeline(step: 1 | -1) {
  const doc = useDoc.getState();
  const { timelines } = doc.project;
  const at = timelines.findIndex((timeline) => timeline.id === activeTimeline(doc).id);
  return selectTimeline(timelines[(at + step + timelines.length) % timelines.length].id);
}
