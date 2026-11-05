// Edits to the open document, built from the latest project at the moment they run. Each is one
// stable function, so it can be handed to any component without a closure over a stale project.

import type { Project } from "../project/types";
import { dispatch, useDoc } from "./store";
import { moveItem, removeItem, removeTrack, setTrackGain, toggleMute } from "./edit";
import { select, useUi } from "./ui";

export const edit = (project: Project) => dispatch({ type: "edit", project });

const editWith = (change: (project: Project) => Project) => edit(change(useDoc.getState().project));

export const moveTo = (id: string, position: number) => editWith((p) => moveItem(p, id, position));
export const muteTrack = (trackId: string) => editWith((p) => toggleMute(p, trackId));
export const setGain = (trackId: string, gainDb: number) => editWith((p) => setTrackGain(p, trackId, gainDb));

export function deleteTrack(trackId: string): void {
  editWith((p) => removeTrack(p, trackId));
  select(null);
}

export function deleteSelection(): void {
  const { selection } = useUi.getState();
  if (!selection) return;
  editWith((p) => removeItem(p, selection.id));
  select(null);
}
