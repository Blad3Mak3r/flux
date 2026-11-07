// Run with `npm test`. These values are the ones the engine's mixer is tested against.
import assert from "node:assert/strict";
import { test } from "node:test";
import { FADE_CURVES, fadePoints, fadeShape } from "./fade.ts";

test("every curve runs from silence to full level and never falls on the way up", () => {
  for (const curve of FADE_CURVES) {
    assert.equal(fadeShape(curve, 0), 0, curve);
    assert.ok(Math.abs(fadeShape(curve, 1) - 1) < 1e-12, curve);
    let last = 0;
    for (let step = 1; step <= 100; step++) {
      const gain = fadeShape(curve, step / 100);
      assert.ok(gain >= last, `${curve} at ${step}`);
      last = gain;
    }
  }
});

test("the curves hit the same midpoints as the mixer", () => {
  assert.equal(fadeShape("linear", 0.5), 0.5);
  assert.ok(Math.abs(fadeShape("equalPower", 0.5) - 0.7071067811865476) < 1e-12);
  assert.ok(Math.abs(fadeShape("sCurve", 0.5) - 0.5) < 1e-12);
  assert.equal(fadeShape("exponential", 0.5), 0.25);
});

test("a fade in is drawn rising and a fade out falling, with silence at the bottom", () => {
  const rising = fadePoints("linear", "in", 100, 40, 4).split(" ");
  assert.equal(rising[0], "0,40");
  assert.equal(rising.at(-1), "100,0");
  const falling = fadePoints("linear", "out", 100, 40, 4).split(" ");
  assert.equal(falling[0], "0,0");
  assert.equal(falling.at(-1), "100,40");
});

test("a curved fade is drawn off the straight line", () => {
  const linear = fadePoints("linear", "in", 100, 40, 4).split(" ")[2];
  const exponential = fadePoints("exponential", "in", 100, 40, 4).split(" ")[2];
  assert.equal(linear, "50,20");
  assert.equal(exponential, "50,30");
});
