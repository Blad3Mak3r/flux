// Keeps the backend's copy of the document current, so that reloading the window does not lose it.

import { syncProject } from "../project/api";
import type { Project } from "../project/types";
import { latestOnly } from "../transport/latest";
import { dispatch, useDoc } from "./store";

/**
 * An edit can follow another faster than the backend writes it down: the newest state wins. The
 * backend answers with the active timeline, which an undo in the window can have changed.
 */
const send = latestOnly(
  async (project: Project) => {
    const id = await syncProject(project);
    if (id) dispatch({ type: "active", id });
  },
  () => undefined,
);

/** Sends every new state of the project to the backend. Call once from the root; call the result to stop. */
export function bindDocumentSync(): () => void {
  return useDoc.subscribe((doc, before) => {
    if (doc.project !== before.project) send(doc.project);
  });
}
