// Drives the backend output for the open project. The UI works without it: with no output running
// the playhead is parked and seeking only moves it, and the first Play opens the devices.
//
// Plain functions over the stores rather than a hook: nothing here renders, so nothing re-renders
// when it works, and every export is one stable function.

import { useLive } from "../live/store";
import { errorMessage } from "../project/api";
import type { Project } from "../project/types";
import { useDoc } from "../state/store";
import { sayError } from "../state/ui";
import * as api from "./api";
import type { OutputLanes } from "./api";
import { latestOnly } from "./latest";
import { stoppedAt } from "./stopped";
import { useTransport } from "./store";

/** Edits arrive in bursts (dragging a clip); the audio is rebuilt once they pause. */
const CLIPS_DEBOUNCE_MS = 200;

/** What the output was opened for; when it changes, the next Play reopens the devices. */
const signature = (project: Project) => JSON.stringify([project.timecode, project.output]);

const project = () => useDoc.getState().project;
const report = (error: unknown) => sayError(errorMessage(error));

let openedFor: string | null = null;
/** Counts the outputs opened; a tick that comes from one closed since is dropped. */
let run = 0;
let clipsTimer: ReturnType<typeof setTimeout> | undefined;

async function guard(action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    report(error);
  }
}

/** Each lane writes only its own field, so the meters never wait for the timecode or the other way round. */
const lanes = (id: number): OutputLanes => ({
  onPosition: (position) => id === run && useTransport.setState({ position }),
  onLevels: (tracks) =>
    id === run && useTransport.setState({ levels: Object.fromEntries(tracks.map((track) => [track.id, track.peak])) }),
  onLive: (state) => id === run && useLive.setState({ state }),
});

/** While the output runs, edits to the tracks (muting, moving, importing) reach the audio too. */
function scheduleClips(): void {
  clearTimeout(clipsTimer);
  if (!useTransport.getState().info) return;
  clipsTimer = setTimeout(() => {
    if (!useTransport.getState().info) return;
    api.setClips(project()).then((clipWarnings) => useTransport.setState({ clipWarnings }), report);
  }, CLIPS_DEBOUNCE_MS);
}

/**
 * Keeps the output in step with the document: a project with another start parks there, and edits
 * to the tracks reach a running output. Call once from the root; call the result to stop.
 */
export function bindTransport(): () => void {
  const unsubscribe = useDoc.subscribe(({ project: now }, { project: before }) => {
    if (now.timecode.start !== before.timecode.start) useTransport.setState({ parked: now.timecode.start });
    if (now.tracks !== before.tracks || now.assets !== before.assets) scheduleClips();
  });
  return () => {
    unsubscribe();
    clearTimeout(clipsTimer);
  };
}

async function open(): Promise<void> {
  const current = project();
  if (useTransport.getState().info && openedFor === signature(current)) return;
  const info = await api.startOutput(current, lanes(++run));
  openedFor = signature(current);
  useTransport.setState({ info, clipWarnings: [], position: null });
  scheduleClips();
  await api.seek(useTransport.getState().parked);
}

async function closeOutput(): Promise<void> {
  run += 1;
  clearTimeout(clipsTimer);
  await api.stopOutput();
  openedFor = null;
  useTransport.setState({ info: null, position: null });
}

export const startOutput = () => guard(open);

export const stopOutput = () =>
  guard(async () => {
    const { position } = useTransport.getState();
    if (position) useTransport.setState({ parked: position.frame });
    await closeOutput();
  });

export const play = () =>
  guard(async () => {
    await open();
    await api.play();
  });

export const pause = () => guard(api.pause);

export const stop = () =>
  guard(async () => {
    const current = project();
    const { start } = current.timecode;
    useTransport.setState({ parked: start });
    const { info, position } = useTransport.getState();
    if (!info) return;
    // An output opened for another start counts from the old one, so its position is wrong.
    if (openedFor !== signature(current)) return closeOutput();
    if (position) useTransport.setState({ position: stoppedAt(position, start) });
    await api.stop();
  });

export const toggle = () => (useTransport.getState().position?.state === "playing" ? pause() : play());

/** Dragging the playhead moves it faster than the backend needs: the newest target wins. */
const sendSeek = latestOnly(api.seek, report);

export function seek(frame: number): void {
  const target = Math.max(project().timecode.start, Math.round(frame));
  const { position, info } = useTransport.getState();
  useTransport.setState({ parked: target, ...(position && { position: { ...position, frame: target } }) });
  if (info) sendSeek(target);
}

const faders = new Map<string, (gainDb: number) => void>();

/** Moves a track's fader on the running output while it is dragged. No output is not an error. */
export function previewTrackGain(trackId: string, gainDb: number): void {
  let send = faders.get(trackId);
  if (!send) {
    send = latestOnly((db: number) => api.setTrackGain(trackId, db), () => undefined);
    faders.set(trackId, send);
  }
  send(gainDb);
}
