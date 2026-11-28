import { operatorEdit, selectedOperatorTrack } from "../voice/store";
// Edits to the open document. Each only forwards the intent to the backend, which applies it and
// answers with the project; each is one stable function, so it can be handed to any component.

import { editVideoClip } from "./clips";
import type { Tempo } from "../project/types";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { removeCueEvent } from "./cues";
import { editItem, removeItems } from "./items";
import { editMarks } from "./marks";
import { findItem, selectedIds } from "./selection";
import { rename, select, useUi } from "./ui";

export const reorderTrack = (trackId: string, to: number) => void editItem({ op: "moveTrack", id: trackId, to });
export const muteTrack = (trackId: string) => void editItem({ op: "toggleMute", id: trackId });
/** Renames a track; the backend trims the name and refuses a blank one. */
export const nameTrack = (id: string, name: string) => editItem({ op: "renameTrack", id, name });
/** Names a clip; a blank name takes it away, so the clip goes by its audio file's name again. */
export const nameClip = (id: string, name: string) => editItem({ op: "renameClip", id, name });
export const setGain = (trackId: string, gainDb: number) => {
  if (activeTimeline(useDoc.getState()).tracks.some((t) => t.id === trackId && t.kind.type === "operator")) void operatorEdit({ op: "setGain", trackId, gainDb });
  else void editItem({ op: "setTrackGain", id: trackId, gainDb });
};

/** Changes the tempo, beats per bar or click switch of the active timeline. */
export const setTimelineTempo = ({ bpm, beatsPerBar, click }: Partial<Tempo>) =>
  void editMarks({ op: "setTempo", bpm, beatsPerBar, click });

export function deleteTrack(trackId: string): void {
  void editItem({ op: "removeTrack", id: trackId });
  select(null);
}

export function deleteSelection(): void {
  const { selection } = useUi.getState();
  if (!selection) return;
  if (selection.kind === "track") { deleteTrack(selection.id); return; }
  if (selection.kind === "operator") { void operatorEdit({ op: "removeOrder", id: selection.id }); select(null); return; }
  // Several items, or one with ripple on, are removed by the backend in a single step.
  const ids = selectedIds();
  if (ids.length > 1 || (ids.length === 1 && useUi.getState().ripple !== "off")) {
    void removeItems(ids);
    select(null);
    return;
  }
  if (selection.kind === "event") {
    void removeCueEvent(selection.id);
    select(null);
    return;
  }
  if (selection.kind === "video") {
    void editVideoClip(selection.id, "remove", 0);
    select(null);
    return;
  }
  void removeItems([selection.id]);
  select(null);
}

/** Opens the name of the track that holds the selected item for typing (F2). */
export function renameSelectedTrack(): void {
  const { selection } = useUi.getState();
  const track = selection?.kind === "track" ? activeTimeline(useDoc.getState()).tracks.find((t) => t.id === selection.id) : (selection && findItem(useDoc.getState().project, selection.id)?.track) || selectedOperatorTrack();
  if (track) rename({ kind: "track", id: track.id });
}
