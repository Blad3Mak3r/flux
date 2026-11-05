// Keeps the live store in step with the backend while the Live view is open. A running output
// streams the state itself (see `transport/playback.ts`); this asks for the cue list when it
// changes and for the state when no output runs, because then the playhead is parked in the window.

import { errorMessage } from "../project/api";
import { activeTimeline } from "../state/doc";
import type { Doc } from "../state/doc";
import { useDoc } from "../state/store";
import { sayError } from "../state/ui";
import { useTransport } from "../transport/store";
import * as api from "./api";
import { useLive } from "./store";

/** Edits arrive in bursts (dragging a clip); the list is asked for again once they pause. */
const CUES_DEBOUNCE_MS = 200;

const report = (error: unknown) => sayError(errorMessage(error));

/**
 * Count the requests of each kind made; an answer to one that was overtaken is dropped. They are
 * counted apart: a playhead that moved must not throw away a cue list still on its way.
 */
let cuesTicket = 0;
let stateTicket = 0;

/** What the cues of the active timeline are derived from. */
const sourceOf = (doc: Doc) => {
  const { id, tracks, timecode } = activeTimeline(doc);
  return { id, tracks, assets: doc.project.assets, timecode };
};

type Source = ReturnType<typeof sourceOf>;

const sameSource = (a: Source | null, b: Source) =>
  a !== null && a.id === b.id && a.tracks === b.tracks && a.assets === b.assets && a.timecode === b.timecode;

/** Whether the cues in the store were derived from what the open timeline holds now. */
const current = () => sameSource(useLive.getState().source, sourceOf(useDoc.getState()));

async function refreshState(): Promise<void> {
  const { info, parked } = useTransport.getState();
  if (info) return; // The running output reports it.
  const ticket = ++stateTicket;
  const state = await api.liveState(useDoc.getState().project, parked);
  if (ticket === stateTicket) useLive.setState({ state });
}

async function refresh(): Promise<void> {
  const ticket = ++cuesTicket;
  const doc = useDoc.getState();
  try {
    const cues = await api.liveCues(doc.project);
    if (ticket !== cuesTicket) return;
    useLive.setState({ cues, ready: true, source: sourceOf(doc) });
    await refreshState();
  } catch (error) {
    // Say what went wrong, and show the (empty) list rather than wait for ever.
    if (ticket === cuesTicket) useLive.setState({ ready: true });
    report(error);
  }
}

/** Starts feeding the store; call the result to stop. */
export function bindLive(): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const later = () => {
    clearTimeout(timer);
    timer = setTimeout(refresh, CUES_DEBOUNCE_MS);
  };

  // Cues of another project (one opened while the editor was showing) are not this show's.
  if (!current()) useLive.setState({ cues: [], state: null, ready: false, source: null });
  void refresh();

  const unDoc = useDoc.subscribe((now, before) => {
    if (!sameSource(sourceOf(before), sourceOf(now))) later();
  });
  const unTransport = useTransport.subscribe((now, before) => {
    // A parked playhead that moved, or an output that closed and left the playhead where it was.
    if (!now.info && (now.parked !== before.parked || before.info)) refreshState().catch(report);
  });
  return () => {
    cuesTicket += 1;
    stateTicket += 1;
    clearTimeout(timer);
    unDoc();
    unTransport();
  };
}
