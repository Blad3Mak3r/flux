// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { dropIndex, dropLineY } from "../timeline/reorder.ts";
import { activeTimeline, docReducer, initialDoc, isDirty } from "./doc.ts";
import {
  addAudioClip,
  addNote,
  emptyProject,
  findItem,
  moveItem,
  moveTrack,
  removeItem,
  removeTrack,
  setAssetBpm,
  setNoteLength,
  setTempo,
  setTrackColor,
  setTrackGain,
  toggleMute,
  updateClip,
  updateNote,
} from "./edit.ts";

/** The id of the timeline every test project has. */
const TL = "l1";

/** The project with these tracks in its timeline. */
const withTracks = (base, tracks) => ({ ...base, timelines: [{ ...base.timelines[0], id: TL, tracks }] });

function project() {
  return withTracks(emptyProject("Show", "fps25"), [
    { id: "t1", name: "Backing", muted: false, kind: { type: "audio", clips: [] } },
    { id: "t2", name: "Notes", muted: false, kind: { type: "notes", notes: [] } },
  ]);
}

test("setTrackGain stores the fader on an audio track without touching the input", () => {
  const before = project();
  const after = setTrackGain(before, TL, "t1", -6);
  assert.equal(after.timelines[0].tracks[0].gainDb, -6);
  assert.equal(before.timelines[0].tracks[0].gainDb, undefined);
  assert.equal(after.timelines[0].tracks[1], before.timelines[0].tracks[1]);
});

test("setTrackGain stores 0 dB as no value", () => {
  const down = setTrackGain(project(), TL, "t1", -6);
  const reset = setTrackGain(down, TL, "t1", 0);
  assert.equal("gainDb" in reset.timelines[0].tracks[0], false);
});

test("setTrackGain returns the same project when nothing changes or the track cannot have a fader", () => {
  const before = project();
  assert.equal(setTrackGain(before, TL, "t1", 0), before);
  assert.equal(setTrackGain(before, TL, "t2", -6), before);
  assert.equal(setTrackGain(before, TL, "missing", -6), before);
});

test("undo and redo step a fader change", () => {
  let doc = initialDoc(project());
  const first = doc.project;
  doc = docReducer(doc, { type: "edit", project: setTrackGain(doc.project, TL, "t1", -12) });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -12);
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, first);
  doc = docReducer(doc, { type: "redo" });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -12);
});

test("setTrackColor stores a colour, returns to the default with null, and keeps the input intact", () => {
  const before = project();
  const coloured = setTrackColor(before, TL, "t1", "#66d5ba");
  assert.equal(coloured.timelines[0].tracks[0].color, "#66d5ba");
  assert.equal(before.timelines[0].tracks[0].color, undefined);
  assert.equal(coloured.timelines[0].tracks[1], before.timelines[0].tracks[1]);

  const reset = setTrackColor(coloured, TL, "t1", null);
  assert.ok(!("color" in reset.timelines[0].tracks[0]));
});

test("setTrackColor leaves the project alone when nothing changes", () => {
  const before = project();
  assert.equal(setTrackColor(before, TL, "t1", null), before);
  assert.equal(setTrackColor(before, TL, "missing", "#66d5ba"), before);
});

test("a colour change is one undo step", () => {
  const before = project();
  let doc = docReducer(initialDoc(before), { type: "edit", project: setTrackColor(before, TL, "t2", "#c3b0fd") });
  assert.equal(doc.project.timelines[0].tracks[1].color, "#c3b0fd");
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, before);
  doc = docReducer(doc, { type: "redo" });
  assert.equal(doc.project.timelines[0].tracks[1].color, "#c3b0fd");
});

function withItems() {
  const base = project();
  return withTracks(base, [
    { ...base.timelines[0].tracks[0], kind: { type: "audio", clips: [{ id: "c1", asset: "a", start: 0, gainDb: 0 }] } },
    { id: "t3", name: "Other", muted: false, kind: { type: "audio", clips: [{ id: "c2", asset: "b", start: 5, gainDb: 0 }] } },
    { ...base.timelines[0].tracks[1], kind: { type: "notes", notes: [{ id: "k1", at: 3, text: "Go" }] } },
  ]);
}

test("moving a clip keeps every other track as it was", () => {
  const before = withItems();
  const after = moveItem(before, TL, "c1", 40);
  assert.equal(after.timelines[0].tracks[0].kind.clips[0].start, 40);
  assert.equal(after.timelines[0].tracks[1], before.timelines[0].tracks[1]);
  assert.equal(after.timelines[0].tracks[2], before.timelines[0].tracks[2]);
});

