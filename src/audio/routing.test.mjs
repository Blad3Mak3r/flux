import assert from "node:assert/strict";
import { test } from "node:test";
import { docReducer, initialDoc } from "../state/doc.ts";
import { emptyProject } from "../state/edit.ts";

test("one backend routing edit restores destinations and every track assignment through undo and redo", () => {
  const project = emptyProject("Show", "fps25");
  const id = "5fda6c22-87d3-40e9-9710-589f8c1ef8f0";
  project.output.audio = [{ id, name: "Monitors" }];
  project.timelines[0].tracks = [{ id: "audio", name: "Music", muted: false, audioOutput: { type: "output", id }, kind: { type: "audio", clips: [] } }];
  const second = structuredClone(project.timelines[0]);
  second.id = "second";
  second.tracks[0].id = "other";
  project.timelines.push(second);
  const result = structuredClone(project);
  result.output.audio = [];
  for (const timeline of result.timelines) timeline.tracks[0].audioOutput = { type: "silent" };
  const edited = docReducer(initialDoc(project), { type: "apply", project: result, active: project.timelines[0].id });
  assert.equal(edited.past.length, 1);
  const undone = docReducer(edited, { type: "undo" });
  assert.deepEqual(undone.project, project);
  assert.deepEqual(docReducer(undone, { type: "redo" }).project, result);
});
