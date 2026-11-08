import { translator, initialLocale, type MessageKey } from "../i18n/messages";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { en } from "../i18n/en";
import type {
  AudioAsset,
  EnvCurve,
  FadeCurve,
  FrameRate,
  MidiMessage,
  MidiPortRef,
  OscArg,
  Project,
  ProjectState,
} from "./types";

/** The document the backend holds for the window: the project, where it lives and how it started. */
export interface BackendDocument {
  project: Project;
  /** Path of the `.flx` or legacy `.fxtc`, or `null` for a project that was never saved. */
  path: string | null;
  /** The project as last opened or saved, or as it started: the document is unsaved when it differs. */
  baseline: Project;
  /** Id of the active timeline. */
  active: string;
}

const PROJECT_FILTER = { name: en["app.title"], extensions: ["flx", "fxtc"] };
const SAVE_FILTER = { name: en["app.title"], extensions: ["flx"] };
const AUDIO_FILTER = {
  name: "Audio",
  extensions: ["wav", "aif", "aiff", "flac", "mp3", "ogg"],
};

/** Backend errors arrive as plain message strings. */
export function errorMessage(err: unknown): string {
  const message = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
  return Object.prototype.hasOwnProperty.call(en, message) ? translator(initialLocale())(message as MessageKey) : message;
}

/** The document being edited, or `null` before one was started: what a reloaded window picks up. */
export function projectDocument(): Promise<BackendDocument | null> {
  return invoke<BackendDocument | null>("project_document");
}

/**
 * Gives the backend the latest state of the project, so a reloaded window gets it back. It answers
 * with the active timeline, which an undo may have changed.
 */
export function syncProject(project: Project): Promise<string> {
  return invoke<string>("project_sync", { project });
}

export const projectClose = () => invoke<void>("project_close");
export const projectSetAutosave = (project: Project, enabled: boolean, intervalSeconds: string) =>
  invoke<ProjectState>("project_set_autosave", { project, enabled, intervalSeconds });

export function newProject(name: string, frameRate: FrameRate): Promise<ProjectState> {
  return invoke<ProjectState>("project_new", { name, frameRate });
}

/** Asks for a `.flx` or legacy `.fxtc`. Resolves to `null` when the dialog is cancelled. */
export function pickProjectPath(): Promise<string | null> {
  return open({ filters: [PROJECT_FILTER] });
}

export function openProjectAt(path: string): Promise<ProjectState> {
  return invoke<ProjectState>("project_open", { path });
}

// The timeline commands do the work in the backend and answer with the project as it is now. They
// take the project the window shows, which the backend makes its document first.

/** Adds a timeline named `"{label} N"` and activates it. */
export const timelineAdd = (project: Project, label: string) =>
  invoke<ProjectState>("timeline_add", { project, label });

export const timelineRename = (project: Project, id: string, name: string) =>
  invoke<ProjectState>("timeline_rename", { project, id, name });

export const timelineRemove = (project: Project, id: string) =>
  invoke<ProjectState>("timeline_remove", { project, id });

export const timelineSelect = (project: Project, id: string) =>
  invoke<ProjectState>("timeline_select", { project, id });

/** Puts a timeline at index `to` of the order; the backend clamps it. */
export const timelineMove = (project: Project, id: string, to: number) =>
  invoke<ProjectState>("timeline_move", { project, id, to });

/** Sets the frame rate and the start of the active timeline. */
export const timelineSetTimecode = (project: Project, frameRate: FrameRate, start: number) =>
  invoke<ProjectState>("timeline_set_timecode", { project, frameRate, start });

/** What can be done to a clip; the backend decides how (see `project/clips.rs`). */
export type ClipOp = "trimStart" | "trimEnd" | "fadeIn" | "fadeOut" | "split";

