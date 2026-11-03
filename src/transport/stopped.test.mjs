// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { stoppedAt } from "./stopped.ts";

const playing = {
  state: "playing",
  frame: 90_123,
  positionSamples: 4_800_000,
  deviceLost: false,
  clockUnpaced: false,
};

test("stoppedAt shows the timeline start, stopped, and keeps the device flags", () => {
  const stopped = stoppedAt({ ...playing, deviceLost: true }, 90_000);
  assert.equal(stopped.state, "stopped");
  assert.equal(stopped.frame, 90_000);
  assert.equal(stopped.positionSamples, 0);
  assert.equal(stopped.deviceLost, true);
});

test("stoppedAt does not change the position it was given", () => {
  stoppedAt(playing, 0);
  assert.equal(playing.state, "playing");
  assert.equal(playing.frame, 90_123);
});
