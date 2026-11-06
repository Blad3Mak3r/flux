// Run with `npm test`. The values match the engine's mixer tests.
import assert from "node:assert/strict";
import { test } from "node:test";
import { envelopeDbAt, envelopeGainAt, envelopeLine, previewMove } from "./envelope.ts";

const p = (at, gainDb, curve) => (curve ? { at, gainDb, curve } : { at, gainDb });

test("outside its points the envelope holds the nearest one", () => {
  const points = [p(1000, -6), p(2000, 0)];
  assert.ok(Math.abs(envelopeGainAt(points, -50) - 10 ** (-6 / 20)) < 1e-12);
  assert.ok(Math.abs(envelopeGainAt(points, 1000) - 10 ** (-6 / 20)) < 1e-12);
  assert.equal(envelopeGainAt(points, 2000), 1);
  assert.equal(envelopeGainAt(points, 99_999), 1);
  assert.equal(envelopeGainAt([], 500), 1);
});

test("a segment runs in dB, so half way from 0 to -6 dB is -3 dB", () => {
  const points = [p(0, 0), p(1000, -6)];
  assert.ok(Math.abs(envelopeDbAt(points, 500) + 3) < 1e-9);
});

test("a segment that touches silence runs in amplitude", () => {
  assert.ok(Math.abs(envelopeGainAt([p(0, -60), p(1000, 0)], 500) - 0.5) < 1e-12);
  assert.ok(Math.abs(envelopeGainAt([p(0, 0), p(1000, -60)], 250) - 0.75) < 1e-12);
  assert.equal(envelopeDbAt([p(0, -60), p(1000, -60)], 500), -60);
});

test("the curves bend a segment the way the mixer does", () => {
  const at = (curve) => envelopeGainAt([p(0, -60, curve), p(1000, 0)], 250);
  assert.ok(at("slow") < at("linear"));
  assert.ok(at("smooth") < at("linear"));
  assert.ok(at("fast") > at("linear"));
  assert.equal(at("hold"), 0);
});

test("the line has a vertex for every point, and more where a segment bends", () => {
  assert.equal(envelopeLine([p(0, 0), p(1000, -6)]).length, 2);
  assert.equal(envelopeLine([p(0, 0, "smooth"), p(1000, -6)], 4).length, 5);
  const hold = envelopeLine([p(0, 0, "hold"), p(1000, -6)]);
  assert.deepEqual(hold, [{ at: 0, db: 0 }, { at: 1000, db: 0 }, { at: 1000, db: -6 }]);
});

test("a dragged point is shown between its neighbours and inside the range of gains", () => {
  const points = [p(0, 0), p(1000, -6), p(2000, 0)];
  assert.deepEqual(previewMove(points, 1, 1500, -12).map((x) => [x.at, x.gainDb]), [[0, 0], [1500, -12], [2000, 0]]);
  assert.equal(previewMove(points, 1, 5000, -6)[1].at, 1999);
  assert.equal(previewMove(points, 1, -5000, -6)[1].at, 1);
  assert.equal(previewMove(points, 0, -100, 0)[0].at, 0);
  assert.equal(previewMove(points, 1, 1000, -400)[1].gainDb, -60);
  assert.equal(previewMove(points, 1, 1000, 400)[1].gainDb, 6);
  assert.deepEqual(previewMove(points, 9, 0, 0), points);
  assert.equal(points[1].at, 1000);
});
