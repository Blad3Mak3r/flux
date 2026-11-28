// The open document as a store. Components read the slice they draw, so an edit re-renders what it
// changed and nothing else; code outside components reads the latest with `useDoc.getState()`.

import { create } from "zustand";
import { initialLocale, translator } from "../i18n/messages";
import { activeTimeline, docReducer, initialDoc, placeholderProject } from "./doc";
import type { Doc, DocAction } from "./doc";

export const useDoc = create<Doc>()(() =>
  initialDoc(placeholderProject(translator(initialLocale())("project.untitled"))),
);

/**
 * Applies an action to the document. Always the same function, so it is safe to hand to any child. The reducer
 * answers a whole document, so it replaces the state: merged, a field it drops (the open `gesture`) would stay.
 */
export function dispatch(action: DocAction): void {
  useDoc.setState((doc) => docReducer(doc, action), true);
}

/** The active timeline, for a component to draw. It only re-renders when that timeline changes. */
export const useTimeline = () => useDoc(activeTimeline);
