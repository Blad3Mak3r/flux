// Keeps the live store in step with the backend while the Live view is open. A running output
// streams the state itself (see `transport/playback.ts`); this asks for the cue list when it
// changes and for the state when no output runs, because then the playhead is parked in the window.

import { errorMessage } from "../project/api";
import { useDoc } from "../state/store";
import { sayError } from "../state/ui";
import { useTransport } from "../transport/store";
import * as api from "./api";
import { useLive } from "./store";

/** Edits arrive in bursts (dragging a clip); the list is asked for again once they pause. */
const CUES_DEBOUNCE_MS = 200;

const report = (error: unknown) => sayError(errorMessage(error));

/** Counts the requests made; an answer to one that was overtaken is dropped. */
let latest = 0;

async function refreshState(ticket: number): Promise<void> {
  const { info, parked } = useTransport.getState();
  if (info) return; // The running output reports it.
  const state = await api.liveState(useDoc.getState().project, parked);
  if (ticket === latest) useLive.setState({ state });
}

async function refresh(): Promise<void> {
  const ticket = ++latest;
  try {
    const cues = await api.liveCues(useDoc.getState().project);
    if (ticket !== latest) return;
    useLive.setState({ cues });
    await refreshState(ticket);
  } catch (error) {
    report(error);
  }
}

async function refreshParked(): Promise<void> {
  try {
    await refreshState(++latest);
  } catch (error) {
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

  void refresh();
  const unDoc = useDoc.subscribe(({ project: now }, { project: before }) => {
    if (now.tracks !== before.tracks || now.assets !== before.assets || now.timecode !== before.timecode) later();
  });
  const unTransport = useTransport.subscribe((now, before) => {
    // A parked playhead that moved, or an output that closed and left the playhead where it was.
    if (!now.info && (now.parked !== before.parked || before.info)) void refreshParked();
  });
  return () => {
    latest += 1;
    clearTimeout(timer);
    unDoc();
    unTransport();
  };
}