/** Trims, fades or splits a clip of the active timeline at `frame` of it; answers with the project as it is now. */
export const clipEdit = (project: Project, id: string, op: ClipOp, frame: number) =>
  invoke<ProjectState>("clip_edit", { project, id, op, frame });

/** What can be done to the names of tracks and clips, a clip's fade curves and its volume envelope; the backend decides how (see `project/item_ops.rs`). Envelope positions are in thousandths of a frame from the start of the audio file. */
export type ItemOp =
  | { op: "renameTrack"; id: string; name: string }
  | { op: "renameClip"; id: string; name: string }
  | { op: "setFadeCurve"; id: string; fade: "in" | "out"; curve: FadeCurve }
  | { op: "addEnvelopePoint"; id: string; at: number; gainDb: number }
  | { op: "moveEnvelopePoint"; id: string; index: number; at: number; gainDb: number }
  | { op: "removeEnvelopePoint"; id: string; index: number }
  | { op: "setEnvelopeCurve"; id: string; index: number; curve: EnvCurve }
  | { op: "clearEnvelope"; id: string };

export const itemEdit = (project: Project, op: ItemOp) => invoke<ProjectState>("item_edit", { project, op });

/** Which kind of cue track an operation is about. */
export type CueTrackKind = "midi" | "osc";

/** What can be done to the MIDI and OSC tracks and the OSC destinations; the backend decides how (see `project/cue_ops.rs`). */
export type CueOp =
  | { op: "setFirstTurnOnly"; id: string; firstTurnOnly: boolean }
  | { op: "addTrack"; kind: CueTrackKind; name: string }
  | { op: "addEvent"; trackId?: string; kind: CueTrackKind; at: number; trackName: string }
  | { op: "setMidi"; id: string; message: MidiMessage }
  | { op: "setOsc"; id: string; address: string; args: OscArg[] }
  | { op: "moveEvent"; id: string; at: number }
  | { op: "removeEvent"; id: string }
  | { op: "setMidiPort"; trackId: string; port: MidiPortRef | null }
  | { op: "setOscDestination"; trackId: string; destination: string | null }
  | { op: "addDestination"; name: string; host: string; port: number }
  | { op: "updateDestination"; id: string; name: string; host: string; port: number }
  | { op: "removeDestination"; id: string };

/** The project after a cue operation, and the id of what it created. */
export interface CueEdited extends ProjectState {
  id: string | null;
}

export const cueEdit = (project: Project, op: CueOp) => invoke<CueEdited>("cue_edit", { project, op });

export type SignalOp =
  | { op: "addLtc" | "addMtc" }
  | { op: "removeLtc" | "removeMtc"; slot: number }
  | { op: "setLtcLevel"; slot: number; text: string }
  | { op: "setMetronome"; enabled: boolean }
  | { op: "setMetronomeGain"; text: string };

export const outputSignalEdit = (project: Project, op: SignalOp) =>
  invoke<ProjectState>("output_signal_edit", { project, op });

/**
 * Saves to `path`, or asks for one when it is `null` (first save, or "Save as").
 * Resolves to the path written and the project metadata the backend recorded, or `null` when the
 * dialog is cancelled.
 */
export async function saveProject(
  project: Project,
  path: string | null,
): Promise<{ path: string; meta: Project["meta"] } | null> {
  const target = path ?? (await save({ filters: [SAVE_FILTER], defaultPath: `${project.name}.flx` }));
  if (target === null) return null;
  const saved = await invoke<Project>("project_save", { project, path: target });
  return { path: target, meta: saved.meta };
}

/** Asks for audio files and registers them; the assets are for the caller to add to its project. */
export async function importAudio(): Promise<AudioAsset[]> {
  const paths = await open({ multiple: true, filters: [AUDIO_FILTER] });
  if (paths === null) return [];
  const assets: AudioAsset[] = [];
  for (const path of paths) {
    assets.push(await invoke<AudioAsset>("project_import_audio", { path }));
  }
  return assets;
}
