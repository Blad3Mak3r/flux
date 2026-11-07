// Keeps the backend's copy of the document current, so that reloading the window does not lose it.

import { syncProject } from "../project/api";
import type { Project } from "../project/types";
import { dispatch, useDoc } from "./store";

/**
 * An edit can follow another faster than the backend writes it down: the newest state wins. The
 * backend answers with the active timeline, which an undo in the window can have changed.
 */
let waiting: Project | null = null;
let running: Promise<void> | null = null;

function send(project: Project): void {
  waiting = project;
  running ??= (async () => {
    while (waiting) {
      const next = waiting;
      waiting = null;
      try {
        const id = await syncProject(next);
        if (id && useDoc.getState().project.meta.id === next.meta.id) dispatch({ type: "active", id });
      } catch { /* A document may have closed while its last sync was in flight. */ }
    }
  })().finally(() => { running = null; });
}

/** Drain queued edits before saving, replacing or closing the document. */
export async function flushDocumentSync(): Promise<void> {
  while (running) await running;
}

/** Sends every new state of the project to the backend. Call once from the root; call the result to stop. */
export function bindDocumentSync(): () => void {
  return useDoc.subscribe((doc, before) => {
    if (doc.project !== before.project) send(doc.project);
  });
}
