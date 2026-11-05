// Run with `npm test`. The ruler and the graticule share these positions.
import assert from "node:assert/strict";
import { test } from "node:test";
import { barTickPositions, beatFrames, frameX, pickTicks, tickPositions } from "./scale.ts";

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

const GRID = { bpm: 120, beatsPerBar: 4, fps: 25 };

test("a beat lasts what the tempo says, in real frames", () => {
  assert.equal(beatFrames(GRID), 12.5);
  assert.equal(beatFrames({ bpm: 60, beatsPerBar: 4, fps: 30000 / 1001 }), 30000 / 1001);
});

test("bar ticks sit on the exact beat positions, not on whole frames", () => {
  // 120 BPM at 25 fps is 12.5 frames a beat: zoomed in, every beat is labelled bar.beat.
  const { major } = barTickPositions(40, GRID, 4_000);
  assert.deepEqual(major.slice(0, 6), [
    { frame: 0, label: "1" },
    { frame: 12.5, label: "1.2" },
    { frame: 25, label: "1.3" },
    { frame: 37.5, label: "1.4" },
    { frame: 50, label: "2" },
    { frame: 62.5, label: "2.2" },
  ]);
});

test("zoomed out, only bars are labelled and they double before labels can crowd", () => {
  for (const zoom of [0.04, 0.1, 0.37, 1.6, 7.3]) {
    const { major } = barTickPositions(zoom, GRID, 20_000);
    const px = (major[1].frame - major[0].frame) * zoom;
    assert.ok(px >= 96 - 1e-9, `${zoom}: labels ${px}px apart`);
    assert.ok(major.every((tick) => !tick.label.includes(".") || zoom > 1));
    // The labelled bars are evenly spaced multiples of one interval, from bar 1.
    const gap = major[1].frame;
    assert.ok(major.every((tick, i) => Math.abs(tick.frame - i * gap) < 1e-6));
    assert.equal(major[0].label, "1");
  }
});

test("minor ticks are the beats between bar lines, and never repeat a labelled tick", () => {
  // At zoom 1.6 a beat is 20px: bars (4 beats, 80px) are not far enough apart, so labels are every 2 bars.
  const { major, minor } = barTickPositions(1.6, GRID, 3_000);
  assert.ok(major[1].frame === 100 && major[1].label === "3");
  assert.ok(minor.length > 0);
  assert.ok(minor.every((f) => !major.some((tick) => tick.frame === f)));
  assert.ok(minor.every((f) => Math.abs(f / 12.5 - Math.round(f / 12.5)) < 1e-9));
});

test("a bar of three beats counts in threes", () => {
  const { major } = barTickPositions(40, { bpm: 120, beatsPerBar: 3, fps: 25 }, 2_000);
  assert.deepEqual(major.slice(0, 4).map((tick) => tick.label), ["1", "1.2", "1.3", "2"]);
});

test("the grid covers the whole width, to the last bar", () => {
  const width = 5_000;
  const { major, minor } = barTickPositions(0.5, GRID, width);
  const last = Math.max(...major.map((t) => t.frame), ...minor);
  assert.ok(last * 0.5 >= width - beatFrames(GRID) * 0.5 * 8 - 1);
});
