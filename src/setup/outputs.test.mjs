import assert from "node:assert/strict";
import { test } from "node:test";
import { SETUP_SECTIONS, groupOf, isAvailable, isListed, portChoices, portKey, portLabel, splitPortKey } from "./outputs.ts";

test("ports that share a name are told apart by their place among them", () => {
  const names = ["Hub", "Other", "Hub"];
  const choices = portChoices(names);
  assert.deepEqual(choices, [{ portName: "Hub" }, { portName: "Other" }, { portName: "Hub", portIndex: 1 }]);
  assert.deepEqual(choices.map((choice) => portLabel(choice, choices)), ["Hub (1)", "Other", "Hub (2)"]);
  assert.deepEqual(splitPortKey(portKey(choices[2])), ["Hub", 1]);

});

test("every page is listed once, in its group, and only the planned ones are unavailable", () => {
  assert.equal(new Set(SETUP_SECTIONS).size, SETUP_SECTIONS.length);
  assert.deepEqual(SETUP_SECTIONS.filter((section) => !isAvailable(section)), ["http"]);
  assert.equal(groupOf("ltc"), "outputs");
  assert.equal(groupOf("audioOutputs"), "audio");
  assert.equal(groupOf("audio"), "audio");
  assert.equal(groupOf("project"), "show");
  assert.equal(groupOf("controlOsc"), "control");
  assert.equal(groupOf("http"), "control");
  assert.equal(groupOf("general"), "app");
});

test("timecode input is only listed for a VJ show, which has no timecode outputs", () => {
  assert.equal(isListed("timecodeIn", true), true);
  assert.equal(isListed("timecodeIn", false), false);
  assert.equal(isListed("analysis", true), true);
  assert.equal(isListed("analysis", false), false);
  assert.equal(isListed("ltc", true), false);
  assert.equal(isListed("ltc", false), true);
});
