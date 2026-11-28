import assert from "node:assert/strict";
import { test } from "node:test";
import { cycleRipple, dismissError, isSelected, say, sayError, select, selectMany, useUi } from "./ui.ts";

test("errors remain pending until dismissed, independently of status updates", () => {
  useUi.setState({ errors: [] });
  sayError("First failure");
  say("status.ready");
  sayError("First failure");
  sayError("Second failure");
  assert.deepEqual(useUi.getState().errors, ["First failure", "Second failure"]);
  assert.equal(useUi.getState().message.tone, "info");

  dismissError();
  assert.deepEqual(useUi.getState().errors, ["Second failure"]);
  dismissError();
  dismissError();
  assert.deepEqual(useUi.getState().errors, []);

  sayError("First failure");
  assert.deepEqual(useUi.getState().errors, ["First failure"]);
  dismissError();
});

test("a selection of several items is picked together and a single click collapses it", () => {
  selectMany([{ kind: "note", id: "a" }, { kind: "clip", id: "b" }, { kind: "event", id: "c" }]);
  const ui = useUi.getState();
  assert.equal(ui.selection?.id, "c");
  assert.deepEqual([...ui.picked].sort(), ["a", "b", "c"]);
  assert.ok(isSelected(ui, "a") && isSelected(ui, "c") && !isSelected(ui, "d"));

  selectMany([{ kind: "note", id: "a" }]);
  assert.equal(useUi.getState().picked.size, 0);
  assert.equal(useUi.getState().selection?.id, "a");
  select(null);
  assert.equal(useUi.getState().selection, null);
});

test("ripple steps through off, per track and all tracks", () => {
  useUi.setState({ ripple: "off" });
  cycleRipple();
  assert.equal(useUi.getState().ripple, "track");
  cycleRipple();
  assert.equal(useUi.getState().ripple, "all");
  cycleRipple();
  assert.equal(useUi.getState().ripple, "off");
});
