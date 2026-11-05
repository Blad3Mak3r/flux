// What the File menu and the keyboard do to the open document. They read the latest state when
// they run, so none of them closes over a project that has since changed.

import type { Translate } from "../i18n/messages";
import {
  errorMessage,
  importAudio,
  newProject,
  openProjectAt,
  pickProjectPath,
  projectDocument,
  saveProject,
} from "../project/api";
import type { BackendDocument } from "../project/api";
import type { Project } from "../project/types";
import { currentFrame } from "../transport/store";
import { reconnect, seek } from "../transport/playback";
import { analyse, forgetAnalyses } from "./audio";
import { isDirty } from "./doc";
import { addAudioClip, addNote, emptyProject } from "./edit";
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
  // A reload leaves what was playing running in the backend, with nothing left to show or stop it:
  // opening the output again replaces it.
  reconnect(true);
}

let started: Promise<void> | null = null;

/**
 * Opens the window on a document: the one the backend kept while the window was reloaded, else the
 * show the welcome window asked for (`?open=` in the address), else a new one. The address keeps
 * its `?open=` on a reload, which is why the backend's copy, with the edits made since, comes
 * first. The editor window is built once per run, so a document the backend already holds is
 * always this window's own. Outside the desktop app there is no backend and the window keeps its
 * own. Once only.
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
    const path = new URLSearchParams(window.location.search).get("open");
    if (path) return openAt(path);
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

async function openAt(path: string): Promise<void> {
  try {
    const opened = await openProjectAt(path);
    load(opened, path);
    say("status.opened", { name: basename(path) });
    analyse(opened.assets);
  } catch (err) {
    fail(err);
  }
}

export const openDocument = () =>
  replaceDoc(async () => {
    try {
      const path = await pickProjectPath();
      if (path) await openAt(path);
    } catch (err) {
      fail(err);
    }
  });

export async function saveDocument(asNew: boolean): Promise<boolean> {
  try {
    const { project, path: current } = useDoc.getState();
    const saved = await saveProject(project, asNew ? null : current);
    if (saved === null) return false;
    dispatch({ type: "saved", path: saved.path, meta: saved.meta });
    say("status.saved", { name: basename(saved.path) });
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

/** Adds an empty note at the playhead, or at `frame` (absolute) when given, and selects it to be written. */
export function addNoteAt(t: Translate, frame = currentFrame(), trackId?: string): void {
  const { project } = useDoc.getState();
  const added = addNote(project, frame - project.timecode.start, t("track.kind.notes"), trackId);
  edit(added.project);
  select({ kind: "note", id: added.id });
  say("status.noteAdded");
}
