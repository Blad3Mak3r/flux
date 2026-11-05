// Run with `npm test`. The magnet lands on the lines the ruler draws.
import assert from "node:assert/strict";
import { test } from "node:test";
import { lineSpacing, barTickPositions, tickPositions } from "./scale.ts";
import { SNAP_PX, snapToSpacing } from "./snap.ts";

test("a position near a line goes onto it, and one far from it stays", () => {
  // Lines every 25 frames at 2 px a frame: 8 px is 4 frames.
  assert.equal(snapToSpacing(27, 25, 2), 25);
  assert.equal(snapToSpacing(22, 25, 2), 25);
  assert.equal(snapToSpacing(30, 25, 2), 30);
  assert.equal(snapToSpacing(46, 25, 2), 50);
  assert.equal(snapToSpacing(0, 25, 2), 0);
});

test("the reach is in pixels, so it shrinks in frames as the view zooms in", () => {
  // 5 frames from a line: 0.5 px at 0.1 px/frame, 50 px at 10 px/frame.
  assert.equal(snapToSpacing(105, 100, 0.1), 100);
  assert.equal(snapToSpacing(105, 100, 10), 105);
  // Exactly at the reach it still snaps.
  assert.equal(snapToSpacing(104, 100, SNAP_PX / 4), 100);
});

test("a line that is not a whole frame snaps to the nearest frame", () => {
  // A beat of 12.5 frames: the third line is at 37.5, which a position can only be 38 (or 37).
  assert.equal(snapToSpacing(36, 12.5, 4), Math.round(37.5));
  assert.ok(Number.isInteger(snapToSpacing(36, 12.5, 4)));
});

test("a spacing that makes no sense leaves the position alone", () => {
  for (const spacing of [0, -5, Number.NaN]) assert.equal(snapToSpacing(41, spacing, 2), 41);
});

test("the time view snaps onto its drawn lines at every zoom", () => {
  for (const rate of ["fps24", "fps25", "fps2997Drop", "fps30"]) {
    for (const zoom of [0.04, 0.37, 1.6, 7.3, 40]) {
      const spacing = lineSpacing(zoom, rate, null);
      const { major, minor } = tickPositions(zoom, rate, 6_000);
      for (const line of [...major, ...minor]) assert.ok(Math.abs(line / spacing - Math.round(line / spacing)) < 1e-9);
      // Lines are never closer than the graticule allows, so a snap never has two to choose from within reach of 8 px.
      assert.ok(spacing * zoom >= 12 - 1e-9 || minor.length === 0);
    }
  }
});

test("the bars view snaps onto its drawn beats and bars at every zoom", () => {
  const grid = { bpm: 123, beatsPerBar: 4, fps: 25 };
  for (const zoom of [0.04, 0.1, 0.37, 1.6, 7.3, 40]) {
    const spacing = lineSpacing(zoom, "fps25", grid);
    const { major, minor } = barTickPositions(zoom, grid, 20_000);
    for (const line of [...major.map((t) => t.frame), ...minor]) {
      assert.ok(Math.abs(line / spacing - Math.round(line / spacing)) < 1e-6, `${zoom}: ${line} / ${spacing}`);
    }
  }
});

test("zooming in snaps to finer lines in the bars view", () => {
  const grid = { bpm: 120, beatsPerBar: 4, fps: 25 };
  assert.ok(lineSpacing(0.1, "fps25", grid) > lineSpacing(1.6, "fps25", grid));
  assert.equal(lineSpacing(1.6, "fps25", grid), 12.5);
});
