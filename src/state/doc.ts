// The open document: the project, where it lives on disk, and the undo history.

import type { Project } from "../project/types";

export interface Doc {
  project: Project;
  /** Path of the `.fxtc` on disk, or `null` for a project that was never saved. */
  path: string | null;
  past: Project[];
  future: Project[];
  /** The project as last saved or opened; `null` for a new one. Dirty means "differs from this". */
  saved: Project | null;
  /** The demo show uses synthetic audio the backend has never seen, so it cannot be saved. */
  demo: boolean;
  /** Length in frames of the audio assets we know the duration of. */
  durations: Record<string, number>;
}

export type DocAction =
  | { type: "load"; project: Project; path: string | null; demo?: boolean; durations?: Record<string, number> }
  | { type: "edit"; project: Project }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "saved"; path: string };

const HISTORY_LIMIT = 200;

export function initialDoc(project: Project): Doc {
  return { project, path: null, past: [], future: [], saved: null, demo: false, durations: {} };
}

export function isDirty(doc: Doc): boolean {
  return doc.saved === null ? doc.past.length > 0 : doc.project !== doc.saved;
}

export function docReducer(doc: Doc, action: DocAction): Doc {
  switch (action.type) {
    case "load":
      return {
        project: action.project,
        path: action.path,
        past: [],
        future: [],
        saved: action.path === null ? null : action.project,
        demo: action.demo ?? false,
        durations: action.durations ?? {},
      };
    case "edit":
      if (action.project === doc.project) return doc;
      return {
        ...doc,
        project: action.project,
        past: [...doc.past, doc.project].slice(-HISTORY_LIMIT),
        future: [],
      };
    case "undo": {
      const previous = doc.past[doc.past.length - 1];
      if (!previous) return doc;
      return {
        ...doc,
        project: previous,
        past: doc.past.slice(0, -1),
        future: [doc.project, ...doc.future],
      };
    }
    case "redo": {
      const [next, ...rest] = doc.future;
      if (!next) return doc;
      return { ...doc, project: next, past: [...doc.past, doc.project], future: rest };
    }
    case "saved":
      return { ...doc, path: action.path, saved: doc.project };
  }
}
