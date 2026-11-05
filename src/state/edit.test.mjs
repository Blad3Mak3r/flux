// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { docReducer, initialDoc, isDirty } from "./doc.ts";
import {
  addNote,
  emptyProject,
  findItem,
  moveItem,
  removeItem,
  setNoteLength,
  setTrackColor,
  setTrackGain,
  toggleMute,
  updateClip,
  updateNote,
} from "./edit.ts";

function project() {
  return {
    ...emptyProject("Show", "fps25"),
    tracks: [
      { id: "t1", name: "Backing", muted: false, kind: { type: "audio", clips: [] } },
      { id: "t2", name: "Notes", muted: false, kind: { type: "notes", notes: [] } },
    ],
  };
}

test("setTrackGain stores the fader on an audio track without touching the input", () => {
  const before = project();
  const after = setTrackGain(before, "t1", -6);
  assert.equal(after.tracks[0].gainDb, -6);
  assert.equal(before.tracks[0].gainDb, undefined);
  assert.equal(after.tracks[1], before.tracks[1]);
});

test("setTrackGain stores 0 dB as no value", () => {
  const down = setTrackGain(project(), "t1", -6);
  const reset = setTrackGain(down, "t1", 0);
  assert.equal("gainDb" in reset.tracks[0], false);
});

test("setTrackGain returns the same project when nothing changes or the track cannot have a fader", () => {
  const before = project();
  assert.equal(setTrackGain(before, "t1", 0), before);
  assert.equal(setTrackGain(before, "t2", -6), before);
  assert.equal(setTrackGain(before, "missing", -6), before);
});

test("undo and redo step a fader change", () => {
  let doc = initialDoc(project());
  const first = doc.project;
  doc = docReducer(doc, { type: "edit", project: setTrackGain(doc.project, "t1", -12) });
  assert.equal(doc.project.tracks[0].gainDb, -12);
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, first);
  doc = docReducer(doc, { type: "redo" });
  assert.equal(doc.project.tracks[0].gainDb, -12);
});

test("setTrackColor stores a colour, returns to the default with null, and keeps the input intact", () => {
  const before = project();
  const coloured = setTrackColor(before, "t1", "#66d5ba");
  assert.equal(coloured.tracks[0].color, "#66d5ba");
  assert.equal(before.tracks[0].color, undefined);
  assert.equal(coloured.tracks[1], before.tracks[1]);

  const reset = setTrackColor(coloured, "t1", null);
  assert.ok(!("color" in reset.tracks[0]));
});

test("setTrackColor leaves the project alone when nothing changes", () => {
  const before = project();
  assert.equal(setTrackColor(before, "t1", null), before);
  assert.equal(setTrackColor(before, "missing", "#66d5ba"), before);
});

test("a colour change is one undo step", () => {
  const before = project();
  let doc = docReducer(initialDoc(before), { type: "edit", project: setTrackColor(before, "t2", "#c3b0fd") });
  assert.equal(doc.project.tracks[1].color, "#c3b0fd");
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, before);
  doc = docReducer(doc, { type: "redo" });
  assert.equal(doc.project.tracks[1].color, "#c3b0fd");
});

function withItems() {
  const base = project();
  return {
    ...base,
    tracks: [
      { ...base.tracks[0], kind: { type: "audio", clips: [{ id: "c1", asset: "a", start: 0, gainDb: 0 }] } },
      { id: "t3", name: "Other", muted: false, kind: { type: "audio", clips: [{ id: "c2", asset: "b", start: 5, gainDb: 0 }] } },
      { ...base.tracks[1], kind: { type: "notes", notes: [{ id: "k1", at: 3, text: "Go" }] } },
    ],
  };
}

test("moving a clip keeps every other track as it was", () => {
  const before = withItems();
  const after = moveItem(before, "c1", 40);
  assert.equal(after.tracks[0].kind.clips[0].start, 40);
  assert.equal(after.tracks[1], before.tracks[1]);
  assert.equal(after.tracks[2], before.tracks[2]);
});