test("moving a note keeps the audio tracks as they were", () => {
  const before = withItems();
  const after = moveItem(before, TL, "k1", 9);
  assert.equal(after.timelines[0].tracks[2].kind.notes[0].at, 9);
  assert.equal(after.timelines[0].tracks[0], before.timelines[0].tracks[0]);
  assert.equal(after.timelines[0].tracks[1], before.timelines[0].tracks[1]);
});

test("removing a note rebuilds only the track that held it and keeps the track when it is empty", () => {
  const before = withItems();
  const after = removeItem(before, TL, "k1");
  assert.deepEqual(after.timelines[0].tracks[2].kind.notes, []);
  assert.equal(after.timelines[0].tracks.length, 3);
  assert.equal(after.timelines[0].tracks[0], before.timelines[0].tracks[0]);
  assert.equal(after.timelines[0].tracks[1], before.timelines[0].tracks[1]);
});

test("removing the last clip of an audio track removes the track and its audio file", () => {
  const before = { ...withItems(), assets: [{ id: "a", name: "A", file: "audio/a" }, { id: "b", name: "B", file: "audio/b" }] };
  const after = removeItem(before, TL, "c2");
  assert.deepEqual(after.timelines[0].tracks.map((track) => track.id), ["t1", "t2"]);
  assert.deepEqual(after.assets.map((asset) => asset.id), ["a"]);
  assert.equal(after.timelines[0].tracks[0], before.timelines[0].tracks[0]);
});

test("removing a clip keeps its track while it has other clips, and its audio while another clip uses it", () => {
  const base = withItems();
  const before = {
    ...withTracks(base, [
      { ...base.timelines[0].tracks[0], kind: { type: "audio", clips: [
        { id: "c1", asset: "a", start: 0, gainDb: 0 },
        { id: "c3", asset: "a", start: 9, gainDb: 0 },
      ] } },
      base.timelines[0].tracks[1],
      base.timelines[0].tracks[2],
    ]),
    assets: [{ id: "a", name: "A", file: "audio/a" }, { id: "b", name: "B", file: "audio/b" }],
  };
  const after = removeItem(before, TL, "c1");
  assert.equal(after.timelines[0].tracks.length, 3);
  assert.deepEqual(after.timelines[0].tracks[0].kind.clips.map((clip) => clip.id), ["c3"]);
  assert.equal(after.assets.length, 2);
});

test("undo brings back a track removed with its last clip", () => {
  const before = { ...withItems(), assets: [{ id: "a", name: "A", file: "audio/a" }, { id: "b", name: "B", file: "audio/b" }] };
  let doc = docReducer(initialDoc(before), { type: "edit", project: removeItem(before, TL, "c2") });
  doc = docReducer(doc, { type: "undo" });
  assert.equal(doc.project, before);
});

test("saving keeps the project and takes the metadata the backend wrote", () => {
  const meta = { ...project().meta, modifiedAt: "2030-01-01T00:00:00Z", savedWith: "9.9.9" };
  const edited = setTrackGain(project(), TL, "t1", -6);
  let doc = docReducer(initialDoc(project()), { type: "edit", project: edited });
  assert.equal(isDirty(doc), true);
  doc = docReducer(doc, { type: "saved", path: "/show.fxtc", meta });
  assert.equal(doc.project.timelines[0].tracks[0].gainDb, -6);
  assert.deepEqual(doc.project.meta, meta);
  assert.equal(isDirty(doc), false);
});

