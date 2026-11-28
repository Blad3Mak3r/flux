// Run with `npm test`. The magnet lands on the lines the ruler draws.
import assert from "node:assert/strict";
import { test } from "node:test";
import { lineSpacing, barTickPositions, tickPositions } from "./scale.ts";
import { SNAP_PX, collectTargets, fixedSpacing, nearestTarget, snapMove, snapPoint, snapToSpacing } from "./snap.ts";

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
  const grid = { segments: [{ startFrame: 0, beatFrames: (60 / 123) * 25, beatsPerBar: 4, beatUnit: 4, bpm: 123, firstBar: 1 }] };
  for (const zoom of [0.04, 0.1, 0.37, 1.6, 7.3, 40]) {
    const spacing = lineSpacing(zoom, "fps25", grid);
    const { major, minor } = barTickPositions(zoom, grid, 20_000);
    for (const line of [...major.map((t) => t.frame), ...minor]) {
      assert.ok(Math.abs(line / spacing - Math.round(line / spacing)) < 1e-6, `${zoom}: ${line} / ${spacing}`);
    }
  }
});

test("zooming in snaps to finer lines in the bars view", () => {
  const grid = { segments: [{ startFrame: 0, beatFrames: 12.5, beatsPerBar: 4, beatUnit: 4, bpm: 120, firstBar: 1 }] };
  assert.ok(lineSpacing(0.1, "fps25", grid) > lineSpacing(1.6, "fps25", grid));
  assert.equal(lineSpacing(1.6, "fps25", grid), 12.5);
});

const ALL = new Set(["grid", "items", "playhead", "origin"]);
const frames = (targets) => targets.map((target) => target.frame);

test("targets are ordered, joined on one frame, and leave out the item being moved", () => {
  const items = [
    { id: "a", start: 100, end: 200 },
    { id: "b", start: 200, end: 260 },
    { id: "c", start: 400 },
  ];
  assert.deepEqual(frames(collectTargets(items, { playhead: 150, sources: ALL })), [0, 100, 150, 200, 260, 400]);
  assert.deepEqual(frames(collectTargets(items, { exclude: new Set(["b"]), playhead: null, sources: ALL })), [0, 100, 200, 400]);
});

test("only the sources that are on give targets", () => {
  const items = [{ id: "a", start: 100, end: 200 }];
  assert.deepEqual(frames(collectTargets(items, { playhead: 50, sources: new Set(["playhead"]) })), [50]);
  assert.deepEqual(frames(collectTargets(items, { playhead: 50, sources: new Set(["items", "origin"]) })), [0, 100, 200]);
  assert.deepEqual(collectTargets(items, { playhead: 50, sources: new Set() }), []);
});

test("the nearest target is found on either side, and with none there is none", () => {
  const targets = collectTargets([{ id: "a", start: 100, end: 200 }], { sources: new Set(["items"]) });
  assert.equal(nearestTarget(10, []), null);
  assert.deepEqual(nearestTarget(120, targets), { target: targets[0], distance: 20 });
  assert.deepEqual(nearestTarget(190, targets), { target: targets[1], distance: 10 });
  assert.equal(nearestTarget(5, targets)?.target.frame, 100);
  assert.equal(nearestTarget(900, targets)?.target.frame, 200);
});

test("a point lands on a target within reach, and on the grid when only that is near", () => {
  const targets = collectTargets([{ id: "a", start: 103 }], { sources: new Set(["items"]) });
  // 2 px a frame: reach is 4 frames.
  assert.deepEqual(snapPoint(102, targets, 25, 2), { frame: 103, source: "items" });
  assert.deepEqual(snapPoint(124, targets, 25, 2), { frame: 125, source: "grid" });
  assert.deepEqual(snapPoint(112, targets, 25, 2), { frame: 112, source: null });
});

test("a target beats the grid when it is as near or nearer, and loses when it is farther", () => {
  const near = collectTargets([{ id: "a", start: 26 }], { sources: new Set(["items"]) });
  assert.equal(snapPoint(27, near, 25, 2).source, "items");
  const tie = collectTargets([{ id: "a", start: 27 }], { sources: new Set(["items"]) });
  assert.deepEqual(snapPoint(26, tie, 25, 2), { frame: 27, source: "items" });
  const far = collectTargets([{ id: "a", start: 28 }], { sources: new Set(["items"]) });
  assert.deepEqual(snapPoint(26, far, 25, 2), { frame: 25, source: "grid" });
});

