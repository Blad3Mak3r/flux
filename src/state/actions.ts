import { operatorEdit, selectedOperatorTrack } from "../voice/store";
// Edits to the open document, built from the latest project at the moment they run. Each is one
// stable function, so it can be handed to any component without a closure over a stale project.

import type { Project, Tempo } from "../project/types";
import { activeTimeline } from "./doc";
import { dispatch, useDoc } from "./store";
import {
  findItem,
  moveItem,
  moveTrack,
  removeItem,
  removeTrack,
  setTempo,
  setTrackGain,
  toggleMute,
} from "./edit";
import { removeCueEvent } from "./cues";
import { editItem } from "./items";
import { rename, select, useUi } from "./ui";

export const edit = (project: Project) => dispatch({ type: "edit", project });

/** Edits the project, given the id of the active timeline at the moment it runs. */
export const editWith = (change: (project: Project, timeline: string) => Project) => {
  const doc = useDoc.getState();
  edit(change(doc.project, activeTimeline(doc).id));
};

export const moveTo = (id: string, position: number) => editWith((p, tl) => moveItem(p, tl, id, position));
export const reorderTrack = (trackId: string, to: number) => editWith((p, tl) => moveTrack(p, tl, trackId, to));
export const muteTrack = (trackId: string) => editWith((p, tl) => toggleMute(p, tl, trackId));
/** Renames a track; the backend trims the name and refuses a blank one. */
export const nameTrack = (id: string, name: string) => editItem({ op: "renameTrack", id, name });
/** Names a clip; a blank name takes it away, so the clip goes by its audio file's name again. */
export const nameClip = (id: string, name: string) => editItem({ op: "renameClip", id, name });
export const setGain = (trackId: string, gainDb: number) => {
  if (activeTimeline(useDoc.getState()).tracks.some((t) => t.id === trackId && t.kind.type === "operator")) void operatorEdit({ op: "setGain", trackId, gainDb });
  else editWith((p, tl) => setTrackGain(p, tl, trackId, gainDb));
};

/** Changes the tempo, beats per bar or click switch of the active timeline. */
export const setTimelineTempo = (patch: Partial<Tempo>) => editWith((p, tl) => setTempo(p, tl, patch));

export function deleteTrack(trackId: string): void {
  editWith((p, tl) => removeTrack(p, tl, trackId));
  select(null);
}

export function deleteSelection(): void {
  const { selection } = useUi.getState();
  if (!selection) return;
  if (selection.kind === "track") { deleteTrack(selection.id); return; }
  if (selection.kind === "operator") { void operatorEdit({ op: "removeOrder", id: selection.id }); select(null); return; }
  if (selection.kind === "event") {
    void removeCueEvent(selection.id);
    select(null);
    return;
  }
  editWith((p, tl) => removeItem(p, tl, selection.id));
  select(null);
}

/** Opens the name of the track that holds the selected item for typing (F2). */
export function renameSelectedTrack(): void {
  const { selection } = useUi.getState();
  const track = selection?.kind === "track" ? activeTimeline(useDoc.getState()).tracks.find((t) => t.id === selection.id) : (selection && findItem(useDoc.getState().project, selection.id)?.track) || selectedOperatorTrack();
  if (track) rename({ kind: "track", id: track.id });
}
