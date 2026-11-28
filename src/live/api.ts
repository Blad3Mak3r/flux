import { invoke } from "@tauri-apps/api/core";
import type { Project } from "../project/types";

// Mirrors src-tauri/crates/flux-engine/src/cues.rs and src-tauri/src/live_commands.rs. The backend derives the
// cues and says where the show stands; nothing here orders, compares or formats them.

export type CueKind = "audio" | "note" | "midi" | "osc" | "operator" | "video";

/** What a MIDI or OSC cue sends; `Cue.detail` holds its notation. */
export type CueAction = "noteOn" | "noteOff" | "controlChange" | "programChange" | "msc" | "sysEx" | "osc";

export type CueStatus = "pending" | "next" | "running" | "done" | "muted";

export interface Cue {
  /** Id of the clip or note. */
  id: string;
  /** Place in the show, from 1. */
  number: number;
  kind: CueKind;
  /** The audio file's name or the first line of a note; a note can have none. */
  name: string;
  /** The whole text of a note. */
  text: string | null;
  /** What a MIDI or OSC cue sends. */
  action: CueAction | null;
  /** Its notation, e.g. `C4 · ch 1 · vel 100` or `/cue/1/start 1`. */
  detail: string | null;
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

/** A note that is on show now. */
export interface NoteNow {
  cueId: string;
  text: string;
  /** Time left of a note that has a length. */
  remaining: string | null;
}

export interface LiveState {
  /** The status of every cue, in the order of the list. */
  statuses: CueStatus[];
  /** The cue the show is at. */
  current: string | null;
  next: Countdown | null;
  running: RunningCue[];
  /** The notes on show now, in the order of the list. */
  notes: NoteNow[];
}

export function liveCues(project: Project): Promise<Cue[]> {
  return invoke<Cue[]>("live_cues", { project });
}

/** The state of the show with the playhead at `frame`, for when no output runs to report it. */
export function liveState(project: Project, frame: number): Promise<LiveState> {
  return invoke<LiveState>("live_state", { project, frame });
}

/** The cues of the active timeline that match what was typed, best first; the backend decides which. */
export function liveSearch(project: Project, query: string): Promise<string[]> {
  return invoke<string[]>("live_search", { project, query });
}

/** Mirrors `PanicReport` in src-tauri/crates/flux-engine/src/midi_panic.rs. */
export interface PanicReport {
  /** Ports that took every message. */
  sent: number;
  /** Of `sent`, the ports a running output holds: its own thread sent to them. */
  viaOutput: number;
  /** Names of the ports that could not be reached. */
  failed: string[];
}

/** Silences the MIDI gear of the project (all sound off, reset controllers, all notes off on 16 channels). */
export function midiPanic(project: Project): Promise<PanicReport> {
  return invoke<PanicReport>("midi_panic", { project });
}

/** Whether the operator has locked the show: the backend then refuses to change, open or close the project. */
export function showLockGet(): Promise<boolean> {
  return invoke<boolean>("show_lock_get");
}

export function showLockSet(locked: boolean): Promise<boolean> {
  return invoke<boolean>("show_lock_set", { locked });
}
