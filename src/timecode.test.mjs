// Run with `npm test`. Expected values are the ones in src-tauri/crates/flux-core/src/timecode.rs.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatFrames,
  framesPerDay,
  framesPerHour,
  framesToTimecode,
  parseTimecode,
  timecodeToFrames,
} from "./timecode.ts";

const RATES = ["fps24", "fps25", "fps2997Drop", "fps30"];
const tc = (hours, minutes, seconds, frames) => ({ hours, minutes, seconds, frames });

test("a drop-frame hour has 107 892 frames", () => {
  assert.equal(framesPerHour("fps2997Drop"), 107_892);
  assert.equal(timecodeToFrames(tc(1, 0, 0, 0), "fps2997Drop"), 107_892);
});

test("drop-frame skips frames 0 and 1 each minute, except every tenth", () => {
  assert.equal(timecodeToFrames(tc(0, 0, 59, 29), "fps2997Drop"), 1799);
  assert.equal(timecodeToFrames(tc(0, 1, 0, 2), "fps2997Drop"), 1800);
  assert.deepEqual(framesToTimecode(1800, "fps2997Drop"), tc(0, 1, 0, 2));
  assert.equal(timecodeToFrames(tc(0, 10, 0, 0), "fps2997Drop"), 17_982);
  assert.deepEqual(framesToTimecode(17_982, "fps2997Drop"), tc(0, 10, 0, 0));
});

test("dropped frames and out-of-range fields are rejected", () => {
  assert.equal(timecodeToFrames(tc(0, 1, 0, 0), "fps2997Drop"), null);
  assert.equal(timecodeToFrames(tc(0, 1, 0, 1), "fps2997Drop"), null);
  assert.equal(timecodeToFrames(tc(24, 0, 0, 0), "fps25"), null);
  assert.equal(timecodeToFrames(tc(0, 60, 0, 0), "fps30"), null);
  assert.equal(timecodeToFrames(tc(0, 0, 0, 24), "fps24"), null);
  assert.equal(timecodeToFrames(tc(0, 0, 0, 29), "fps30"), 29);
});

test("frames round trip over a whole day for every rate", () => {
  for (const rate of RATES) {
    for (let frame = 0; frame < framesPerDay(rate); frame += 1) {
      assert.equal(timecodeToFrames(framesToTimecode(frame, rate), rate), frame, `${rate} ${frame}`);
    }
  }
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

test("parseTimecode reads separators and keypad digits", () => {
  assert.equal(parseTimecode("01:00:00:00", "fps25"), 90_000);
  assert.equal(parseTimecode("00:01:00;02", "fps2997Drop"), 1800);
  assert.equal(parseTimecode("1000000", "fps25"), 90_000);
  assert.equal(parseTimecode("1:10", "fps25"), 35);
  assert.equal(parseTimecode("", "fps25"), null);
  assert.equal(parseTimecode("ab", "fps25"), null);
  assert.equal(parseTimecode("00:00:00:25", "fps25"), null);
  assert.equal(parseTimecode("1:2:3:4:5", "fps25"), null);
});
