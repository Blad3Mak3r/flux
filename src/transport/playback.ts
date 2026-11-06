// Drives the backend output for the open project. The output connects by itself: when the window
// starts, and whenever the output settings change, the devices are opened, so there is no Start
// button. A change made while playing waits until playback stops or pauses, so the timecode never
// glitches mid-show. The UI still works with no output running (a device is missing, or this is
// not the desktop app): the playhead is parked and seeking only moves it.
//
// Plain functions over the stores rather than a hook: nothing here renders, so nothing re-renders
// when it works, and every export is one stable function.

import { useLive } from "../live/store";
import { errorMessage } from "../project/api";
import type { Project } from "../project/types";
import { activeTimeline } from "../state/doc";
import type { Doc } from "../state/doc";
import { useDoc } from "../state/store";
import { hasNativeWindow } from "../components/WindowControls";
import { sayError, useUi } from "../state/ui";
import { realFps } from "../timecode";
import * as api from "./api";
import type { OutputLanes } from "./api";
import { effectiveOutput } from "./effective";
import { latestOnly } from "./latest";
import { displayClock } from "./smooth";
import { stoppedAt } from "./stopped";
import { useTransport } from "./store";

/** Edits arrive in bursts (dragging a clip); the audio is rebuilt once they pause. */
const CLIPS_DEBOUNCE_MS = 200;

/** Choosing a device fires several edits in a row (a select, then a channel); one reconnect follows. */
const RECONNECT_DEBOUNCE_MS = 300;

/** What the output is opened with: the project, with the outputs the current view plays through. */
const forView = (project: Project): Project => ({
  ...project,
  output: effectiveOutput(project.output, useUi.getState().view),
});

/** What the output was opened for; when it changes, the devices are reopened. */
const signature = (doc: Doc) => {
  const { id, timecode } = activeTimeline(doc);
  return JSON.stringify([id, timecode, forView(doc.project).output]);
};

const project = () => useDoc.getState().project;
const timeline = () => activeTimeline(useDoc.getState());
const report = (error: unknown) => sayError(errorMessage(error));

let openedFor: string | null = null;
/** The start offset the open output counts from; a parked playhead is absolute, so it is only carried over while this holds. */
let openedStart: number | null = null;
/** The timeline the open output plays; its parked playhead belongs to no other. */
let openedTimeline: string | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
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
  onPosition: (position) => {
    if (id !== run) return;
    const { timecode } = timeline();
    displayClock.feed(position, performance.now(), realFps(timecode.frameRate), timecode.start);
    useTransport.setState({ position });
  },
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
  const unsubscribeDoc = useDoc.subscribe((now, before) => {
    const [current, previous] = [activeTimeline(now), activeTimeline(before)];
    if (current.id !== previous.id || current.timecode.start !== previous.timecode.start) {
      useTransport.setState({ parked: current.timecode.start });
    }
    // Another timeline is played by reopening the output, which builds its clips.
    const sameTimeline = current.id === previous.id;
    if (sameTimeline && (current.tracks !== previous.tracks || now.project.assets !== before.project.assets)) {
      scheduleClips();
    }
    // The metronome follows the tempo and its switch while the show runs; no reopening needed.
    if (sameTimeline && current.tempo !== previous.tempo && useTransport.getState().info) sendTempo(current.tempo);
    if (signature(now) !== signature(before)) reconnect();
  });
  // A change that waited for playback to end is applied as soon as it does.
  const unsubscribeTransport = useTransport.subscribe((state) => {
    if (state.pendingReconfigure && state.position?.state !== "playing") reconnect();
  });
  // The Edit and Live views may play through different devices.
  const unsubscribeView = useUi.subscribe((now, before) => {
    if (now.view !== before.view) reconnect();
  });
  reconnect();
  return () => {
    unsubscribeDoc();
    unsubscribeTransport();
    unsubscribeView();
    clearTimeout(clipsTimer);
    clearTimeout(reconnectTimer);
  };
}

/**
 * Brings the output in line with the project's settings, once the edits settle. `force` reopens
 * even when nothing changed, for a device that was plugged in since.
 */
export function reconnect(force = false): void {
  clearTimeout(reconnectTimer);
  reconnectTimer = setTimeout(() => void connect(force), RECONNECT_DEBOUNCE_MS);
}

async function connect(force: boolean): Promise<void> {
  // Outside the desktop app there is no backend to open anything on.
  if (!hasNativeWindow()) return;
  const { position, info } = useTransport.getState();
  if (position?.state === "playing") {
    useTransport.setState({ pendingReconfigure: true });
    return;
  }
  useTransport.setState({ pendingReconfigure: false });
  if (!force && info && openedFor === signature(useDoc.getState())) return;
  try {
    await open(true);
  } catch (error) {
    // The backend already dropped the old output when it tried the new one.
    await closeOutput();
    report(error);
  }
}

async function open(force = false): Promise<void> {
  const doc = useDoc.getState();
  const { id, timecode } = activeTimeline(doc);
  if (!force && useTransport.getState().info && openedFor === signature(doc)) return;
  const { position } = useTransport.getState();
  // Reopening from a pause or a stop keeps the playhead where it was, unless the start moved or
  // it is another timeline.
  if (position && openedStart === timecode.start && openedTimeline === id) {
    useTransport.setState({ parked: position.frame });
  }
  // In Live a show that runs to its end hands over to the next timeline, stopped.
  const advance = useUi.getState().view === "live";
  const audition = useUi.getState().view === "edit" && doc.project.output.preview !== undefined;
  const info = await api.startOutput(forView(doc.project), advance, audition, lanes(++run));
  openedFor = signature(doc);
  openedStart = timecode.start;
  openedTimeline = id;
  useTransport.setState({ info, clipWarnings: [], position: null });
  scheduleClips();
  await api.seek(useTransport.getState().parked);
}

async function closeOutput(): Promise<void> {
  run += 1;
  clearTimeout(clipsTimer);
  await api.stopOutput();
  openedFor = null;
  openedStart = null;
  openedTimeline = null;
  useTransport.setState({ info: null, position: null });
}

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
    const doc = useDoc.getState();
    const { start } = activeTimeline(doc).timecode;
    useTransport.setState({ parked: start });
    displayClock.jump();
    const { info, position } = useTransport.getState();
    if (!info) return;
    // An output opened for another start counts from the old one, so its position is wrong.
    if (openedFor !== signature(doc)) return closeOutput();
    if (position) useTransport.setState({ position: stoppedAt(position, start) });
    await api.stop();
  });

export const toggle = () => (useTransport.getState().position?.state === "playing" ? pause() : play());

/** Typing a tempo or spinning its field sends many values; the newest wins. */
const sendTempo = latestOnly(api.setTempo, report);

/** Dragging the playhead moves it faster than the backend needs: the newest target wins. */
const sendSeek = latestOnly(api.seek, report);

export function seek(frame: number): void {
  const target = Math.max(timeline().timecode.start, Math.round(frame));
  const { position, info } = useTransport.getState();
  displayClock.jump();
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
