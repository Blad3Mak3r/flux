import assert from "node:assert/strict";
import { test } from "node:test";
import { isAvailable, portChoices, portKey, portLabel, splitPortKey, stepSection } from "./outputs.ts";

test("ports that share a name are told apart by their place among them", () => {
  const names = ["Hub", "Other", "Hub"];
  const choices = portChoices(names);
  assert.deepEqual(choices, [{ portName: "Hub" }, { portName: "Other" }, { portName: "Hub", portIndex: 1 }]);
  assert.deepEqual(choices.map((choice) => portLabel(choice, choices)), ["Hub (1)", "Other", "Hub (2)"]);
  assert.deepEqual(splitPortKey(portKey(choices[2])), ["Hub", 1]);

});

test("stepSection wraps round and skips the section that is not available", () => {
  assert.equal(isAvailable("video"), false);
  assert.equal(stepSection("project", 1), "audio");
  assert.equal(stepSection("general", 1), "timeline");
  assert.equal(stepSection("timeline", -1), "general");
  assert.equal(stepSection("project", -1), "timeline");
});