test("moving a note keeps the audio tracks as they were", () => {
  const before = withItems();
  const after = moveItem(before, "k1", 9);
  assert.equal(after.tracks[2].kind.notes[0].at, 9);
  assert.equal(after.tracks[0], before.tracks[0]);
  assert.equal(after.tracks[1], before.tracks[1]);
});

test("removing an item rebuilds only the track that held it", () => {
  const before = withItems();
  const after = removeItem(before, "c2");
  assert.deepEqual(after.tracks[1].kind.clips, []);
  assert.equal(after.tracks[0], before.tracks[0]);
  assert.equal(after.tracks[2], before.tracks[2]);
});

test("an edit that finds nothing to change returns the same project", () => {
  const before = withItems();
  assert.equal(removeItem(before, "missing"), before);
  assert.equal(updateClip(before, "missing", { start: 1 }), before);
  assert.equal(toggleMute(before, "missing"), before);
});

test("restore picks a document up with no history, and unsaved work is told from what it is compared to", () => {
  const saved = project();
  const edited = setTrackGain(saved, "t1", -6);
  let doc = docReducer(initialDoc(project()), { type: "restore", project: edited, path: "/show.fxtc", saved });
  assert.equal(doc.project, edited);
  assert.equal(doc.path, "/show.fxtc");
  assert.deepEqual([doc.past, doc.future, doc.audio], [[], [], {}]);
  assert.equal(isDirty(doc), true);

  doc = docReducer(initialDoc(project()), { type: "restore", project: saved, path: null, saved });
  assert.equal(isDirty(doc), false);
});

test("addNote creates a notes track when there is none and puts an empty note at the position", () => {
  const before = { ...project(), tracks: [project().tracks[0]] };
  const { project: after, id } = addNote(before, 120, "Notes");
  assert.equal(after.tracks.length, 2);
  assert.equal(after.tracks[1].kind.type, "notes");
  assert.deepEqual(after.tracks[1].kind.notes, [{ id, at: 120, text: "" }]);
  assert.equal(before.tracks.length, 1);
});

test("addNote uses the first notes track and leaves the other tracks as they were", () => {
  const before = project();
  const { project: after, id } = addNote(before, 10, "Notes");
  assert.equal(after.tracks.length, 2);
  assert.equal(after.tracks[1].kind.notes[0].id, id);
  assert.equal(after.tracks[0], before.tracks[0]);
});

test("addNote prefers the track it is given when that is a notes track", () => {
  const base = project();
  const two = { ...base, tracks: [...base.tracks, { id: "t9", name: "More", muted: false, kind: { type: "notes", notes: [] } }] };
  const { project: after, id } = addNote(two, 5, "Notes", "t9");
  assert.equal(after.tracks[2].kind.notes[0].id, id);
  assert.equal(after.tracks[1], two.tracks[1]);
});

test("addNote ignores a track id that is not a notes track", () => {
  const { project: after, id } = addNote(project(), 5, "Notes", "t1");
  assert.equal(after.tracks[1].kind.notes[0].id, id);
});

test("addNote never puts a note before the timeline origin", () => {
  const { project: after } = addNote(project(), -30, "Notes");
  assert.equal(after.tracks[1].kind.notes[0].at, 0);
});

test("updateNote changes the text and keeps the other notes", () => {
  const before = withItems();
  const after = updateNote(before, "k1", { text: "Lights up\nslowly" });
  assert.equal(after.tracks[2].kind.notes[0].text, "Lights up\nslowly");
  assert.equal(after.tracks[0], before.tracks[0]);
});

test("setNoteLength stores a length, and no time removes it", () => {
  const withLength = setNoteLength(withItems(), "k1", 50);
  assert.equal(withLength.tracks[2].kind.notes[0].length, 50);
  const without = setNoteLength(withLength, "k1", 0);
  assert.equal("length" in without.tracks[2].kind.notes[0], false);
});

test("setNoteLength on something that is not a note changes nothing", () => {
  const before = withItems();
  assert.equal(setNoteLength(before, "c1", 50), before);
});

test("findItem finds a note and says so", () => {
  const found = findItem(withItems(), "k1");
  assert.equal(found.kind, "note");
  assert.equal(found.note.text, "Go");
});
