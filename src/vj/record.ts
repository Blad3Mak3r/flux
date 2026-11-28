// Recording the composition to a file: the backend (`src-tauri/src/vj_record.rs`) draws, compresses and writes it; this
// keeps what it last sent, and puts the project it returns on stop (the file added to the
// media) in the history.

import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { errorMessage } from "../project/api";
import type { Project } from "../project/types";
import { dispatch, useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { checkHap } from "../state/video";
import { watch } from "../watch";

/** Mirrors `RecordState` in src-tauri/src/vj_record.rs. */
export interface RecordState {
  recording: boolean;
  elapsedMs: number;
  frames: number;
  dropped: number;
  path: string | null;
  audio: boolean;
  error: string | null;
}

/** Mirrors `RecordStop`. */
interface RecordStop {
  state: { project: Project; active: string };
  path: string;
  frames: number;
  dropped: number;
}

const IDLE: RecordState = { recording: false, elapsedMs: 0, frames: 0, dropped: 0, path: null, audio: false, error: null };

export const useRecord = create<RecordState>(() => IDLE);

const fail = (err: unknown) => sayError(errorMessage(err));
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

let stopping = false;

/** Keeps the state up to date while the VJ screen is open; a recording that failed on its own is stopped, which says why. */
export function bindRecordState(): () => void {
  checkHap();
  return watch<RecordState>("vj_record_watch", (state) => {
    useRecord.setState(state);
    if (state.recording && state.error && !stopping) void stopRecording();
  });
}

export const startRecording = () => void invoke<RecordState>("vj_record_start").then((state) => useRecord.setState(state), fail);

export async function stopRecording(): Promise<void> {
  if (stopping) return;
  stopping = true;
  try {
    const saved = await invoke<RecordStop>("vj_record_stop", { project: useDoc.getState().project });
    useRecord.setState(IDLE);
    dispatch({ type: "apply", project: saved.state.project, active: saved.state.active });
    say("status.recordSaved", { name: fileName(saved.path) });
  } catch (err) {
    useRecord.setState(IDLE);
    fail(err);
  } finally {
    stopping = false;
  }
}