test("with the grid off a point only snaps onto targets", () => {
  assert.deepEqual(snapPoint(24, [], null, 2), { frame: 24, source: null });
  assert.deepEqual(snapPoint(24, [], 0, 2), { frame: 24, source: null });
});

test("a block snaps by its end as well as by its start", () => {
  const targets = collectTargets([{ id: "other", start: 500 }], { sources: new Set(["items"]) });
  // The block is 100 long: its end at 498 is 2 frames from 500, its start (398) is on nothing.
  assert.deepEqual(snapMove(398, 100, targets, null, 2), { frame: 400, start: 400, source: "items", guide: 500 });
  // Its start at 502 is the nearer.
  assert.deepEqual(snapMove(502, 100, targets, null, 2), { frame: 500, start: 500, source: "items", guide: 500 });
});

test("a block takes the edge that has to move least, and the start when they tie", () => {
  const targets = collectTargets([{ id: "a", start: 100 }, { id: "b", start: 203 }], { sources: new Set(["items"]) });
  // start 101 is 1 from 100; end 201 is 2 from 203.
  assert.equal(snapMove(101, 100, targets, null, 2).start, 100);
  // start 102 is 2 from 100; end 202 is 1 from 203.
  assert.deepEqual(snapMove(102, 100, targets, null, 2), { frame: 103, start: 103, source: "items", guide: 203 });
  const tie = collectTargets([{ id: "a", start: 100 }, { id: "b", start: 202 }], { sources: new Set(["items"]) });
  assert.equal(snapMove(101, 100, tie, null, 2).guide, 100);
});

test("a block with nothing in reach stays put and shows no guide", () => {
  assert.deepEqual(snapMove(300, 40, [], null, 2), { frame: 300, start: 300, source: null, guide: null });
});

test("a block with no length snaps by its start only", () => {
  const targets = collectTargets([{ id: "a", start: 60 }], { sources: new Set(["items"]) });
  assert.deepEqual(snapMove(58, 0, targets, null, 2), { frame: 60, start: 60, source: "items", guide: 60 });
});

test("a fixed grid step has the same spacing at every zoom", () => {
  assert.equal(fixedSpacing("frame", 25), 1);
  assert.equal(fixedSpacing("second", 25), 25);
  assert.equal(fixedSpacing("second", 30_000 / 1_001), 30_000 / 1_001);
});

test("a beat that is not a whole frame snaps to a whole frame", () => {
  assert.deepEqual(snapPoint(36, [], 12.5, 4), { frame: Math.round(37.5), source: "grid" });
});

test("markers and the edges of regions are targets when that source is on", () => {
  const marks = [300, 120, 500];
  assert.deepEqual(frames(collectTargets([], { marks, sources: new Set(["markers"]) })), [120, 300, 500]);
  assert.deepEqual(collectTargets([], { marks, sources: new Set(["items", "playhead"]) }), []);
  assert.equal(collectTargets([], { marks, sources: new Set(["markers"]) })[0].source, "markers");
});

test("a position lands on a marker when it is nearer than the grid line", () => {
  const targets = collectTargets([], { marks: [99], sources: new Set(["markers"]) });
  assert.deepEqual(snapPoint(98, targets, 50, 2), { frame: 99, source: "markers" });
  assert.deepEqual(snapPoint(101, targets, 50, 2), { frame: 100, source: "grid" });
});

test("the grid can be a function that finds the nearest line, for lines that are not evenly spaced", () => {
  // Lines at 0, 10, 20, then every 30 from 20: 50, 80.
  const lines = (frame) => (frame < 20 ? Math.round(frame / 10) * 10 : 20 + Math.round((frame - 20) / 30) * 30);
  assert.deepEqual(snapPoint(11, [], lines, 2), { frame: 10, source: "grid" });
  assert.deepEqual(snapPoint(48, [], lines, 2), { frame: 50, source: "grid" });
  assert.deepEqual(snapPoint(35, [], lines, 2), { frame: 35, source: null });
  assert.equal(snapMove(48, 28, [], lines, 2).start, 50);
});
