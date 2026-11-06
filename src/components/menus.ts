// The items of every menu: the menu bar's and the ones a right click opens. They are built when the
// menu opens, from the latest state, and call the same functions the keyboard does.

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MessageKey, Translate } from "../i18n/messages";
import type { Cue } from "../live/api";
import type { AudioClip, Note, Track } from "../project/types";
import { addCueEvent, addCueTrack, removeCueEvent } from "../state/cues";
import { deleteSelection, deleteTrack, muteTrack, reorderTrack, setGain } from "../state/actions";
import { activeTimeline } from "../state/doc";
import { splitAtPlayhead } from "../state/clips";
import { findItem } from "../state/edit";
import { dispatch, useDoc } from "../state/store";
import { isPlaying } from "../transport/store";
import {
  addTimeline,
  nudgeTimeline,
  openTimelineSettings,
  selectTimeline,
  stepTimeline,
} from "../state/timelines";
import {
  askRemoveTimeline,
  openSetup,
  select,
  setView,
  toggleFollow,
  toggleInspector,
  toggleSnap,
  useUi,
  zoomBy,
} from "../state/ui";
import type { MenuItem } from "../state/ui";
import {
  addNoteAt,
  closeDocument,
  importAudioFiles,
  newDocument,
  openDocument,
  saveDocument,
} from "../state/workflows";
import { nominalFps } from "../timecode";
import { exportDiagnosticsFile, openLogFolder } from "../state/diagnostics";
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

/** The magnet, ticked while it is on. Every menu of the timeline offers it. */
const snapItem = (t: Translate): MenuItem => ({
  label: t("context.snap"),
  checked: useUi.getState().snap,
  run: toggleSnap,
});

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
    { label: t("menu.close"), shortcut: mod("W"), run: closeDocument },
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
    { label: t("menu.split"), shortcut: "S", disabled: ui.view !== "edit" || ui.selection?.kind !== "clip", run: () => void splitAtPlayhead() },
    { label: t("menu.delete"), shortcut: "Del", disabled: !ui.selection, run: deleteSelection },
    { label: t("menu.deselect"), shortcut: "Esc", disabled: !ui.selection, run: () => select(null) },
  ];
}

