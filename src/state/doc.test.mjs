// Run with `npm test`. The edits of a project are the backend's (see `item_ops.rs`, `track_ops.rs`
// and their tests); this covers what the window does with the project it answers with.
import assert from "node:assert/strict";
import { test } from "node:test";
import { dropIndex, dropLineY } from "../timeline/reorder.ts";
import { activeTimeline, docReducer, initialDoc, isDirty, placeholderProject } from "./doc.ts";

/** The id of the timeline every test project has. */
const TL = "l1";

/** The project with these tracks in its timeline. */
const withTracks = (base, tracks) => ({ ...base, timelines: [{ ...base.timelines[0], id: TL, tracks }] });

function project() {
  return withTracks(placeholderProject("Show"), [
    { id: "t1", name: "Backing", muted: false, kind: { type: "audio", clips: [] } },
    { id: "t2", name: "Notes", muted: false, kind: { type: "notes", notes: [] } },
  ]);
}

/** What the backend answers to a fader change. */
const withGain = (base, gainDb) => withTracks(base, [{ ...base.timelines[0].tracks[0], gainDb }, base.timelines[0].tracks[1]]);

test("undo and redo step a change the backend answered", () => {
  let doc = initialDoc(project());
  const first = doc.project;
  doc = docReducer(doc, { type: "apply", project: withGain(doc.project, -12), active: TL });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -12);
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, first);
  doc = docReducer(doc, { type: "redo" });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -12);
});

test("saving keeps the project and takes the metadata the backend wrote", () => {
  const meta = { ...project().meta, modifiedAt: "2030-01-01T00:00:00Z", savedWith: "9.9.9" };
  let doc = docReducer(initialDoc(project()), { type: "apply", project: withGain(project(), -6), active: TL });
  assert.equal(isDirty(doc), true);
  doc = docReducer(doc, { type: "saved", path: "/show.fxtc", meta });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -6);
  assert.deepEqual(doc.project.meta, meta);
  assert.equal(isDirty(doc), false);
});

test("restore picks a document up with no history, and unsaved work is told from what it is compared to", () => {
  const saved = project();
  const edited = withGain(saved, -6);
  let doc = docReducer(initialDoc(project()), { type: "restore", project: edited, path: "/show.fxtc", saved, active: TL });
  assert.equal(doc.project, edited);
  assert.equal(doc.path, "/show.fxtc");
  assert.deepEqual([doc.past, doc.future, doc.audio], [[], [], {}]);
  assert.equal(isDirty(doc), true);

  doc = docReducer(initialDoc(project()), { type: "restore", project: saved, path: null, saved, active: TL });
  assert.equal(isDirty(doc), false);
});

test("saving a snapshot keeps edits made during the save dirty", () => {
  const snapshot = placeholderProject("Before save");
  const edited = { ...snapshot, name: "During save" };
  const doc = { ...initialDoc(snapshot), project: edited };
  const result = docReducer(doc, { type: "saved", path: "/show.fxtc", meta: snapshot.meta, snapshot });
  assert.equal(result.project.name, "During save");
  assert.equal(result.saved.name, "Before save");
  assert.equal(isDirty(result), true);
});

test("a recovered project keeps its options and unsaved baseline through undo and redo", () => {
  const baseline = placeholderProject("Show");
  const recovered = { ...baseline, autosave: { enabled: false, intervalSeconds: 60 } };
  const doc = docReducer(initialDoc(baseline), { type: "restore", project: recovered, path: null, saved: baseline, active: baseline.timelines[0].id });
  assert.equal(isDirty(doc), true);
  const applied = docReducer(doc, { type: "apply", project: { ...recovered, autosave: baseline.autosave }, active: doc.active });
  const undone = docReducer(applied, { type: "undo" });
  assert.deepEqual(undone.project.autosave, recovered.autosave);
  assert.deepEqual(docReducer(undone, { type: "redo" }).project.autosave, baseline.autosave);
});

test("dropIndex follows the middle of the dragged row past the middles of the others", () => {
  const heights = [96, 64, 96];
  assert.equal(dropIndex(heights, 0, 0), 0);
  assert.equal(dropIndex(heights, 0, 90), 1); // middle at 138, past the 128 of row 1
  assert.equal(dropIndex(heights, 0, 400), 2);
  assert.equal(dropIndex(heights, 2, -400), 0);
});

