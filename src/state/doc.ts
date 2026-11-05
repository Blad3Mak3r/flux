// The open document: the project, where it lives on disk, and the undo history.

import type { Project, Timeline } from "../project/types";

/** The waveform the backend decoded: `[min, max]` byte pairs, `perSecond` of them per second. */
export interface Peaks {
  values: number[];
  perSecond: number;
}

/** What is known about an imported audio file. */
export interface AssetAudio {
  seconds: number;
  /** Kept as one object, so what draws it can tell by identity whether the waveform changed. */
  peaks?: Peaks;
}

export interface Doc {
  project: Project;
  /** Id of the timeline being edited and played. The backend chooses it; this only shows it. */
  active: string;
  /** Path of the `.fxtc` on disk, or `null` for a project that was never saved. */
  path: string | null;
  past: Project[];
  future: Project[];
  /** The project as last saved or opened; `null` for a new one. Dirty means "differs from this". */
  saved: Project | null;
  /** Real length and waveform of the assets the backend has decoded, by asset id. */
  audio: Record<string, AssetAudio>;
}

export type DocAction =
  | { type: "load"; project: Project; path: string | null; active: string }
  /** A document the backend kept while the window was reloaded; `saved` is what it is compared to. */
  | { type: "restore"; project: Project; path: string | null; saved: Project; active: string }
  /** The backend changed the project (a timeline was added, removed, retimed): it goes in the undo history. */
  | { type: "apply"; project: Project; active: string }
  /** The backend says which timeline is active, e.g. after an undo took one away. */
  | { type: "active"; id: string }
  | { type: "audio"; id: string; info: AssetAudio }
  | { type: "edit"; project: Project }
  | { type: "undo" }
  | { type: "redo" }
  /** `meta` is the one the backend wrote with the save. */
  | { type: "saved"; path: string; meta: Project["meta"] };

const HISTORY_LIMIT = 200;

export function initialDoc(project: Project): Doc {
  return { project, active: project.timelines[0]?.id ?? "", path: null, past: [], future: [], saved: null, audio: {} };
}

/** The active timeline. A project always has one; the first stands in while the backend catches up. */
export function activeTimeline(doc: Pick<Doc, "project" | "active">): Timeline {
  return doc.project.timelines.find((timeline) => timeline.id === doc.active) ?? doc.project.timelines[0];
}

export function isDirty(doc: Doc): boolean {
  return doc.saved === null ? doc.past.length > 0 : doc.project !== doc.saved;
}

export function docReducer(doc: Doc, action: DocAction): Doc {
  switch (action.type) {
    case "load":
      return {
        project: action.project,
        active: action.active,
        path: action.path,
        past: [],
        future: [],
        saved: action.path === null ? null : action.project,
        audio: {},
      };
    case "restore":
      return {
        project: action.project,
        active: action.active,
        path: action.path,
        past: [],
        future: [],
        saved: action.saved,
        audio: {},
      };
    case "audio":
      return { ...doc, audio: { ...doc.audio, [action.id]: action.info } };
    case "edit":
      if (action.project === doc.project) return doc;
      return {
        ...doc,
        project: action.project,
        past: [...doc.past, doc.project].slice(-HISTORY_LIMIT),
        future: [],
      };
    case "apply":
      if (action.project === doc.project) return action.active === doc.active ? doc : { ...doc, active: action.active };
      return {
        ...doc,
        project: action.project,
        active: action.active,
        past: [...doc.past, doc.project].slice(-HISTORY_LIMIT),
        future: [],
      };
    case "active":
      return action.id === doc.active ? doc : { ...doc, active: action.id };
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
    case "saved": {
      const project = { ...doc.project, meta: action.meta };
      return { ...doc, path: action.path, project, saved: project };
    }
  }
}
