// The open document: the project, where it lives on disk, and the undo history.

import type { Project, Timeline } from "../project/types";
import { DEFAULT_TEMPO } from "../project/types.ts";
import type { VideoProbe, VideoStrip } from "../video/api";

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
  /** The tempo the backend found in a video's sound. An audio file keeps its tempo in the project. */
  bpm?: number;
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
  /** What FFmpeg read from the videos, by asset id. */
  video: Record<string, VideoProbe>;
  /** The thumbnails the backend made of the videos, by asset id. */
  thumbs: Record<string, VideoStrip>;
  /**
   * While a controller moves a value, the project as it was before the first move: the history
   * takes it when the gesture ends (or when anything else is done), so a sweep is one undo step.
   */
  gesture?: Project;
}

export type DocAction =
  | { type: "load"; project: Project; path: string | null; active: string }
  /** A document the backend kept while the window was reloaded; `saved` is what it is compared to. */
  | { type: "restore"; project: Project; path: string | null; saved: Project; active: string }
  /** The backend changed the project (a timeline was added, removed, retimed): it goes in the undo history. */
  | { type: "apply"; project: Project; active: string }
  /**
   * A MIDI controller or an OSC message changed the project in the backend. Until `commit` the moves
   * replace the project without a history entry; the one that commits closes the gesture as a single step.
   */
  | { type: "controlled"; project: Project; active: string; commit: boolean }
  /** The backend says which timeline is active, e.g. after an undo took one away. */
  | { type: "active"; id: string }
  | { type: "audio"; id: string; info: AssetAudio }
  | { type: "video"; id: string; info: VideoProbe }
  | { type: "thumbs"; id: string; strip: VideoStrip }
  /** A tempo read from the audio of a file opened with none: derived data, so it is no edit (no history, not unsaved work). */
  | { type: "derivedBpm"; id: string; bpm: number }
  | { type: "undo" }
  | { type: "redo" }
  /** `meta` is the one the backend wrote with the save. */
  | { type: "saved"; path: string; meta: Project["meta"]; links?: Pick<Project, "assets" | "videos">; snapshot?: Project };

const HISTORY_LIMIT = 200;

/**
 * A project to fill the store until the backend answers with the real one (`startDocument` loads
 * it): it is never edited or saved. New projects are made by the backend.
 */
export function placeholderProject(name: string): Project {
  return {
    formatVersion: 12,
    autosave: { enabled: true, intervalSeconds: 30 },
    name,
    meta: { id: "placeholder", createdAt: "", modifiedAt: "", createdWith: "", savedWith: "" },
    assets: [],
    output: { ltc: [], mtc: [] },
    timelines: [{ id: "placeholder", name, timecode: { frameRate: "fps25", start: 0 }, tempo: DEFAULT_TEMPO, tracks: [] }],
  };
}

export function initialDoc(project: Project): Doc {
  return { project, active: project.timelines[0]?.id ?? "", path: null, past: [], future: [], saved: null, audio: {}, video: {}, thumbs: {} };
}

/** The active timeline. A project always has one; the first stands in while the backend catches up. */
export function activeTimeline(doc: Pick<Doc, "project" | "active">): Timeline {
  return doc.project.timelines.find((timeline) => timeline.id === doc.active) ?? doc.project.timelines[0];
}

export function isDirty(doc: Doc): boolean {
  return doc.saved === null ? doc.past.length > 0 : doc.project !== doc.saved;
}

/** The kinds of action that can arrive while a controller is moving a value without closing the gesture. */
const KEEPS_GESTURE = new Set<DocAction["type"]>(["controlled", "audio", "video", "thumbs", "derivedBpm", "active"]);

export function docReducer(doc: Doc, action: DocAction): Doc {
  // Anything else done meanwhile (an edit, an undo) is a step after the gesture, which is closed first.
  if (doc.gesture && !KEEPS_GESTURE.has(action.type)) {
    doc = { ...doc, gesture: undefined, past: [...doc.past, doc.gesture].slice(-HISTORY_LIMIT), future: [] };
  }
  return reduce(doc, action);
}

function reduce(doc: Doc, action: DocAction): Doc {
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
        video: {},
        thumbs: {},
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
        video: {},
        thumbs: {},
      };
    case "audio":
      return { ...doc, audio: { ...doc.audio, [action.id]: action.info } };
    case "video":
      return { ...doc, video: { ...doc.video, [action.id]: action.info } };
    case "thumbs":
      return { ...doc, thumbs: { ...doc.thumbs, [action.id]: action.strip } };
    case "derivedBpm": {
      const withBpm = (project: Project): Project => ({
        ...project,
        assets: project.assets.map((asset) => (asset.id === action.id ? { ...asset, bpm: action.bpm } : asset)),
      });
      const target = doc.project.assets.find((asset) => asset.id === action.id);
      if (!target || target.bpm !== undefined) return doc;
      const project = withBpm(doc.project);
      // The saved copy gets it too, so finding it does not make the document look edited.
      return { ...doc, project, saved: doc.saved === doc.project ? project : doc.saved && withBpm(doc.saved) };
    }
    case "apply":
      if (action.project === doc.project) return action.active === doc.active ? doc : { ...doc, active: action.active };
      return {
        ...doc,
        project: action.project,
        active: action.active,
        past: [...doc.past, doc.project].slice(-HISTORY_LIMIT),
        future: [],
      };
    case "controlled": {
      const before = doc.gesture ?? doc.project;
      if (!action.commit) return { ...doc, project: action.project, active: action.active, gesture: before };
      return { ...doc, gesture: undefined, project: action.project, active: action.active, past: [...doc.past, before].slice(-HISTORY_LIMIT), future: [] };
    }
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
      const snapshot = action.snapshot ?? doc.project;
      // The links as the save wrote them, so the window and the backend hold the same project.
      const saved = { ...snapshot, ...action.links, meta: action.meta };
      const project = doc.project === snapshot ? saved : { ...doc.project, meta: action.meta };
      return { ...doc, path: action.path, project, saved };
    }
  }
}
