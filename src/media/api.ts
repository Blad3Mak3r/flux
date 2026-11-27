// The project's media files: which are linked, which are offline, and finding or gathering them.
// Mirrors `flux-project/src/media.rs` and `src-tauri/src/media_commands.rs`.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { Project, ProjectState } from "../project/types";

export type MediaKind = "audio" | "video" | "image";
/** `embedded` lives inside the project file (older projects); the others are linked by path. */
export type MediaStatus = "online" | "offline" | "embedded";

export interface MediaItem {
  id: string;
  name: string;
  kind: MediaKind;
  state: MediaStatus;
  path: string | null;
  /** Clips (and test cards) that show it. */
  uses: number;
}

export interface MediaList {
  items: MediaItem[];
  missing: number;
}

export interface Relinked {
  state: ProjectState;
  found: number;
  missing: number;
}

export interface Collected {
  state: ProjectState;
  copied: number;
  skipped: number;
}

export interface CollectProgress {
  doneBytes: number;
  totalBytes: number;
}

export const COLLECT_PROGRESS = "media-collect-progress";

const EXTENSIONS: Record<MediaKind, string[]> = {
  audio: ["wav", "aif", "aiff", "flac", "mp3", "ogg"],
  video: ["mp4", "m4v", "mov", "webm"],
  image: ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
};

export const mediaList = (project: Project) => invoke<MediaList>("media_list", { project });

export const mediaRelink = (project: Project, id: string, path: string) =>
  invoke<Relinked>("media_relink", { project, id, path });

export const mediaSearchFolder = (project: Project, folder: string) =>
  invoke<Relinked>("media_search_folder", { project, folder });

export const mediaCollect = (project: Project) => invoke<Collected>("media_collect", { project });

/** Asks for the file that replaces a missing one of `kind`; `null` when the dialog is cancelled. */
export async function pickMediaFile(kind: MediaKind): Promise<string | null> {
  const picked = await open({ filters: [{ name: kind, extensions: EXTENSIONS[kind] }] });
  return typeof picked === "string" ? picked : null;
}

/** Asks for the folder to search for missing files; `null` when the dialog is cancelled. */
export async function pickFolder(): Promise<string | null> {
  const picked = await open({ directory: true });
  return typeof picked === "string" ? picked : null;
}
