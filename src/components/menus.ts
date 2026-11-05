// The items of every menu: the menu bar's and the ones a right click opens. They are built when the
// menu opens, from the latest state, and call the same functions the keyboard does.

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MessageKey, Translate } from "../i18n/messages";
import type { Cue } from "../live/api";
import type { AudioClip, Note, Track } from "../project/types";
import { deleteSelection, deleteTrack, muteTrack, reorderTrack, setGain } from "../state/actions";
import { findItem } from "../state/edit";
import { dispatch, useDoc } from "../state/store";
import { openSetup, select, setView, toggleFollow, toggleInspector, useUi, zoomBy } from "../state/ui";
import type { MenuItem } from "../state/ui";
import {
  addNoteAt,
  importAudioFiles,
  newDocument,
  openDocument,
  saveDocument,
} from "../state/workflows";
import { nominalFps } from "../timecode";
import { seek, stop, toggle } from "../transport/playback";
import { currentFrame } from "../transport/store";
import { hasNativeWindow } from "./WindowControls";

/** The modifier key as this platform prints it, for shortcut hints. */
export const MOD_KEY = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

const ZOOM_STEP = 1.25;
const mod = (keys: string) => `${MOD_KEY}+${keys}`;
const state = () => ({ doc: useDoc.getState(), ui: useUi.getState() });

/** The track of the selected clip or note, if anything is selected. */
function selectedTrack(): Track | null {
  const { doc, ui } = state();
  return (ui.selection && findItem(doc.project, ui.selection.id)?.track) || null;
}

const muteItem = (t: Translate, track: Track): MenuItem => ({
  label: t(track.muted ? "menu.unmuteTrack" : "menu.muteTrack"),
  run: () => muteTrack(track.id),
});

// ---- Menu bar ----

function fileMenu(t: Translate): MenuItem[] {
  return [
    { label: t("menu.new"), shortcut: mod("N"), run: () => newDocument(t) },
    { label: t("menu.open"), shortcut: mod("O"), run: openDocument },
    "-",
    { label: t("menu.save"), shortcut: mod("S"), run: () => void saveDocument(false) },
    { label: t("menu.saveAs"), shortcut: mod("⇧+S"), run: () => void saveDocument(true) },
    "-",
    {
      label: t("menu.import"),
      submenu: [
        { label: t("menu.audio"), shortcut: "I", run: () => void importAudioFiles() },
        { label: t("menu.video"), disabled: true, run: () => {} },
      ],
    },
    "-",
    { label: t("menu.setup"), shortcut: mod(","), run: () => openSetup() },
    ...(hasNativeWindow()
      ? ["-" as const, { label: t("menu.exit"), run: () => void getCurrentWindow().close() }]
      : []),
  ];
}

function editMenu(t: Translate): MenuItem[] {
  const { doc, ui } = state();
  return [
    { label: t("menu.undo"), shortcut: mod("Z"), disabled: doc.past.length === 0, run: () => dispatch({ type: "undo" }) },
    { label: t("menu.redo"), shortcut: mod("⇧+Z"), disabled: doc.future.length === 0, run: () => dispatch({ type: "redo" }) },
    "-",
    { label: t("menu.delete"), shortcut: "Del", disabled: !ui.selection, run: deleteSelection },
    { label: t("menu.deselect"), shortcut: "Esc", disabled: !ui.selection, run: () => select(null) },
  ];
}

function trackMenu(t: Translate): MenuItem[] {
  const track = selectedTrack();
  const editing = state().ui.view === "edit";
  const index = track ? state().doc.project.tracks.indexOf(track) : -1;
  const last = state().doc.project.tracks.length - 1;
  return [
    { label: t("menu.addSong"), shortcut: "I", run: () => void importAudioFiles() },
    { label: t("menu.addNote"), shortcut: "N", disabled: !editing, run: () => addNoteAt(t) },
    "-",
    {
      label: t(track?.muted ? "menu.unmuteTrack" : "menu.muteTrack"),
      disabled: !track,
      run: () => track && muteTrack(track.id),
    },
    {
      label: t("menu.moveTrackUp"),
      disabled: !editing || index < 1,
      run: () => track && reorderTrack(track.id, index - 1),
    },
    {
      label: t("menu.moveTrackDown"),
      disabled: !editing || index === -1 || index === last,
      run: () => track && reorderTrack(track.id, index + 1),
    },
    { label: t("menu.deleteTrack"), disabled: !track, run: () => track && deleteTrack(track.id) },
  ];
}