test("a new project has an id of its own and a creation date", () => {
  const first = emptyProject("A", "fps25").meta;
  assert.notEqual(first.id, emptyProject("B", "fps25").meta.id);
  assert.match(first.createdAt, /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
  assert.equal(first.modifiedAt, first.createdAt);
});

test("an edit that finds nothing to change returns the same project", () => {
  const before = withItems();
  assert.equal(removeItem(before, TL, "missing"), before);
  assert.equal(updateClip(before, TL, "missing", { start: 1 }), before);
  assert.equal(toggleMute(before, TL, "missing"), before);
});

test("restore picks a document up with no history, and unsaved work is told from what it is compared to", () => {
  const saved = project();
  const edited = setTrackGain(saved, TL, "t1", -6);
  let doc = docReducer(initialDoc(project()), { type: "restore", project: edited, path: "/show.fxtc", saved, active: TL });
  assert.equal(doc.project, edited);
  assert.equal(doc.path, "/show.fxtc");
  assert.deepEqual([doc.past, doc.future, doc.audio], [[], [], {}]);
  assert.equal(isDirty(doc), true);

  doc = docReducer(initialDoc(project()), { type: "restore", project: saved, path: null, saved, active: TL });
  assert.equal(isDirty(doc), false);
});

test("addNote creates a notes track when there is none and puts an empty note at the position", () => {
  const before = withTracks(project(), [project().timelines[0].tracks[0]]);
  const { project: after, id } = addNote(before, TL, 120, "Notes");
  assert.equal(after.timelines[0].tracks.length, 2);
  assert.equal(after.timelines[0].tracks[1].kind.type, "notes");
  assert.deepEqual(after.timelines[0].tracks[1].kind.notes, [{ id, at: 120, text: "" }]);
  assert.equal(before.timelines[0].tracks.length, 1);
});

test("addNote uses the first notes track and leaves the other tracks as they were", () => {
  const before = project();
  const { project: after, id } = addNote(before, TL, 10, "Notes");
  assert.equal(after.timelines[0].tracks.length, 2);
  assert.equal(after.timelines[0].tracks[1].kind.notes[0].id, id);
  assert.equal(after.timelines[0].tracks[0], before.timelines[0].tracks[0]);
});

test("addNote prefers the track it is given when that is a notes track", () => {
  const base = project();
  const two = withTracks(base, [
    ...base.timelines[0].tracks,
    { id: "t9", name: "More", muted: false, kind: { type: "notes", notes: [] } },
  ]);
  const { project: after, id } = addNote(two, TL, 5, "Notes", "t9");
  assert.equal(after.timelines[0].tracks[2].kind.notes[0].id, id);
  assert.equal(after.timelines[0].tracks[1], two.timelines[0].tracks[1]);
});

test("addNote ignores a track id that is not a notes track", () => {
  const { project: after, id } = addNote(project(), TL, 5, "Notes", "t1");
  assert.equal(after.timelines[0].tracks[1].kind.notes[0].id, id);
});

test("addNote never puts a note before the timeline origin", () => {
  const { project: after } = addNote(project(), TL, -30, "Notes");
  assert.equal(after.timelines[0].tracks[1].kind.notes[0].at, 0);
});

test("updateNote changes the text and keeps the other notes", () => {
  const before = withItems();
  const after = updateNote(before, TL, "k1", { text: "Lights up\nslowly" });
  assert.equal(after.timelines[0].tracks[2].kind.notes[0].text, "Lights up\nslowly");
  assert.equal(after.timelines[0].tracks[0], before.timelines[0].tracks[0]);
});

test("setNoteLength stores a length, and no time removes it", () => {
  const withLength = setNoteLength(withItems(), TL, "k1", 50);
  assert.equal(withLength.timelines[0].tracks[2].kind.notes[0].length, 50);
  const without = setNoteLength(withLength, TL, "k1", 0);
  assert.equal("length" in without.timelines[0].tracks[2].kind.notes[0], false);
});

test("setNoteLength on something that is not a note changes nothing", () => {
  const before = withItems();
  assert.equal(setNoteLength(before, TL, "c1", 50), before);
});

test("findItem finds a note and says so", () => {
  const found = findItem(withItems(), "k1");
  assert.equal(found.kind, "note");
  assert.equal(found.note.text, "Go");
});

test("removing a track takes its audio file with it and keeps the file another track still uses", () => {
  const base = withItems();
  const before = {
    ...base,
    assets: [{ id: "a", name: "A", file: "audio/a" }, { id: "b", name: "B", file: "audio/b" }],
  };
  const after = removeTrack(before, TL, "t3");
  assert.deepEqual(after.timelines[0].tracks.map((track) => track.id), ["t1", "t2"]);
  assert.deepEqual(after.assets.map((asset) => asset.id), ["a"]);
  assert.equal(removeTrack(before, TL, "nope"), before);
});

const order = (p) => p.timelines[0].tracks.map((track) => track.id);
const three = () =>
  withTracks(project(), [
    ...project().timelines[0].tracks,
    { id: "t3", name: "Cues", muted: false, kind: { type: "notes", notes: [] } },
  ]);

test("moveTrack puts a track at the new index without touching the input", () => {
  const before = three();
  assert.deepEqual(order(moveTrack(before, TL, "t1", 2)), ["t2", "t3", "t1"]);
  assert.deepEqual(order(moveTrack(before, TL, "t3", 0)), ["t3", "t1", "t2"]);
  assert.deepEqual(order(before), ["t1", "t2", "t3"]);
});

test("moveTrack clamps the index and ignores a move that changes nothing", () => {
  const before = three();
  assert.deepEqual(order(moveTrack(before, TL, "t1", 99)), ["t2", "t3", "t1"]);
  assert.equal(moveTrack(before, TL, "t2", 1), before);
  assert.equal(moveTrack(before, TL, "nope", 0), before);
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

/** The project with a second timeline, `l2`, that holds one audio clip of asset `a`. */
function twoTimelines() {
  const base = withItems();
  const second = {
    id: "l2",
    name: "Second",
    timecode: { frameRate: "fps30", start: 0 },
    tracks: [{ id: "t8", name: "Song", muted: false, kind: { type: "audio", clips: [{ id: "c8", asset: "a", start: 0, gainDb: 0 }] } }],
  };
  return { ...base, assets: [{ id: "a", name: "A", file: "audio/a" }, { id: "b", name: "B", file: "audio/b" }], timelines: [...base.timelines, second] };
}

test("an edit of one timeline leaves the other as it was", () => {
  const before = twoTimelines();
  const after = moveItem(before, "l1", "c1", 40);
  assert.equal(after.timelines[0].tracks[0].kind.clips[0].start, 40);
  assert.equal(after.timelines[1], before.timelines[1]);
});

test("an edit given a timeline that does not exist changes nothing", () => {
  const before = twoTimelines();
  assert.equal(moveItem(before, "nope", "c1", 40), before);
  assert.equal(toggleMute(before, "nope", "t1"), before);
});

test("findItem finds an item in any timeline", () => {
  assert.equal(findItem(twoTimelines(), "c8").kind, "clip");
});

test("removing a clip keeps the audio file that a clip of another timeline still plays", () => {
  const before = twoTimelines();
  const after = removeItem(before, "l1", "c1");
  assert.deepEqual(after.assets.map((asset) => asset.id), ["a", "b"]);
});

test("removing the last clip that plays an audio file drops the file, whatever timeline it was in", () => {
  const after = removeItem(removeItem(twoTimelines(), "l1", "c1"), "l1", "c2");
  assert.deepEqual(after.assets.map((asset) => asset.id), ["a"]);
});

test("removing a clip prunes the empty audio tracks of its own timeline only", () => {
  const before = twoTimelines();
  const after = removeItem(before, "l2", "c8");
  assert.deepEqual(after.timelines[1].tracks, []);
  assert.equal(after.timelines[0], before.timelines[0]);
});

test("addAudioClip adds the track to the given timeline and the audio file to the project", () => {
  const before = twoTimelines();
  const asset = { id: "z", name: "Z", file: "audio/z" };
  const { project: after, id } = addAudioClip(before, "l2", asset, 12);
  assert.equal(after.timelines[1].tracks[1].kind.clips[0].id, id);
  assert.equal(after.timelines[0], before.timelines[0]);
  assert.equal(after.assets.at(-1), asset);
});

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

test("a new project starts at 120 BPM in 4, with the click off", () => {
  assert.deepEqual(emptyProject("Show", "fps25").timelines[0].tempo, { bpm: 120, beatsPerBar: 4, click: false });
});

test("setTempo changes the tempo of one timeline and leaves the input alone", () => {
  const before = project();
  const after = setTempo(before, TL, { bpm: 128.456, beatsPerBar: 3, click: true });
  assert.deepEqual(after.timelines[0].tempo, { bpm: 128.46, beatsPerBar: 3, click: true });
  assert.equal(before.timelines[0].tempo.bpm, 120);
});

test("setTempo keeps the tempo in the range the backend accepts", () => {
  assert.equal(setTempo(project(), TL, { bpm: 5 }).timelines[0].tempo.bpm, 20);
  assert.equal(setTempo(project(), TL, { bpm: 9000 }).timelines[0].tempo.bpm, 400);
  assert.equal(setTempo(project(), TL, { bpm: Number.NaN }).timelines[0].tempo.bpm, 120);
  assert.equal(setTempo(project(), TL, { beatsPerBar: 0 }).timelines[0].tempo.beatsPerBar, 1);
  assert.equal(setTempo(project(), TL, { beatsPerBar: 40 }).timelines[0].tempo.beatsPerBar, 16);
  assert.equal(setTempo(project(), TL, { beatsPerBar: 5.6 }).timelines[0].tempo.beatsPerBar, 6);
});

test("setTempo with nothing new returns the same project, so no history entry is made", () => {
  const before = project();
  assert.equal(setTempo(before, TL, { bpm: 120 }), before);
  assert.equal(setTempo(before, "missing", { bpm: 90 }), before);
});

test("setAssetBpm stores the tempo of an audio file and forgets it on null", () => {
  const base = { ...project(), assets: [{ id: "a1", name: "Song", file: "audio/song.wav" }] };
  const set = setAssetBpm(base, "a1", 127.456);
  assert.equal(set.assets[0].bpm, 127.46);
  assert.equal(base.assets[0].bpm, undefined);
  const cleared = setAssetBpm(set, "a1", null);
  assert.ok(!("bpm" in cleared.assets[0]));
  assert.equal(setAssetBpm(base, "a1", null), base);
  assert.equal(setAssetBpm(set, "missing", 100), set);
  assert.equal(setAssetBpm(set, "a1", 127.46), set);
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
