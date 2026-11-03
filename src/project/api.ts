import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AudioAsset, FrameRate, Project } from "./types";

const PROJECT_FILTER = { name: "Flux Timecode project", extensions: ["fxtc"] };
const AUDIO_FILTER = {
  name: "Audio",
  extensions: ["wav", "aif", "aiff", "flac", "mp3", "ogg"],
};

/** Backend errors arrive as plain message strings. */
export function errorMessage(err: unknown): string {
  return typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
}

export function newProject(name: string, frameRate: FrameRate): Promise<Project> {
  return invoke<Project>("project_new", { name, frameRate });
}

/** Asks for a `.fxtc` and opens it. Resolves to `null` when the dialog is cancelled. */
export async function openProject(): Promise<{ path: string; project: Project } | null> {
  const path = await open({ filters: [PROJECT_FILTER] });
  if (path === null) return null;
  return { path, project: await invoke<Project>("project_open", { path }) };
}

/**
 * Saves to `path`, or asks for one when it is `null` (first save, or "Save as").
 * Resolves to the path written, or `null` when the dialog is cancelled.
 */
export async function saveProject(project: Project, path: string | null): Promise<string | null> {
  const target = path ?? (await save({ filters: [PROJECT_FILTER], defaultPath: `${project.name}.fxtc` }));
  if (target === null) return null;
  await invoke("project_save", { project, path: target });
  return target;
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
