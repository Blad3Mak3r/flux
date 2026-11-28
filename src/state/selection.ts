// Picking several items at once, and finding what is picked. Which items are picked is window
// state; what is done to them (move, delete, ripple) is the backend's, asked for in `items.ts`.

import { create } from "zustand";
import { realFps, nominalFps } from "../timecode";
import { GUESS_SECONDS, clipLength, videoClipLength } from "../timeline/clip";
import type { AudioClip, DmxScene, MidiEvent, Note, OscEvent, Project, Track } from "../project/types";
import { useVoice } from "../voice/store";
import { activeTimeline } from "./doc";
import { useDoc } from "./store";
import { isSelected, select, selectMany, useUi } from "./ui";
import type { Selected } from "./ui";

export type Found =
  | { kind: "clip"; track: Track; clip: AudioClip }
  | { kind: "note"; track: Track; note: Note }
  | { kind: "midiEvent"; track: Track; event: MidiEvent }
  | { kind: "oscEvent"; track: Track; event: OscEvent }
  | { kind: "dmxScene"; track: Track; scene: DmxScene };

/** Finds a clip or a note by its id, which is unique in the whole project. */
export function findItem(project: Project, id: string): Found | null {
  for (const track of project.timelines.flatMap((t) => t.tracks)) {
    if (track.kind.type === "audio") {
      const clip = track.kind.clips.find((c) => c.id === id);
      if (clip) return { kind: "clip", track, clip };
    } else if (track.kind.type === "notes") {
      const note = track.kind.notes.find((n) => n.id === id);
      if (note) return { kind: "note", track, note };
    } else if (track.kind.type === "midi") {
      const event = track.kind.events.find((e) => e.id === id);
      if (event) return { kind: "midiEvent", track, event };
    } else if (track.kind.type === "osc") {
      const event = track.kind.events.find((e) => e.id === id);
      if (event) return { kind: "oscEvent", track, event };
    } else if (track.kind.type === "dmx") {
      const scene = track.kind.scenes.find((s) => s.id === id);
      if (scene) return { kind: "dmxScene", track, scene };
    }
  }
  return null;
}

/** An item of a lane as far as picking and the magnet care: where it starts and ends (`end` is absent for a point). */
export interface LaneItem extends Selected {
  start: number;
  end?: number;
}

/** The clips, notes, events and orders of a track. */
export function trackItems(track: Track): LaneItem[] {
  const doc = useDoc.getState();
  const { timecode } = activeTimeline(doc);
  const fps = realFps(timecode.frameRate);
  const guess = GUESS_SECONDS * nominalFps(timecode.frameRate);
  const kind = track.kind;
  switch (kind.type) {
    case "audio":
      return kind.clips.map((clip) => ({ kind: "clip", id: clip.id, start: clip.start, end: clip.start + clipLength(clip, doc.audio[clip.asset], fps, guess) }));
    case "video":
      return kind.clips.map((clip) => ({ kind: "video", id: clip.id, start: clip.start, end: clip.start + videoClipLength(clip, doc.video[clip.asset], fps, guess) }));
    case "notes":
      return kind.notes.map((note) => ({ kind: "note", id: note.id, start: note.at, end: note.length === undefined ? undefined : note.at + note.length }));
    case "operator":
      return kind.orders.map((order) => {
        const view = useVoice.getState().views[order.id];
        return { kind: "operator", id: order.id, start: view?.start ?? order.at, end: view?.start != null && view.length != null ? view.start + view.length : undefined };
      });
    case "dmx":
      return kind.scenes.map((scene) => ({ kind: "event", id: scene.id, start: scene.at, end: scene.fade ? scene.at + scene.fade : undefined }));
    default:
      return kind.events.map((event) => ({ kind: "event", id: event.id, start: event.at }));
  }
}

/** The ids of the items of a track: all `trackItems` needs, without measuring them. */
export function trackIds(track: Track): string[] {
  const kind = track.kind;
  const items = kind.type === "audio" || kind.type === "video" ? kind.clips : kind.type === "notes" ? kind.notes : kind.type === "operator" ? kind.orders : kind.type === "dmx" ? kind.scenes : kind.events;
  return items.map((item) => item.id);
}

