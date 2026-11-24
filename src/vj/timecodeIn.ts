// The external timecode the VJ clock follows. The backend opens the input the preferences choose
// and decodes it; this only asks where it is, a few times a frame.

import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { FrameRate } from "../project/types";

export type TimecodeInSource = "ltc" | "mtc";
export type TimecodeInState = "unset" | "missing" | "listening" | "error";

export interface TimecodeInStatus {
  state: TimecodeInState;
  /** What came in last: frames since midnight at its rate, and whether it runs. */
  reading: { frame: number; rate: FrameRate; running: boolean } | null;
}

/** How often to ask, often enough that no frame is skipped on the display. */
const POLL_MS = 30;

/** Where the external timecode of `source` is, kept up to date; `null` asks nothing. */
export function useTimecodeIn(source: TimecodeInSource | null): TimecodeInStatus | null {
  const [status, setStatus] = useState<TimecodeInStatus | null>(null);
  useEffect(() => {
    setStatus(null);
    if (source === null || !isTauri()) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<TimecodeInStatus>("timecode_in_status", { source });
        if (!disposed) setStatus(next);
      } catch {
        if (!disposed) setStatus({ state: "error", reading: null });
      }
      if (!disposed) timer = setTimeout(() => void poll(), POLL_MS);
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [source]);
  return status;
}

/** Closes the inputs, once nothing shows them. */
export const closeTimecodeIn = () => (isTauri() ? invoke("timecode_in_close").catch(() => undefined) : Promise.resolve());

export const listTimecodeMidiInputs = () => invoke<string[]>("timecode_midi_inputs");
