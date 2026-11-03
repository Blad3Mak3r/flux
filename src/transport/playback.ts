import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../project/api";
import type { Project } from "../project/types";
import { createStore } from "../store";
import * as api from "./api";
import type { OutputInfo, Position } from "./api";
import { stoppedAt } from "./stopped";

/** Latest position from the backend, or `null` while no output runs. Many readers, 30 Hz. */
export const positionStore = createStore<Position | null>(null);

/** Peak of each audio track in the latest `transport-levels` event, linear, by track id. */
export const levelsStore = createStore<Record<string, number>>({});

/** Moves a track's fader on the running output while it is dragged. No output is not an error. */
export function previewTrackGain(trackId: string, gainDb: number): void {
  api.setTrackGain(trackId, gainDb).catch(() => undefined);
}

/** Edits arrive in bursts (dragging a clip); the audio is rebuilt once they pause. */
const CLIPS_DEBOUNCE_MS = 200;

/** What the output was opened for; when it changes, the next Play reopens the devices. */
function signature(project: Project): string {
  return JSON.stringify([project.timecode, project.output]);
}

export interface Playback {
  info: OutputInfo | null;
  /** Files the output could not use (missing or unreadable audio), from starting or from edits. */
  clipWarnings: string[];
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
  const [clipWarnings, setClipWarnings] = useState<string[]>([]);
  const [parked, setParked] = useState(project.timecode.start);
  const openedFor = useRef<string | null>(null);
  const projectRef = useRef(project);
  projectRef.current = project;
  const parkedRef = useRef(parked);
  parkedRef.current = parked;
  const reportRef = useRef(report);
  reportRef.current = report;

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const keep = (off: () => void) => (cancelled ? off() : unlisteners.push(off));
    // Outside the desktop app there is no event bus; the UI still works parked.
    api.onPosition((position) => positionStore.set(position)).then(keep, () => undefined);
    api
      .onLevels(({ tracks }) => levelsStore.set(Object.fromEntries(tracks.map((t) => [t.id, t.peak]))))
      .then(keep, () => undefined);
    return () => {
      cancelled = true;
      unlisteners.forEach((off) => off());
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
    setClipWarnings([]);
    positionStore.set(null);
    await api.seek(parkedRef.current);
  }, [info]);

  // While the output runs, edits to the tracks (muting, moving, importing) reach the audio too.
  const { tracks, assets } = project;
  useEffect(() => {
    if (!info) return;
    const timer = setTimeout(() => {
      api.setClips(projectRef.current).then(setClipWarnings, (err) => reportRef.current(errorMessage(err)));
    }, CLIPS_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [info, tracks, assets]);

  const startOutput = useCallback(() => guard(open), [guard, open]);

  const closeOutput = useCallback(async () => {
    await api.stopOutput();
    openedFor.current = null;
    setInfo(null);
    positionStore.set(null);
  }, []);

  const stopOutput = useCallback(
    () =>
      guard(async () => {
        const last = positionStore.get();
        if (last) setParked(last.frame);
        await closeOutput();
      }),
    [guard, closeOutput],
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
        const current = projectRef.current;
        const { start } = current.timecode;
        setParked(start);
        if (!info) return;
        // An output opened for another start counts from the old one, so its position is wrong.
        if (openedFor.current !== signature(current)) return closeOutput();
        const last = positionStore.get();
        if (last) positionStore.set(stoppedAt(last, start));
        await api.stop();
      }),
    [guard, info, closeOutput],
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

  return { info, clipWarnings, parked, play, pause, stop, toggle, seek, startOutput, stopOutput };
}