/** What can be moved and deleted in bulk: everything on a lane but the orders of an operator. */
const pickable = (item: LaneItem) => item.kind !== "operator";

const allItems = () => activeTimeline(useDoc.getState()).tracks.flatMap(trackItems);

/** The ids of the picked items that are still in the timeline; empty when only one or none is picked. */
export function pickedIds(): string[] {
  const { picked } = useUi.getState();
  if (picked.size < 2) return [];
  return allItems().filter((item) => picked.has(item.id)).map((item) => item.id);
}

/** The ids an edit refers to: the picked ones, or the selected item alone (never a track or an order). */
export function selectedIds(): string[] {
  const many = pickedIds();
  if (many.length > 0) return many;
  const { selection } = useUi.getState();
  return selection && selection.kind !== "track" && selection.kind !== "operator" ? [selection.id] : [];
}

/** How many items are selected, for the inspector and the status. */
export const usePickedCount = () => useUi((ui) => ui.picked.size);

/** Ctrl/Cmd+click: adds the item to the selection, or takes it out when it is in. */
export function toggleItem(item: Selected): void {
  const { selection, picked } = useUi.getState();
  const items = allItems().filter(pickable);
  const current: Selected[] = items
    .filter((it) => (picked.size > 0 ? picked.has(it.id) : it.id === selection?.id))
    .map(({ kind, id }) => ({ kind, id }));
  selectMany(current.some((it) => it.id === item.id) ? current.filter((it) => it.id !== item.id) : [...current, item]);
}

/** Shift+click: every item of the track between the last one selected and this one, ends included. */
export function selectRange(track: Track, item: Selected): void {
  const { selection } = useUi.getState();
  const items = trackItems(track).filter(pickable);
  const anchor = items.find((it) => it.id === selection?.id);
  const target = items.find((it) => it.id === item.id);
  if (!anchor || !target) return select(item);
  const [from, to] = [Math.min(anchor.start, target.start), Math.max(anchor.start, target.start)];
  const range = items.filter((it) => it.start >= from && it.start <= to);
  // The clicked one goes last, so it stays the one the inspector shows.
  selectMany([...range.filter((it) => it.id !== item.id), target].map(({ kind, id }) => ({ kind, id })));
}

/** The items selected now, to keep while a box adds to them. */
export function selectedItems(): Selected[] {
  const ui = useUi.getState();
  return allItems().filter((item) => pickable(item) && isSelected(ui, item.id)).map(({ kind, id }) => ({ kind, id }));
}

/** Ctrl/Cmd+A: every item of the active timeline. */
export function selectAll(): void {
  selectMany(allItems().filter(pickable).map(({ kind, id }) => ({ kind, id })));
}

/**
 * Selects the items of the tracks `trackIds` that touch the frames `from` to `to`, besides those in `base`.
 * A point counts when it is inside; a clip when any of it is.
 */
export function pickInBox(from: number, to: number, trackIds: readonly string[], base: readonly Selected[]): void {
  const { tracks } = activeTimeline(useDoc.getState());
  const hit = tracks
    .filter((track) => trackIds.includes(track.id))
    .flatMap(trackItems)
    .filter(pickable)
    .filter((item) => item.start <= to && (item.end ?? item.start) >= from);
  selectMany([...base.filter((item) => !hit.some((h) => h.id === item.id)), ...hit.map(({ kind, id }) => ({ kind, id }))]);
}

/** The earliest start among the picked items, which is how far the group can go back. */
export function groupStart(ids: readonly string[]): number {
  const set = new Set(ids);
  return Math.min(...allItems().filter((item) => set.has(item.id)).map((item) => item.start));
}

/** How far the picked items are dragged right now, in frames, while a group is on the move. */
export const useGroupDrag = create<{ delta: number }>(() => ({ delta: 0 }));
export const setGroupDelta = (delta: number) => useGroupDrag.setState({ delta });
