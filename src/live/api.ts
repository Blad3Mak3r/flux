import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../project/types";

// Mirrors src-tauri/src/engine/cues.rs and src-tauri/src/live_commands.rs. The backend derives the
// cues and says where the show stands; nothing here orders, compares or formats them.

export type CueKind = "audio" | "marker" | "midi";

export type CueStatus = "pending" | "next" | "running" | "done" | "muted";

export interface Cue {
  /** Id of the clip or trigger. */
  id: string;
  /** Place in the show, from 1. */
  number: number;
  kind: CueKind;
  /** The audio file's name or the trigger's label; a trigger can have none. */
  name: string;
  trackId: string;
  trackName: string;
  /** `#rrggbb`, or null for the default colour of its kind. */
  color: string | null;
  muted: boolean;
  /** Timecode frame to seek to, including the project's start offset. */
  frame: number;
  timecode: string;
  /** Frames the cue lasts; only for a clip whose audio could be read. */
  length: number | null;
  duration: string | null;
}

export interface Countdown {
  cueId: string;
  remaining: number;
  text: string;
}

export interface RunningCue {
  cueId: string;
  elapsed: string;
  remaining: string;
  /** 0 to 1. */
  progress: number;
}

export interface LiveState {
  /** The status of every cue, in the order of the list. */
  statuses: CueStatus[];
  /** The cue the show is at. */
  current: string | null;
  next: Countdown | null;
  running: RunningCue[];
}

export function liveCues(project: Project): Promise<Cue[]> {
  return invoke<Cue[]>("live_cues", { project });
}

/** The state of the show with the playhead at `frame`, for when no output runs to report it. */
export function liveState(project: Project, frame: number): Promise<LiveState> {
  return invoke<LiveState>("live_state", { project, frame });
}
