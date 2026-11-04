// The open document as a store. Components read the slice they draw, so an edit re-renders what it
// changed and nothing else; code outside components reads the latest with `useDoc.getState()`.

import { create } from "zustand";
import { initialLocale, translator } from "../i18n/messages";
import { docReducer, initialDoc } from "./doc";
import type { Doc, DocAction } from "./doc";
import { emptyProject } from "./edit";

export const useDoc = create<Doc>()(() =>
  initialDoc(emptyProject(translator(initialLocale())("project.untitled"), "fps25")),
);

/** Applies an action to the document. Always the same function, so it is safe to hand to any child. */
export function dispatch(action: DocAction): void {
  useDoc.setState((doc) => docReducer(doc, action));
}
