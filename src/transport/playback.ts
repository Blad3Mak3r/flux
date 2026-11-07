import { resolveOutput, usePreferences } from "../preferences/store";
// Drives the backend output for the open project. The output connects by itself: when the window
// starts, and whenever the output settings change, the devices are opened, so there is no Start
// button. Physical routing changes wait for pause/stop. Signal-only LTC/MTC changes can be applied without
// touching the programme's stream; a change that reaches the programme's stream (its device, channels,
// the LTC on it, the metronome, the frame rate) waits until playback stops or pauses, so the timecode
// never glitches mid-show. When the programme's device is lost and comes back, the output reopens at the
// same frame, stopped. The UI still works with no output running (a device is missing, or this is
// not the desktop app): the playhead is parked and seeking only moves it.
//
// Plain functions over the stores rather than a hook: nothing here renders, so nothing re-renders
// when it works, and every export is one stable function.

import { listen } from "@tauri-apps/api/event";
import { useLive } from "../live/store";
import { errorMessage } from "../project/api";
import { activeTimeline } from "../state/doc";
import type { Doc } from "../state/doc";
import { useDoc } from "../state/store";
import { hasNativeWindow } from "../components/WindowControls";
import { sayError, useUi } from "../state/ui";
import { realFps } from "../timecode";
import * as api from "./api";
import type { OutputLanes } from "./api";
import { latestOnly } from "./latest";
import { displayClock } from "./smooth";
import { stoppedAt } from "./stopped";
import { useTransport } from "./store";

/** Edits arrive in bursts (dragging a clip); the audio is rebuilt once they pause. */
const CLIPS_DEBOUNCE_MS = 200;

/** Choosing a device fires several edits in a row (a select, then a channel); one reconnect follows. */
const RECONNECT_DEBOUNCE_MS = 300;

/** What the output is opened for, including how the current view uses the devices. */
function setup(doc: Doc): api.OutputSetup {
  const { id: timelineId, timecode } = activeTimeline(doc);
  const { view } = useUi.getState();
  return {
    timelineId,
    timecode,
    output: view === "edit" ? usePreferences.getState().editOutput : usePreferences.getState().output,
    advance: view === "live",
    audition: view === "edit" && usePreferences.getState().audition,
  };
}

/** What the output was opened for; when it changes, the devices are reopened. */
const signature = (doc: Doc) => JSON.stringify(setup(doc));

const project = () => useDoc.getState().project;
const timeline = () => activeTimeline(useDoc.getState());
const report = (error: unknown) => sayError(errorMessage(error));

let openedFor: string | null = null;
/** The start offset the open output counts from; a parked playhead is absolute, so it is only carried over while this holds. */
let openedStart: number | null = null;
/** The timeline the open output plays; its parked playhead belongs to no other. */
let openedTimeline: string | null = null;
/** The view the open output was opened for; the other view may play through other outputs. */
let openedView: string | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
/** Counts the outputs opened; a tick that comes from one closed since is dropped. */
let run = 0;
let clipsTimer: ReturnType<typeof setTimeout> | undefined;
let attaching: Promise<void> | undefined;
let initialized = false;

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

/** Once per page, pick up the backend output and its transport before considering a restart. */
function attach(): Promise<void> {
  attaching ??= (async () => {
    const requested = setup(useDoc.getState());
    const subscriber = lanes(++run);
    const attached = await api.attachOutput(requested, subscriber);
    if (!attached) return;
    openedFor = attached.matches ? JSON.stringify(requested) : null;
    openedStart = attached.start;
    openedTimeline = attached.timelineId;
    openedView = attached.matches ? useUi.getState().view : null;
    if (!useTransport.getState().position) subscriber.onPosition(attached.position);
    useTransport.setState({ info: attached.info });
  })().catch((error) => {
    attaching = undefined;
    throw error;
  });
  return attaching;
}

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
    if (now.project.output !== before.project.output || signature(now) !== signature(before)) reconnect();
  });
  // A change that waited for playback to end is applied as soon as it does.
  const unsubscribeTransport = useTransport.subscribe((state) => {
    if (state.pendingReconfigure && state.position?.state !== "playing") reconnect();
  });
  // The Edit and Live views may play through different devices.
  const unsubscribeView = useUi.subscribe((now, before) => {
    if (now.view !== before.view) reconnect();
  });
  const unsubscribePrefs = usePreferences.subscribe((now, before) => {
    if (now.saved !== before.saved) reconnect();
  });
  const listening = listen("output-device-back", () => reconnect(true)).catch(() => undefined);
  return () => {
    void listening.then((unlisten) => unlisten?.());
    unsubscribePrefs();
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
  if (!initialized) return;
  clearTimeout(reconnectTimer);
  reconnectTimer = setTimeout(() => void connect(force), RECONNECT_DEBOUNCE_MS);
}

/** The editor awaits the first connection instead of queuing it behind the settings debounce. */
export async function startTransport(): Promise<void> {
  await connect(false);
  initialized = true;
  if (useTransport.getState().pendingReconfigure && useTransport.getState().position?.state !== "playing") reconnect();
}

async function connect(force: boolean): Promise<void> {
  // Outside the desktop app there is no backend to open anything on.
  if (!hasNativeWindow()) return;
  try {
    await resolveOutput(useDoc.getState().project.output);
    await attach();
  } catch (error) {
    report(error);
    return;
  }
  if (!force && useTransport.getState().info && openedFor === signature(useDoc.getState())) {
    useTransport.setState({ pendingReconfigure: false });
    return;
  }
  if (!force && useTransport.getState().info && (await reconfigureInPlace())) return;
  // The state may have moved while the backend answered.
  const { position } = useTransport.getState();
  // A lost device is not playing anything, whatever the last position says: it reopens at once.
  if (position?.state === "playing" && !position.deviceLost) {
    useTransport.setState({ pendingReconfigure: true });
    return;
  }
  useTransport.setState({ pendingReconfigure: false });
  try {
    await open(true);
  } catch (error) {
    // The backend already dropped the old output when it tried the new one.
    await closeOutput();
    report(error);
  }
}

/**
 * Asks the backend to bring the open output in line with the project without reopening it. False
 * when that is not possible (another timeline or view, or the change reaches the programme's
 * stream) and the output has to be opened again.
 */
async function reconfigureInPlace(): Promise<boolean> {
  const doc = useDoc.getState();
  const requested = setup(doc);
  const { timelineId, timecode, audition } = requested;
  if (openedTimeline !== timelineId || openedStart !== timecode.start || openedView !== useUi.getState().view) return false;
  try {
    const result = await api.reconfigureOutput(doc.project, audition);
    if (result.kind !== "applied") return false;
    openedFor = JSON.stringify(requested);
    useTransport.setState({ info: result.info, pendingReconfigure: false });
    return true;
  } catch {
    return false;
  }
}
async function open(force = false): Promise<void> {
  const doc = useDoc.getState();
  const requested = setup(doc);
  const { timelineId, timecode, advance, audition } = requested;
  const key = JSON.stringify(requested);
  const { position } = useTransport.getState();
  if (!force && useTransport.getState().info && (openedFor === key || position?.state === "playing")) return;
  // Reopening from a pause or a stop keeps the playhead where it was, unless the start moved or
  // it is another timeline.
  if (position && openedStart === timecode.start && openedTimeline === timelineId) {
    useTransport.setState({ parked: position.frame });
  }
  const info = await api.startOutput(doc.project, advance, audition, lanes(++run));
  openedFor = key;
  openedStart = timecode.start;
  openedTimeline = timelineId;
  openedView = useUi.getState().view;
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
  openedView = null;
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
    await attach();
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