function viewMenu(t: Translate): MenuItem[] {
  const { view, collapsed, follow, detached } = state().ui;
  return [
    { label: t("view.edit"), shortcut: mod("1"), checked: view === "edit", run: () => setView("edit") },
    { label: t("view.live"), shortcut: mod("2"), checked: view === "live", run: () => setView("live") },
    "-",
    { label: t("timeline.zoomIn"), shortcut: mod("="), run: () => zoomBy(ZOOM_STEP) },
    { label: t("timeline.zoomOut"), shortcut: mod("-"), run: () => zoomBy(1 / ZOOM_STEP) },
    "-",
    { label: t("inspector.legend"), shortcut: mod("B"), checked: !collapsed, run: toggleInspector },
    { label: t("timeline.follow"), checked: follow && !detached, run: toggleFollow },
  ];
}

function transportMenu(t: Translate): MenuItem[] {
  const second = () => nominalFps(useDoc.getState().project.timecode.frameRate);
  const by = (frames: () => number) => () => seek(currentFrame() + frames());
  return [
    { label: t("menu.playPause"), shortcut: "Space", run: () => void toggle() },
    { label: t("menu.stop"), shortcut: "Home", run: () => void stop() },
    "-",
    { label: t("menu.frameBack"), shortcut: "←", run: by(() => -1) },
    { label: t("menu.frameForward"), shortcut: "→", run: by(() => 1) },
    { label: t("menu.secondBack"), shortcut: "⇧+←", run: by(() => -second()) },
    { label: t("menu.secondForward"), shortcut: "⇧+→", run: by(second) },
  ];
}

export const BAR_MENUS: { id: string; label: MessageKey; items: (t: Translate) => MenuItem[] }[] = [
  { id: "file", label: "menu.file", items: fileMenu },
  { id: "edit", label: "menu.edit", items: editMenu },
  { id: "track", label: "menu.track", items: trackMenu },
  { id: "view", label: "menu.view", items: viewMenu },
  { id: "transport", label: "menu.transport", items: transportMenu },
];

// ---- Right click ----

const deleteNote = (id: string) => () => {
  select({ kind: "note", id });
  deleteSelection();
};

/** An audio track: its clip and its header offer the same, the clip with a way to its start. */
export function audioMenu(t: Translate, track: Track, clip?: AudioClip): MenuItem[] {
  const { start } = useDoc.getState().project.timecode;
  return [
    ...(clip ? [{ label: t("context.goToStart"), run: () => seek(start + clip.start) }] : []),
    muteItem(t, track),
    { label: t("context.resetVolume"), disabled: (track.gainDb ?? 0) === 0, run: () => setGain(track.id, 0) },
    "-",
    { label: t("context.deleteSong"), run: () => deleteTrack(track.id) },
  ];
}

export function noteMenu(t: Translate, track: Track, note: Note): MenuItem[] {
  const { start } = useDoc.getState().project.timecode;
  return [
    { label: t("context.goToNote"), run: () => seek(start + note.at) },
    muteItem(t, track),
    "-",
    { label: t("context.deleteNote"), run: deleteNote(note.id) },
  ];
}

export function notesTrackMenu(t: Translate, track: Track): MenuItem[] {
  return [
    { label: t("menu.addNote"), run: () => addNoteAt(t, currentFrame(), track.id) },
    muteItem(t, track),
    "-",
    { label: t("menu.deleteTrack"), run: () => deleteTrack(track.id) },
  ];
}

/** Empty timeline; on a notes lane `here` is the frame under the pointer. */
export function emptyMenu(t: Translate, here?: { frame: number; trackId: string }): MenuItem[] {
  return [
    here
      ? { label: t("context.addNoteHere"), run: () => addNoteAt(t, here.frame, here.trackId) }
      : { label: t("menu.addNote"), run: () => addNoteAt(t) },
    { label: t("menu.importAudio"), run: () => void importAudioFiles() },
  ];
}

export const cueMenu = (t: Translate, cue: Cue): MenuItem[] => [
  { label: t("context.goToCue"), run: () => seek(cue.frame) },
];

export function textMenu(t: Translate, field: HTMLInputElement | HTMLTextAreaElement): MenuItem[] {
  const selected = (field.selectionStart ?? 0) !== (field.selectionEnd ?? 0);
  const editable = !field.readOnly && !field.disabled;
  return [
    { label: t("context.cut"), disabled: !selected || !editable, run: () => document.execCommand("cut") },
    { label: t("context.copy"), disabled: !selected, run: () => document.execCommand("copy") },
    {
      label: t("context.paste"),
      disabled: !editable,
      // The webview may refuse to read the clipboard; Ctrl+V still pastes then.
      run: () =>
        void navigator.clipboard
          .readText()
          .then((text) => document.execCommand("insertText", false, text))
          .catch(() => undefined),
    },
    "-",
    { label: t("context.selectAll"), run: () => field.select() },
  ];
}
