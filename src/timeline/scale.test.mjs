// Run with `npm test`. The ruler and the graticule share these positions.
import assert from "node:assert/strict";
import { test } from "node:test";
import { frameX, pickTicks, tickPositions } from "./scale.ts";

const RATES = ["fps24", "fps25", "fps2997Drop", "fps30"];
const ZOOMS = [0.04, 0.1, 0.37, 1.6, 2.5, 7.3, 40];
// A few hours, at the start, in the middle and at the far end.
const HOURS = 3;

test("ticks are whole multiples of their interval, from frame 0, with no overlap", () => {
  for (const rate of RATES) {
    for (const zoom of ZOOMS) {
      const { major, minor } = tickPositions(zoom, rate, 5_000);
      const ticks = pickTicks(zoom, rate);
      assert.equal(major[0], 0);
      assert.ok(major.every((f, i) => f === i * ticks.major));
      assert.ok(minor.every((f) => f % ticks.minor === 0 && f % ticks.major !== 0));
    }
  }
});

test("positions stay exact to the last pixel at any distance, not drifting from a tiled approximation", () => {
  for (const rate of RATES) {
    const fps = rate === "fps24" ? 24 : rate === "fps25" ? 25 : 30;
    for (const zoom of ZOOMS) {
      const width = HOURS * 3600 * fps * zoom;
      const { major } = tickPositions(zoom, rate, width);
      const step = pickTicks(zoom, rate).major;
      for (const index of [0, Math.floor(major.length / 2), major.length - 1]) {
        assert.equal(frameX(major[index], zoom), Math.round(index * step * zoom));
        // Never more than half a pixel from the exact position.
        assert.ok(Math.abs(frameX(major[index], zoom) - index * step * zoom) <= 0.5);
      }
    }
  }
});

test("the ruler's labelled ticks are the graticule's major lines", () => {
  const { major, minor } = tickPositions(1.6, "fps2997Drop", 3_000);
  assert.deepEqual(major.slice(0, 3), [0, 60, 120]);
  assert.ok(minor.length > 0 && minor.every((f) => !major.includes(f)));
});