test("dropLineY marks the edge of the row the dragged one lands against", () => {
  const heights = [96, 64, 96];
  assert.equal(dropLineY(heights, 0, 2), 256); // below the last row
  assert.equal(dropLineY(heights, 2, 0), 0); // above the first
  assert.equal(dropLineY(heights, 2, 1), 96);
});

/** The project with a second timeline, `l2`. */
function twoTimelines() {
  const base = project();
  const second = { id: "l2", name: "Second", timecode: { frameRate: "fps30", start: 0 }, tracks: [] };
  return { ...base, timelines: [...base.timelines, second] };
}

test("apply puts what the backend answered in the history, with its active timeline", () => {
  const first = twoTimelines();
  const answered = { ...first, timelines: [...first.timelines, { id: "l3", name: "Third", timecode: { frameRate: "fps25", start: 0 }, tracks: [] }] };
  let doc = initialDoc(first);
  doc = docReducer(doc, { type: "apply", project: answered, active: "l3" });
  assert.equal(activeTimeline(doc).name, "Third");
  assert.equal(doc.past.length, 1);

  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, first);
  assert.equal(doc.active, "l3");
});

test("a controller sweep is one undo step, closed by its commit or by anything done meanwhile", () => {
  const first = twoTimelines();
  const at = (value) => ({ ...first, name: `moved ${value}` });
  let doc = initialDoc(first);
  doc = docReducer(doc, { type: "controlled", project: at(1), active: doc.active, commit: false });
  doc = docReducer(doc, { type: "controlled", project: at(2), active: doc.active, commit: false });
  assert.equal(doc.project.name, "moved 2");
  assert.equal(doc.past.length, 0, "moves enter no history");
  doc = docReducer(doc, { type: "controlled", project: at(3), active: doc.active, commit: true });
  assert.equal(doc.past.length, 1);
  assert.equal(doc.gesture, undefined);
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, first, "one undo takes the whole sweep back");

  let open = docReducer(initialDoc(first), { type: "controlled", project: at(1), active: "l1", commit: false });
  open = docReducer(open, { type: "apply", project: at(9), active: "l1" });
  assert.equal(open.past.length, 2, "an edit made during the gesture closes it first");
  assert.equal(open.past[0], first);
});

test("a discrete controller edit is one step of its own", () => {
  const first = twoTimelines();
  const doc = docReducer(initialDoc(first), { type: "controlled", project: { ...first, name: "x" }, active: "l1", commit: true });
  assert.equal(doc.past.length, 1);
  assert.equal(doc.past[0], first);
});

test("the active timeline falls back to the first while the backend names one that is gone", () => {
  let doc = docReducer(initialDoc(twoTimelines()), { type: "active", id: "l2" });
  assert.equal(activeTimeline(doc).id, "l2");
  doc = docReducer(doc, { type: "active", id: "gone" });
  assert.equal(activeTimeline(doc).id, "l1");
});

test("active with the same id returns the same document", () => {
  const doc = initialDoc(twoTimelines());
  assert.equal(docReducer(doc, { type: "active", id: doc.active }), doc);
});

test("a tempo found for an opened file is no edit: no history, and the document stays saved", () => {
  const opened = { ...project(), assets: [{ id: "a1", name: "Song", file: "audio/song.wav" }] };
  const doc = docReducer(initialDoc(opened), { type: "load", project: opened, path: "/shows/a.fxtc", active: TL });
  const after = docReducer(doc, { type: "derivedBpm", id: "a1", bpm: 126.5 });
  assert.equal(after.project.assets[0].bpm, 126.5);
  assert.equal(after.past.length, 0);
  assert.equal(isDirty(after), false);
  // A tempo the file already has is not replaced, and an unknown asset changes nothing.
  assert.equal(docReducer(after, { type: "derivedBpm", id: "a1", bpm: 90 }), after);
  assert.equal(docReducer(after, { type: "derivedBpm", id: "zzz", bpm: 90 }), after);
});

test("a controller gesture is closed in the store too, which merges what a reducer answers", () => {
  const start = initialDoc(project());
  const moved = docReducer(start, { type: "controlled", project: project(), active: TL, commit: false });
  assert.ok(moved.gesture);
  const done = docReducer(moved, { type: "controlled", project: project(), active: TL, commit: true });
  // What a merging store would hold: a dropped key would keep the old gesture.
  assert.equal({ ...moved, ...done }.gesture, undefined);
  assert.equal(done.past.length, 1);
});
