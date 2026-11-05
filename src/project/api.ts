import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AudioAsset, FrameRate, Project } from "./types";

/** The document the backend holds for the window: the project, where it lives and how it started. */
export interface BackendDocument {
  project: Project;
  /** Path of the `.fxtc`, or `null` for a project that was never saved. */
  path: string | null;
  /** The project as last opened or saved, or as it started: the document is unsaved when it differs. */
  baseline: Project;
}

const PROJECT_FILTER = { name: "Flux Timecode project", extensions: ["fxtc"] };
const AUDIO_FILTER = {
  name: "Audio",
  extensions: ["wav", "aif", "aiff", "flac", "mp3", "ogg"],
};

/** Backend errors arrive as plain message strings. */
export function errorMessage(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

/** The document being edited, or `null` before one was started: what a reloaded window picks up. */
export function projectDocument(): Promise<BackendDocument | null> {
  return invoke<BackendDocument | null>("project_document");
}

/** Gives the backend the latest state of the project, so a reloaded window gets it back. */
export function syncProject(project: Project): Promise<void> {
  return invoke("project_sync", { project });
}

export function newProject(name: string, frameRate: FrameRate): Promise<Project> {
  return invoke<Project>("project_new", { name, frameRate });
}

/** Asks for a `.fxtc`. Resolves to `null` when the dialog is cancelled. */
export function pickProjectPath(): Promise<string | null> {
  return open({ filters: [PROJECT_FILTER] });
}

export function openProjectAt(path: string): Promise<Project> {
  return invoke<Project>("project_open", { path });
}

/**
 * Saves to `path`, or asks for one when it is `null` (first save, or "Save as").
 * Resolves to the path written and the project metadata the backend recorded, or `null` when the
 * dialog is cancelled.
 */
export async function saveProject(
  project: Project,
  path: string | null,
): Promise<{ path: string; meta: Project["meta"] } | null> {
  const target = path ?? (await save({ filters: [PROJECT_FILTER], defaultPath: `${project.name}.fxtc` }));
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
