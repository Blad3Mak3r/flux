import assert from "node:assert/strict";
import { test } from "node:test";
import { dismissError, say, sayError, useUi } from "./ui.ts";

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
