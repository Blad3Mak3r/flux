// What the marker lane asks for. The backend does the work: it numbers and trims names, keeps the
// markers, the regions and the tempo changes in order, puts a tempo change on a bar and finds the next
// marker. It answers with the project as it is now, and this only shows it. Each answer is one step of undo.

import { create } from "zustand";
import type { Translate } from "../i18n/messages";
import { errorMessage, itemsExtent, marksEdit, markerStep } from "../project/api";
import type { MarksOp } from "../project/api";
import { currentFrame } from "../transport/store";
import { seek } from "../transport/playback";
import { activeTimeline } from "./doc";
import { selectedIds } from "./selection";
import { beatGrid } from "./tempoMap";
import { dispatch, useDoc } from "./store";
import { say, sayError, useUi } from "./ui";

/** What is picked in the marker lane, and what is being renamed there. */
export interface MarkRef {
  kind: "marker" | "region" | "tempo";
  id: string;
}

export const useMarks = create<{ selected: MarkRef | null; renaming: MarkRef | null }>()(() => ({
  selected: null,
  renaming: null,
}));

export const selectMark = (selected: MarkRef | null) => useMarks.setState({ selected, renaming: null });
export const renameMark = (renaming: MarkRef | null) => useMarks.setState({ renaming });

// Picking something elsewhere lets go of the mark.
useUi.subscribe((ui, before) => {
  if (ui.selection !== before.selection && ui.selection !== null && useMarks.getState().selected) selectMark(null);
});

/** Asks for `op`. Resolves when the answer is shown; a refusal is said on the status line. */
export async function editMarks(op: MarksOp): Promise<void> {
  try {
    const { project, active, changed } = await marksEdit(useDoc.getState().project, op);
    if (changed) dispatch({ type: "apply", project, active });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

const origin = () => activeTimeline(useDoc.getState()).timecode.start;

/** The playhead, in frames from the timeline origin. */
const playhead = () => Math.max(0, currentFrame() - origin());

export const addMarkerAtPlayhead = (t: Translate) => editMarks({ op: "addMarker", at: playhead(), label: t("marks.marker.label") });

/** Goes to the marker before or after the playhead; says so when there is none that way. */
export async function stepMarker(forward: boolean): Promise<void> {
  const { project } = useDoc.getState();
  try {
    const at = await markerStep(project, activeTimeline(useDoc.getState()).id, playhead(), forward);
    if (at === null) say(forward ? "marks.step.none.next" : "marks.step.none.previous");
    else seek(origin() + at);
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** A region over the selected items, from where the first starts to where the last ends. */
export async function addRegionFromSelection(t: Translate): Promise<void> {
  const ids = selectedIds();
  if (ids.length === 0) return say("marks.region.needSelection");
  try {
    const extent = await itemsExtent(useDoc.getState().project, ids);
    if (!extent || extent.to <= extent.from) return say("marks.region.needSelection");
    await editMarks({ op: "addRegion", start: extent.from, end: extent.to, label: t("marks.region.label") });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Deletes the picked marker, region or tempo change. Says whether there was one. */
export function deleteSelectedMark(): boolean {
  const { selected } = useMarks.getState();
  if (!selected) return false;
  selectMark(null);
  const op: MarksOp =
    selected.kind === "marker"
      ? { op: "removeMarker", id: selected.id }
      : selected.kind === "region"
        ? { op: "removeRegion", id: selected.id }
        : { op: "removeTempoChange", id: selected.id };
  void editMarks(op);
  return true;
}

/**
 * Puts a tempo change at the bar closest to `at` (the playhead when none is given), with the tempo and
 * signature that are there now, for the editor to change.
 */
export function addTempoChangeAt(at: number = playhead()): Promise<void> {
  const { segments } = beatGrid();
  if (!segments.length) return Promise.resolve();
  const here = [...segments].reverse().find((segment) => segment.startFrame <= at) ?? segments[0];
  return editMarks({ op: "setTempoAt", at, bpm: here.bpm, beatsPerBar: here.beatsPerBar, beatUnit: here.beatUnit });
}
