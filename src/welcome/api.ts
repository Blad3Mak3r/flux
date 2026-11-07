import { invoke } from "@tauri-apps/api/core";
import type { FrameRate } from "../project/types";

// Mirrors src-tauri/src/recents.rs, startup.rs, windows.rs and the `project_delete` command.

export interface RecentShow {
  path: string;
  /** Seconds since the Unix epoch. */
  openedAt: number;
  /** `false` when the file was moved or deleted outside Flux. */
  exists: boolean;
  /** `null` when the file is missing or is not a readable show. */
  meta: { name: string; frameRate: FrameRate; trackCount: number } | null;
}

export type StepId = "audio" | "midi" | "ltc" | "mtc";
export type StepState = "running" | "ready" | "missing" | "fault";

export interface Step {
  id: StepId;
  state: StepState;
  count: number | null;
  message: string | null;
}

export const recentsList = () => invoke<RecentShow[]>("recents_list");
export const recentsForget = (path: string) => invoke<void>("recents_forget", { path });
/** Permanently deletes the file. Callers must have asked the user first. */
export const projectDelete = (path: string) => invoke<void>("project_delete", { path });

export const startupRun = () => invoke<Step[]>("startup_run");
export const showWelcome = () => invoke<void>("window_show_welcome");
export const closeEditor = () => invoke<void>("window_close_editor");
/** Opens the editor on `path`, or on a new show. */
export const openEditor = (path: string | null) => invoke<void>("window_open_editor", { path });

export interface RecoverySummary {
  id: string;
  name: string | null;
  path: string | null;
  savedAt: string | null;
  readable: boolean;
}

export const recoveryList = () => invoke<RecoverySummary[]>("recovery_list");
export const recoveryDiscard = (id: string) => invoke<void>("recovery_discard", { id });
export const recoveryRestore = (id: string) => invoke<void>("window_restore_recovery", { id });
export const recoveryOriginal = (id: string) => invoke<void>("window_open_original", { id });
export const openPending = () => invoke<void>("window_open_editor", { path: null, usePending: true });
export const pendingPath = () => invoke<string | null>("window_pending_path");
