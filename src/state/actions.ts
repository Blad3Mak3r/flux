// Edits to the open document, built from the latest project at the moment they run. Each is one
// stable function, so it can be handed to any component without a closure over a stale project.

import type { EnvPoint, Project, Tempo } from "../project/types";
import { activeTimeline } from "./doc";
import { dispatch, useDoc } from "./store";
import {
  findItem,
  moveItem,
  moveTrack,
  removeItem,
  removeTrack,
  renameClip,
  renameTrack,
  setTempo,
  setTrackGain,
  toggleMute,
  updateClip,
} from "./edit";
import { removeCueEvent } from "./cues";
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
export const nameTrack = (trackId: string, name: string) => editWith((p, tl) => renameTrack(p, tl, trackId, name));
/** Sets the volume envelope of a clip; no points take it away. One call is one step of undo. */
export const setClipEnvelope = (id: string, envelope: EnvPoint[]) =>
  editWith((p, tl) => updateClip(p, tl, id, { envelope: envelope.length > 0 ? envelope : undefined }));
export const nameClip = (id: string, name: string) => editWith((p, tl) => renameClip(p, tl, id, name));
export const setGain = (trackId: string, gainDb: number) =>
  editWith((p, tl) => setTrackGain(p, tl, trackId, gainDb));

/** Changes the tempo, beats per bar or click switch of the active timeline. */
export const setTimelineTempo = (patch: Partial<Tempo>) => editWith((p, tl) => setTempo(p, tl, patch));

export function deleteTrack(trackId: string): void {
  editWith((p, tl) => removeTrack(p, tl, trackId));
  select(null);
}

export function deleteSelection(): void {
  const { selection } = useUi.getState();
  if (!selection) return;
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
  const track = selection && findItem(useDoc.getState().project, selection.id)?.track;
  if (track) rename({ kind: "track", id: track.id });
}
