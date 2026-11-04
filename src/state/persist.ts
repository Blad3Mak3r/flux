// Keeps the backend's copy of the document current, so that reloading the window does not lose it.

import { syncProject } from "../project/api";
import { latestOnly } from "../transport/latest";
import { useDoc } from "./store";

/** An edit can follow another faster than the backend writes it down: the newest state wins. */
const send = latestOnly(syncProject, () => undefined);

/** Sends every new state of the project to the backend. Call once from the root; call the result to stop. */
export function bindDocumentSync(): () => void {
  return useDoc.subscribe((doc, before) => {
    if (doc.project !== before.project) send(doc.project);
  });
}
