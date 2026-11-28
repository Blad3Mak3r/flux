// What the user does on the media page. The backend finds, relinks and copies; the project it
// answers with enters the undo history, and what can be read now is read.

import { fail } from "../state/fail";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { Project } from "../project/types";
import { analyse } from "../state/audio";
import { dispatch, useDoc } from "../state/store";
import { say } from "../state/ui";
import { probeVideos } from "../state/video";
import { COLLECT_PROGRESS, mediaCollect, mediaRelink, mediaSearchFolder, pickFolder, pickMediaFile } from "./api";
import type { CollectProgress, MediaItem } from "./api";
import { refreshMedia, useMedia } from "./store";

/** Runs a backend search, relink or copy: the project it answers with enters the undo history. */
async function run<T extends { state: { project: Project; active: string } }>(work: () => Promise<T>): Promise<T | undefined> {
  useMedia.setState({ busy: true });
  const before = useMedia.getState().offline;
  try {
    const done = await work();
    const { project, active } = done.state;
    dispatch({ type: "apply", project, active });
    await refreshMedia();
    // What was offline and is there now has not been read yet.
    const { offline } = useMedia.getState();
    const found = (asset: { id: string }) => before.has(asset.id) && !offline.has(asset.id);
    void probeVideos((project.videos ?? []).filter(found));
    void analyse(project.assets.filter(found));
    return done;
  } catch (err) {
    fail(err);
  } finally {
    useMedia.setState({ busy: false, progress: null });
  }
}

/** Asks for the file of the media `item` and links it, and the other missing files beside it. */
export async function relinkMedia(item: MediaItem): Promise<void> {
  const path = await pickMediaFile(item.kind);
  if (path === null) return;
  const done = await run(() => mediaRelink(useDoc.getState().project, item.id, path));
  if (done) say("media.relinked", { count: done.found });
}

/** Asks for a folder and links every missing file found by name in it or under it. */
export async function searchMediaFolder(): Promise<void> {
  const folder = await pickFolder();
  if (folder === null) return;
  const done = await run(() => mediaSearchFolder(useDoc.getState().project, folder));
  if (done) say("media.searched", { found: done.found, missing: done.missing });
}

/** Gathers the media beside the project file; the copy of a big video takes a while. */
export async function collectMedia(): Promise<void> {
  const listening = listen<CollectProgress>(COLLECT_PROGRESS, ({ payload }) => useMedia.setState({ progress: payload }));
  try {
    const done = await run(() => mediaCollect(useDoc.getState().project));
    if (done) say("media.collected", { copied: done.copied, skipped: done.skipped });
  } finally {
    void listening.then((stop) => stop());
  }
}

/** Shows the file in the system's file manager. */
export function revealMedia(path: string): void {
  revealItemInDir(path).catch(fail);
}
