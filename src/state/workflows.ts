// What the File menu and the keyboard do to the open document. They read the latest state when
// they run, so none of them closes over a project that has since changed.

import { fail } from "./fail";
import { protectVoiceWork } from "../voice/store";
import type { Translate } from "../i18n/messages";
import {
  importAudio,
  importVideo,
  pickVideoPaths,
  newProject,
  openProjectAt,
  pickProjectPath,
  projectDocument,
  saveProject,
} from "../project/api";
import type { BackendDocument } from "../project/api";
import type { Project, ProjectMode, ProjectState } from "../project/types";
import { closeEditor } from "../welcome/api";
import { vjTemplateNew } from "../vj/api";
import { flushDocumentSync } from "./persist";
import { currentFrame } from "../transport/store";
import { seek, startTransport } from "../transport/playback";
import { analyse, detectTempos, forgetAnalyses } from "./audio";
import { probeVideos } from "./video";
import { activeTimeline, isDirty } from "./doc";
import { editItem } from "./items";
import { dispatch, useDoc } from "./store";
import { ask, resetView, say, select } from "./ui";

const basename = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** Replace the document, asking first when there is work to lose. */
const replaceDoc = async (action: () => Promise<void> | void) => {
  if (!await protectVoiceWork()) return;
  if (isDirty(useDoc.getState())) ask(action); else await action();
};

function load({ project, active }: ProjectState, path: string | null): void {
  forgetAnalyses();
  dispatch({ type: "load", project, path, active });
  resetView();
  seek(activeTimeline(useDoc.getState()).timecode.start);
}

/** The same content, whether or not it is the same object: what unsaved work is told by identity. */
const sameProject = (a: Project, b: Project) => a === b || JSON.stringify(a) === JSON.stringify(b);

function restore({ project, path, baseline, active }: BackendDocument): Project {
  forgetAnalyses();
  dispatch({ type: "restore", project, path, active, saved: sameProject(project, baseline) ? project : baseline });
  resetView();
  say("status.restored", { name: project.name });
  return project;
}

export type EditorPhase = "project" | "audio" | "outputs";

let started: Promise<void> | null = null;

/**
 * Opens the window on a document: the one the backend kept while the window was reloaded, else the
 * show the welcome window asked for (`?open=` in the address), else a new one. The address keeps
 * its `?open=` on a reload, which is why the backend's copy, with the edits made since, comes
 * first. The editor window is built once per run, so a document the backend already holds is
 * always this window's own. Outside the desktop app there is no backend and the window keeps its
 * own. Once only.
 */
export function startDocument(t: Translate, phase: (value: EditorPhase) => void): Promise<void> {
  started ??= (async () => {
    let found: BackendDocument | null;
    try {
      found = await projectDocument();
    } catch (err) {
      fail(err);
      return;
    }
    let project: Project | undefined;
    if (found) {
      project = restore(found);
    } else {
      const query = new URLSearchParams(window.location.search);
      const path = query.get("open");
      if (path) project = await openAt(path);
      else {
        const mode = query.get("new") === "vj" ? "vj" : "timeline";
        const template = query.get("template");
        const fresh = template
          ? await vjTemplateNew(template, t("project.untitled"))
          : await newProject(t("project.untitled"), "fps25", mode);
        load(fresh, null);
      }
    }
    if (project?.assets.length || project?.videos?.length) {
      phase("audio");
      await prepareAudio(project);
    }
  })().catch(fail).then(() => {
    phase("outputs");
    return startTransport();
  }).catch(fail);
  return started;
}

/** A new show of the kind `mode`: a cue show with a timeline, or a VJ show with a grid. */
export const newDocument = (t: Translate, mode: ProjectMode = "timeline") =>
  replaceDoc(async () => {
    await flushDocumentSync();
    try {
      load(await newProject(t("project.untitled"), "fps25", mode), null);
      say("status.ready");
    } catch (err) {
      fail(err);
    }
  });

/** A new VJ show with the structure of the template `id` (see `vj_presets`). */
export const newFromTemplate = (t: Translate, id: string) =>
  replaceDoc(async () => {
    try {
      await flushDocumentSync();
      load(await vjTemplateNew(id, t("project.untitled")), null);
      say("status.ready");
    } catch (err) {
      fail(err);
    }
  });

async function openAt(path: string): Promise<Project | undefined> {
  try {
    await flushDocumentSync();
    const opened = await openProjectAt(path);
    load(opened, path);
    say("status.opened", { name: basename(path) });
    return opened.project;
  } catch (err) {
    fail(err);
  }
}

async function prepareAudio(project: Project): Promise<void> {
  void probeVideos(project.videos ?? []);
  await analyse(project.assets);
  await detectTempos(project.assets, true);
}

export const openDocument = () =>
  replaceDoc(async () => {
    try {
      const path = await pickProjectPath();
      const project = path && await openAt(path);
      if (project) void prepareAudio(project);
    } catch (err) {
      fail(err);
    }
  });

/** Returns to the welcome window after the shared unsaved-changes flow has finished. */
export const closeDocument = () => replaceDoc(async () => {
  await flushDocumentSync();
  await closeEditor();
});

export async function saveDocument(asNew: boolean): Promise<boolean> {
  try {
    await flushDocumentSync();
    const { project, path: current } = useDoc.getState();
    const saved = await saveProject(project, asNew ? null : current);
    if (saved === null) return false;
    dispatch({ type: "saved", path: saved.path, meta: saved.meta, links: saved.links, snapshot: project });
    say("status.saved", { name: basename(saved.path) });
    return !isDirty(useDoc.getState());
  } catch (err) {
    fail(err);
    return false;
  }
}

export async function importAudioFiles(): Promise<void> {
  try {
    const assets = await importAudio();
    if (assets.length === 0) return;
    const { timecode } = activeTimeline(useDoc.getState());
    const last = await editItem({ op: "addAudioClips", assets, at: currentFrame() - timecode.start });
    if (last === null) return;
    for (const asset of assets) say("status.imported", { name: asset.name });
    // The tempo is read from the decoded audio, which the analysis leaves in the backend's cache.
    void analyse(assets).then(() => detectTempos(assets));
    select({ kind: "clip", id: last });
  } catch (err) {
    fail(err);
  }
}

/** Asks for video files and adds each on a new track at the playhead; the backend reads and places them. */
export async function importVideoFiles(): Promise<void> {
  try {
    for (const path of await pickVideoPaths()) {
      const doc = useDoc.getState();
      const at = Math.max(0, currentFrame() - activeTimeline(doc).timecode.start);
      const { project, active } = await importVideo(doc.project, path, at);
      dispatch({ type: "apply", project, active });
      const added = (project.videos ?? []).filter((video) => !doc.project.videos?.some((old) => old.id === video.id));
      for (const video of added) say("status.imported", { name: video.name });
      void probeVideos(added);
    }
  } catch (err) {
    fail(err);
  }
}

/** Adds an empty note at the playhead, or at `frame` (absolute) when given, and selects it to be written. */
export async function addNoteAt(t: Translate, frame = currentFrame(), trackId?: string): Promise<void> {
  const { timecode } = activeTimeline(useDoc.getState());
  const id = await editItem({ op: "addNote", trackId, at: frame - timecode.start, trackName: t("track.kind.notes") });
  if (id === null) return;
  select({ kind: "note", id });
  say("status.noteAdded");
}
