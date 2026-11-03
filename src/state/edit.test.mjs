// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { docReducer, initialDoc } from "./doc.ts";
import { emptyProject, setTrackColor, setTrackGain } from "./edit.ts";

function project() {
  return {
    ...emptyProject("Show", "fps25"),
    tracks: [
      { id: "t1", name: "Backing", muted: false, kind: { type: "audio", clips: [] } },
      { id: "t2", name: "Cues", muted: false, kind: { type: "events", triggers: [] } },
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