function trackMenu(t: Translate): MenuItem[] {
  const track = selectedTrack();
  const editing = state().ui.view === "edit";
  const index = track ? activeTimeline(state().doc).tracks.indexOf(track) : -1;
  const last = activeTimeline(state().doc).tracks.length - 1;
  return [
    { label: t("menu.addSong"), shortcut: "I", run: () => void importAudioFiles() },
    { label: t("menu.addNote"), shortcut: "N", disabled: !editing, run: () => addNoteAt(t) },
    { label: t("menu.addMidiTrack"), disabled: !editing, run: () => void addCueTrack("midi", t("track.kind.midi")) },
    { label: t("menu.addOscTrack"), disabled: !editing, run: () => void addCueTrack("osc", t("track.kind.osc")) },
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

/** What the Timeline menu of the bar offers, for the active timeline. */
function timelineBarMenu(t: Translate): MenuItem[] {
  const { project } = state().doc;
  const active = activeTimeline(state().doc).id;
  return [
    { label: t("menu.timeline.new"), run: () => void addTimeline(t) },
    { label: t("menu.timeline.rename"), run: () => void openTimelineSettings(active) },
    { label: t("menu.timeline.delete"), disabled: project.timelines.length === 1, run: () => askRemoveTimeline(active) },
    "-",
    ...moveItems(t, active),
    "-",
    { label: t("menu.timeline.previous"), shortcut: mod("PgUp"), run: () => void stepTimeline(-1) },
    { label: t("menu.timeline.next"), shortcut: mod("PgDn"), run: () => void stepTimeline(1) },
  ];
}

/** Every timeline with the active one ticked: the way to reach one that scrolled out of the strip. */
export function allTimelinesMenu(t: Translate): MenuItem[] {
  const doc = state().doc;
  const active = activeTimeline(doc).id;
  return doc.project.timelines.map((timeline) => ({
    label: timeline.name || t("timeline.default"),
    checked: timeline.id === active,
    run: () => void selectTimeline(timeline.id),
  }));
}

/** Move left and right, for whoever does not drag: disabled at the ends and while the show plays. */
function moveItems(t: Translate, id: string): MenuItem[] {
  const { timelines } = state().doc.project;
  const index = timelines.findIndex((timeline) => timeline.id === id);
  const locked = isPlaying();
  return [
    { label: t("timeline.moveLeft"), disabled: locked || index < 1, run: () => void nudgeTimeline(id, -1) },
    {
      label: t("timeline.moveRight"),
      disabled: locked || index === -1 || index === timelines.length - 1,
      run: () => void nudgeTimeline(id, 1),
    },
  ];
}

/** The right-click menu of a timeline tab. */
export function timelineMenu(t: Translate, id: string): MenuItem[] {
  return [
    { label: t("timeline.rename"), run: () => void openTimelineSettings(id) },
    ...moveItems(t, id),
    "-",
    { label: t("timeline.delete"), disabled: state().doc.project.timelines.length === 1, run: () => askRemoveTimeline(id) },
  ];
}

function viewMenu(t: Translate): MenuItem[] {
  const { view, collapsed, follow, detached } = state().ui;
  const locked = isPlaying();
  return [
    { label: t("view.edit"), shortcut: mod("1"), checked: view === "edit", disabled: locked && view !== "edit", run: () => setView("edit") },
    { label: t("view.live"), shortcut: mod("2"), checked: view === "live", disabled: locked && view !== "live", run: () => setView("live") },
    "-",
    { label: t("timeline.zoomIn"), shortcut: mod("="), run: () => zoomBy(ZOOM_STEP) },
    { label: t("timeline.zoomOut"), shortcut: mod("-"), run: () => zoomBy(1 / ZOOM_STEP) },
    "-",
    { label: t("inspector.legend"), shortcut: mod("B"), checked: !collapsed, run: toggleInspector },
    { label: t("timeline.follow"), checked: follow && !detached, run: toggleFollow },
    snapItem(t),
  ];
}

function transportMenu(t: Translate): MenuItem[] {
  const second = () => nominalFps(activeTimeline(useDoc.getState()).timecode.frameRate);
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

function helpMenu(t: Translate): MenuItem[] {
  return [
    { label: t("menu.openLogs"), run: () => void openLogFolder() },
    { label: t("menu.exportDiagnostics"), run: () => void exportDiagnosticsFile() },
  ];
}

export const BAR_MENUS: { id: string; label: MessageKey; items: (t: Translate) => MenuItem[] }[] = [
  { id: "file", label: "menu.file", items: fileMenu },
  { id: "edit", label: "menu.edit", items: editMenu },
  { id: "track", label: "menu.track", items: trackMenu },
  { id: "timeline", label: "menu.timeline", items: timelineBarMenu },
  { id: "view", label: "menu.view", items: viewMenu },
  { id: "transport", label: "menu.transport", items: transportMenu },
  { id: "help", label: "menu.help", items: helpMenu },
];

// ---- Right click ----

const deleteNote = (id: string) => () => {
  select({ kind: "note", id });
  deleteSelection();
};

/** An audio track: its clip and its header offer the same, the clip with a way to its start. */
export function audioMenu(t: Translate, track: Track, clip?: AudioClip): MenuItem[] {
  const { start } = activeTimeline(useDoc.getState()).timecode;
  return [
    ...(clip
      ? [
          { label: t("context.goToStart"), run: () => seek(start + clip.start) },
          { label: t("menu.split"), shortcut: "S", run: () => void splitAtPlayhead() },
        ]
      : []),
    muteItem(t, track),
    { label: t("context.resetVolume"), disabled: (track.gainDb ?? 0) === 0, run: () => setGain(track.id, 0) },
    "-",
    { label: t("context.deleteSong"), run: () => deleteTrack(track.id) },
    "-",
    snapItem(t),
  ];
}

export function noteMenu(t: Translate, track: Track, note: Note): MenuItem[] {
  const { start } = activeTimeline(useDoc.getState()).timecode;
  return [
    { label: t("context.goToNote"), run: () => seek(start + note.at) },
    muteItem(t, track),
    "-",
    { label: t("context.deleteNote"), run: deleteNote(note.id) },
    "-",
    snapItem(t),
  ];
}

export function notesTrackMenu(t: Translate, track: Track): MenuItem[] {
  return [
    { label: t("menu.addNote"), run: () => addNoteAt(t, currentFrame(), track.id) },
    muteItem(t, track),
    "-",
    { label: t("menu.deleteTrack"), run: () => deleteTrack(track.id) },
    "-",
    snapItem(t),
  ];
}

/** A MIDI or OSC track: its header offers an event at the playhead and the track's own actions. */
export function cueTrackMenu(t: Translate, track: Track): MenuItem[] {
  const kind = track.kind.type === "osc" ? "osc" : "midi";
  return [
    {
      label: t(kind === "osc" ? "context.addOscHere" : "context.addMidiHere"),
      run: () => void addCueEvent(kind, currentFrame(), t(`track.kind.${kind}`), track.id),
    },
    muteItem(t, track),
    "-",
    { label: t("menu.deleteTrack"), run: () => deleteTrack(track.id) },
    "-",
    snapItem(t),
  ];
}

/** One MIDI or OSC event on its lane. */
export function eventMenu(t: Translate, track: Track, id: string, at: number): MenuItem[] {
  const { start } = activeTimeline(useDoc.getState()).timecode;
  return [
    { label: t("context.goToEvent"), run: () => seek(start + at) },
    muteItem(t, track),
    "-",
    { label: t("context.deleteEvent"), run: () => void removeCueEvent(id) },
    "-",
    snapItem(t),
  ];
}

/** Empty timeline; on a notes, MIDI or OSC lane `here` is the frame under the pointer. */
export function emptyMenu(
  t: Translate,
  here?: { frame: number; trackId: string; kind: "notes" | "midi" | "osc" },
): MenuItem[] {
  return [
    here?.kind === "midi" || here?.kind === "osc"
      ? {
          label: t(here.kind === "osc" ? "context.addOscHere" : "context.addMidiHere"),
          run: () => void addCueEvent(here.kind as "midi" | "osc", here.frame, t(`track.kind.${here.kind as "midi" | "osc"}`), here.trackId),
        }
      : here
        ? { label: t("context.addNoteHere"), run: () => addNoteAt(t, here.frame, here.trackId) }
        : { label: t("menu.addNote"), run: () => addNoteAt(t) },
    { label: t("menu.importAudio"), run: () => void importAudioFiles() },
    "-",
    snapItem(t),
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
