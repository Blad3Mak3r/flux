// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  BALLISTICS_START,
  FADER_MAX_DB,
  FADER_MIN_DB,
  HOLD_MS,
  METER_SEGMENTS,
  clampFaderDb,
  dbFromLinear,
  dbToFader,
  faderToDb,
  formatDb,
  isSettled,
  litSegments,
  segmentTopDb,
  stepBallistics,
  toneOf,
} from "./level.ts";

test("faderToDb maps the ends and unity of the fader", () => {
  assert.equal(faderToDb(0), FADER_MIN_DB);
  assert.equal(faderToDb(0.8), 0);
  assert.equal(faderToDb(1), FADER_MAX_DB);
  assert.equal(faderToDb(-1), FADER_MIN_DB);
  assert.equal(faderToDb(2), FADER_MAX_DB);
});

test("dbToFader inverts faderToDb along the whole travel", () => {
  for (const db of [-60, -45, -30, -12, -6, 0, 3, 6]) {
    assert.ok(Math.abs(faderToDb(dbToFader(db)) - db) <= 0.1, `${db} dB`);
  }
  assert.ok(dbToFader(-12) < dbToFader(0));
});

test("clampFaderDb stays in range and to a tenth", () => {
  assert.equal(clampFaderDb(-100), FADER_MIN_DB);
  assert.equal(clampFaderDb(40), FADER_MAX_DB);
  assert.equal(clampFaderDb(-3.14159), -3.1);
});

test("formatDb prints a sign, one decimal and -inf at the bottom", () => {
  assert.equal(formatDb(3), "+3.0");
  assert.equal(formatDb(-6), "-6.0");
  assert.equal(formatDb(0), "0.0");
  assert.equal(formatDb(FADER_MIN_DB), "-inf");
  assert.equal(formatDb(-Infinity), "-inf");
});

test("dbFromLinear converts a peak to dBFS", () => {
  assert.equal(dbFromLinear(1), 0);
  assert.ok(Math.abs(dbFromLinear(0.5) + 6.0206) < 1e-3);
  assert.equal(dbFromLinear(0), -Infinity);
});

test("toneOf is green up to -12, yellow up to -3 and red above", () => {
  assert.equal(toneOf(-40), "green");
  assert.equal(toneOf(-12), "green");
  assert.equal(toneOf(-11.9), "yellow");
  assert.equal(toneOf(-3), "yellow");
  assert.equal(toneOf(-2.9), "red");
  assert.equal(toneOf(0), "red");
});

test("litSegments lights a segment as soon as the level clears the floor", () => {
  assert.equal(litSegments(-Infinity), 0);
  assert.equal(litSegments(-60), 0);
  assert.equal(litSegments(-59.9), 1);
  assert.equal(litSegments(0), METER_SEGMENTS);
  assert.equal(litSegments(6), METER_SEGMENTS);
  assert.equal(segmentTopDb(METER_SEGMENTS - 1), 0);
});

test("a meter rises at once and falls slowly", () => {
  let state = stepBallistics(BALLISTICS_START, -6, 33);
  assert.equal(state.level, -6);
  state = stepBallistics(state, -Infinity, 100);
  assert.ok(state.level < -6 && state.level > -12, `level ${state.level}`);
  assert.equal(state.hold, -6);
});

test("the peak marker holds, then falls slower than the bar settles", () => {
  let state = stepBallistics(BALLISTICS_START, -6, 33);
  for (let t = 0; t < HOLD_MS - 100; t += 100) state = stepBallistics(state, -Infinity, 100);
  assert.equal(state.hold, -6);
  for (let t = 0; t < 600; t += 100) state = stepBallistics(state, -Infinity, 100);
  assert.ok(state.hold < -6);
});

test("a meter settles at silence after playback stops", () => {
  let state = stepBallistics(BALLISTICS_START, -3, 33);
  for (let t = 0; t < 10_000 && !isSettled(state); t += 33) state = stepBallistics(state, -Infinity, 33);
  assert.ok(isSettled(state));
});

test("clip latches at 0 dBFS until the state is replaced", () => {
  let state = stepBallistics(BALLISTICS_START, 0.5, 33);
  assert.equal(state.clip, true);
  state = stepBallistics(state, -Infinity, 5000);
  assert.equal(state.clip, true);
  assert.equal(stepBallistics(BALLISTICS_START, -0.1, 33).clip, false);
});

test("with reduced motion the bar follows the signal and the marker does not slide", () => {
  let state = stepBallistics(BALLISTICS_START, -6, 33, true);
  state = stepBallistics(state, -30, 33, true);
  assert.equal(state.level, -30);
  assert.equal(state.hold, -6);
  state = stepBallistics(state, -30, HOLD_MS + 1, true);
  assert.equal(state.hold, -30);
});
