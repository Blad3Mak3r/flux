// Files dragged in from the file explorer. The webview reports them with their paths and the
// pointer's position in physical pixels; this finds what is under the pointer by the `data-drop-*`
// attributes of the page, lights it, and on the drop hands the paths and the place to the backend,
// which decides which files it takes and where they go.

import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { create } from "zustand";
import { errorMessage } from "../project/api";
import type { AudioAsset, ProjectState, VideoAsset } from "../project/types";
import { analyse, detectTempos } from "./audio";
import { snapFrame, snapTargets } from "./snap";
import { dispatch, useDoc } from "./store";
import { say, sayError, useUi } from "./ui";
import { probeVideos } from "./video";

/** Where a file would land: a cell of the VJ grid, or a time on the timeline and the track it is on, if any. */
interface Target {
  cell: { layer: string; column: string } | null;
  timeline: { track: string | null; frame: number } | null;
}

/** What the page lights while files are dragged over it. */
export const useFileDrop = create<Target>(() => ({ cell: null, timeline: null }));

const NONE: Target = { cell: null, timeline: null };

function targetAt(x: number, y: number): Target {
  const el = document.elementFromPoint(x, y);
  const cell = el?.closest<HTMLElement>("[data-drop-column]");
  if (cell?.dataset.dropLayer && cell.dataset.dropColumn) {
    return { cell: { layer: cell.dataset.dropLayer, column: cell.dataset.dropColumn }, timeline: null };
  }
  const timeline = el?.closest("[data-drop-timeline]");
  if (!el || !timeline) return NONE;
  const left = timeline.getBoundingClientRect().left;
  const frame = Math.max(0, snapFrame(Math.round((x - left) / useUi.getState().zoom), snapTargets()));
  const track = el.closest<HTMLElement>("[data-drop-track]")?.dataset.dropTrack ?? null;
  return { cell: null, timeline: { track, frame } };
}

type Placed = ProjectState & { skipped: number };

async function drop(paths: string[], target: Target): Promise<void> {
  const { project } = useDoc.getState();
  try {
    const placed = target.cell
      ? await invoke<Placed>("project_drop_vj", { project, paths, layer: target.cell.layer, column: target.cell.column })
      : target.timeline
        ? await invoke<Placed>("project_drop_timeline", { project, paths, track: target.timeline.track, at: target.timeline.frame })
        : null;
    if (!placed) return;
    dispatch({ type: "apply", project: placed.project, active: placed.active });
    const fresh = <T extends { id: string }>(now: T[] | undefined, before: T[] | undefined) =>
      (now ?? []).filter((item) => !before?.some((old) => old.id === item.id));
    const videos: VideoAsset[] = fresh(placed.project.videos, project.videos);
    const audio: AudioAsset[] = fresh(placed.project.assets, project.assets);
    if (placed.skipped > 0) say("status.dropSkipped", { count: placed.skipped });
    else if (videos.length + audio.length === 1) say("status.imported", { name: (videos[0] ?? audio[0]).name });
    else say("status.importedMany", { count: videos.length + audio.length });
    void probeVideos(videos);
    // The tempo is read from the decoded audio, which the analysis leaves in the backend's cache.
    if (audio.length > 0) void analyse(audio).then(() => detectTempos(audio));
  } catch (err) {
    sayError(errorMessage(err));
  }
}

/** Starts listening to files dropped on the window; the answer stops it. Does nothing outside the desktop app. */
export async function watchFileDrops(): Promise<() => void> {
  if (!("__TAURI_INTERNALS__" in window)) return () => undefined;
  const ratio = () => window.devicePixelRatio || 1;
  return getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === "leave") {
      useFileDrop.setState(NONE);
      return;
    }
    const target = targetAt(payload.position.x / ratio(), payload.position.y / ratio());
    if (payload.type === "drop") {
      useFileDrop.setState(NONE);
      void drop(payload.paths, target);
    } else useFileDrop.setState(target);
  });
}

