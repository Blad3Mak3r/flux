import type { Cue, LiveState } from "../live/api";
import type { FrameRate } from "../project/types";
import type { Position } from "../transport/api";

export type Transition = { type: "wait" } | { type: "continue" } | { type: "after"; milliseconds: number };
export interface SetlistEntry { id: string; timelineId: string; name: string; notes: string; omitted: boolean; transition: Transition }
export interface Setlist { id: string; name: string; entries: SetlistEntry[] }
export type TimelineControl = { id: string; name: string; at: number } & ({ type: "wait" } | { type: "loop"; end: number; turns: number | null });
export type ShowStatus = "ready" | "playing" | "paused" | "waiting" | "devamping" | "delay" | "completed" | "blocked";
export interface ShowState {
  setlistId: string; entryId: string; timelineId: string; name: string; notes: string; next: string | null;
  tick: { entry: number; status: ShowStatus; go: "start" | "resume" | "devamp" | "none"; turn: number; devampPending: boolean; control: number | null };
  position: Position; frameRate: FrameRate; cues: Cue[]; live: LiveState; start: number; durationFrames: number | null; durationText: string | null;
}
export type ShowEdit =
  | { op: "addSetlist"; name: string }
  | { op: "renameSetlist"; id: string; name: string }
  | { op: "removeSetlist"; id: string }
  | { op: "addEntry"; setlistId: string; timelineId: string }
  | { op: "setEntry"; setlistId: string; entry: SetlistEntry }
  | { op: "moveEntry"; setlistId: string; id: string; to: number }
  | { op: "removeEntry"; setlistId: string; id: string }
  | { op: "putControl"; timelineId: string; control: TimelineControl }
  | { op: "removeControl"; timelineId: string; id: string };
export type ShowAction = { type: "go" | "devamp" | "cancelDevamp" } | { type: "prepare"; entryId: string };
