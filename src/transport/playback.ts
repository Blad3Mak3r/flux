import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../project/api";
import type { Project } from "../project/types";
import { createStore } from "../store";
import * as api from "./api";
import type { OutputInfo, Position } from "./api";

/** Latest position from the backend, or `null` while no output runs. Many readers, 30 Hz. */
export const positionStore = createStore<Position | null>(null);

/** What the output was opened for; when it changes, the next Play reopens the devices. */
function signature(project: Project): string {
  return JSON.stringify([project.timecode, project.output]);
}

export interface Playback {
  info: OutputInfo | null;
  /** Frame count to show when no output runs (absolute, including the start offset). */
  parked: number;
  play: () => Promise<void>;
  pause: () => Promise<void>;
  stop: () => Promise<void>;
  toggle: () => Promise<void>;
  seek: (frame: number) => void;
  startOutput: () => Promise<void>;
  stopOutput: () => Promise<void>;
}

/**
 * Drives the backend output for the open project. The UI works without it: with no output running
 * the playhead is parked and seeking only moves it, and the first Play opens the devices.
 */
export function usePlayback(project: Project, report: (error: string) => void): Playback {
  const [info, setInfo] = useState<OutputInfo | null>(null);
  const [parked, setParked] = useState(project.timecode.start);
  const openedFor = useRef<string | null>(null);
  const projectRef = useRef(project);
  projectRef.current = project;
  const parkedRef = useRef(parked);
  parkedRef.current = parked;
  const reportRef = useRef(report);
  reportRef.current = report;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    api
      .onPosition((position) => positionStore.set(position))
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      // Outside the desktop app there is no event bus; the UI still works parked.
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // A different project has its own start: park there.
  const start = project.timecode.start;
  useEffect(() => setParked(start), [start]);

  const guard = useCallback(async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (err) {
      reportRef.current(errorMessage(err));
    }
  }, []);

  const open = useCallback(async () => {
    const current = projectRef.current;
    if (info && openedFor.current === signature(current)) return;
    const opened = await api.startOutput(current);
    openedFor.current = signature(current);
    setInfo(opened);
    positionStore.set(null);
    await api.seek(parkedRef.current);
  }, [info]);

  const startOutput = useCallback(() => guard(open), [guard, open]);

  const stopOutput = useCallback(
    () =>
      guard(async () => {
        const last = positionStore.get();
        if (last) setParked(last.frame);
        await api.stopOutput();
        openedFor.current = null;
        setInfo(null);
        positionStore.set(null);
      }),
    [guard],
  );

  const play = useCallback(
    () =>
      guard(async () => {
        await open();
        await api.play();
      }),
    [guard, open],
  );

  const pause = useCallback(() => guard(api.pause), [guard]);

  const stop = useCallback(
    () =>
      guard(async () => {
        setParked(projectRef.current.timecode.start);
        if (info) await api.stop();
      }),
    [guard, info],
  );

  const toggle = useCallback(async () => {
    const state = positionStore.get()?.state;
    await (state === "playing" ? pause() : play());
  }, [pause, play]);

  const seek = useCallback(
    (frame: number) => {
      const target = Math.max(projectRef.current.timecode.start, Math.round(frame));
      setParked(target);
      const last = positionStore.get();
      if (last) positionStore.set({ ...last, frame: target });
      if (info) void guard(() => api.seek(target));
    },
    [guard, info],
  );

  return { info, parked, play, pause, stop, toggle, seek, startOutput, stopOutput };
}
