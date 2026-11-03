// What the File menu and the keyboard do to the open document. They read the latest state when
// they run, so none of them closes over a project that has since changed.

import type { Translate } from "../i18n/messages";
import { errorMessage, importAudio, newProject, openProject, projectDocument, saveProject } from "../project/api";
import type { BackendDocument } from "../project/api";
import type { Project } from "../project/types";
import { currentFrame } from "../transport/store";
import { seek, stopOutput } from "../transport/playback";
import { analyse, forgetAnalyses } from "./audio";
import { isDirty } from "./doc";
import { addAudioClip, addMarker, emptyProject } from "./edit";
import { edit } from "./actions";
import { dispatch, useDoc } from "./store";
import { ask, resetView, say, sayError, select } from "./ui";

const basename = (path: string) => path.split(/[\\/]/).pop() ?? path;
const fail = (err: unknown) => sayError(errorMessage(err));

/** Replace the document, asking first when there is work to lose. */
const replaceDoc = (action: () => Promise<void> | void) =>
  isDirty(useDoc.getState()) ? ask(action) : void action();

function load(next: Project, path: string | null): void {
  forgetAnalyses();
  dispatch({ type: "load", project: next, path });
  resetView();
  seek(next.timecode.start);
}

/** The same content, whether or not it is the same object: what unsaved work is told by identity. */
const sameProject = (a: Project, b: Project) => a === b || JSON.stringify(a) === JSON.stringify(b);

function restore({ project, path, baseline }: BackendDocument): void {
  forgetAnalyses();
  dispatch({ type: "restore", project, path, saved: sameProject(project, baseline) ? project : baseline });
  resetView();
  seek(project.timecode.start);
  say("status.restored", { name: project.name });
  analyse(project.assets);
  // A reload leaves what was playing running in the backend, with nothing left to show or stop it.
  void stopOutput();
}

let started: Promise<void> | null = null;

/**
 * Opens the window on the backend's document: the one it kept while the window was reloaded, or a
 * new one. Outside the desktop app there is no backend and the window keeps its own. Once only.
 */
export function startDocument(t: Translate): Promise<void> {
  started ??= (async () => {
    let found: BackendDocument | null;
    try {
      found = await projectDocument();
    } catch {
      return;
    }
    if (found) return restore(found);
    const fresh = await newProject(t("project.untitled"), "fps25").catch(() => null);
    if (fresh) load(fresh, null);
  })();
  return started;
}

export const newDocument = (t: Translate) =>
  replaceDoc(async () => {
    const fresh = await newProject(t("project.untitled"), "fps25").catch(() =>
      emptyProject(t("project.untitled"), "fps25"),
    );
    load(fresh, null);
    say("status.ready");
  });

export const openDocument = () =>
  replaceDoc(async () => {
    try {
      const opened = await openProject();
      if (!opened) return;
      load(opened.project, opened.path);
      say("status.opened", { name: basename(opened.path) });
      analyse(opened.project.assets);
    } catch (err) {
      fail(err);
    }
  });

export async function saveDocument(asNew: boolean): Promise<boolean> {
  try {
    const { project, path: current } = useDoc.getState();
    const path = await saveProject(project, asNew ? null : current);
    if (path === null) return false;
    dispatch({ type: "saved", path });
    say("status.saved", { name: basename(path) });
    return true;
  } catch (err) {
    fail(err);
    return false;
  }
}

export async function importAudioFiles(): Promise<void> {
  try {
    const assets = await importAudio();
    if (assets.length === 0) return;
    let next = useDoc.getState().project;
    const { start } = next.timecode;
    let last = "";
    for (const asset of assets) {
      const added = addAudioClip(next, asset, currentFrame() - start);
      next = added.project;
      last = added.id;
      say("status.imported", { name: asset.name });
    }
    edit(next);
    analyse(assets);
    select({ kind: "clip", id: last });
  } catch (err) {
    fail(err);
  }
}

export function addMarkerAtPlayhead(t: Translate): void {
  const { project } = useDoc.getState();
  const added = addMarker(project, currentFrame() - project.timecode.start, (n) => `Marker ${n}`, t("track.kind.events"));
  edit(added.project);
  select({ kind: "trigger", id: added.id });
  say("status.markerAdded");
}
