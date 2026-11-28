// Run with `npm test`. Expected values are the ones in src-tauri/crates/flux-core/src/timecode.rs.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatFrames,
  framesPerDay,
  framesPerHour,
  framesToTimecode,
} from "./timecode.ts";

const RATES = ["fps24", "fps25", "fps2997Drop", "fps30"];
const tc = (hours, minutes, seconds, frames) => ({ hours, minutes, seconds, frames });

test("a drop-frame hour has 107 892 frames", () => {
  assert.equal(framesPerHour("fps2997Drop"), 107_892);
});

test("drop-frame skips frames 0 and 1 each minute, except every tenth", () => {
  assert.deepEqual(framesToTimecode(1800, "fps2997Drop"), tc(0, 1, 0, 2));
  assert.deepEqual(framesToTimecode(17_982, "fps2997Drop"), tc(0, 10, 0, 0));
});

test("counts wrap after 24 hours", () => {
  for (const rate of RATES) {
    assert.deepEqual(framesToTimecode(framesPerDay(rate), rate), tc(0, 0, 0, 0));
  }
});

test("drop-frame uses a semicolon before the frames", () => {
  assert.equal(formatFrames(1800, "fps2997Drop"), "00:01:00;02");
  assert.equal(formatFrames(90_000, "fps25"), "01:00:00:00");
});
