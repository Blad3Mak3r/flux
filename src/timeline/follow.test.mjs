// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ANCHOR, createFollower } from "./follow.ts";

const VIEW = { width: 1000, max: 100_000 };
const ANCHOR_PX = VIEW.width * ANCHOR;

/** Runs `frames` drawn frames at 60 Hz with the playhead moving `speed` px per second. */
function run(follower, x0, speed, frames, t0 = 0) {
  const scrolls = [];
  for (let i = 0; i < frames; i += 1) {
    const now = t0 + i * (1000 / 60);
    scrolls.push(follower.step(x0 + (speed * (now - t0)) / 1000, now, VIEW));
  }
  return scrolls;
}

test("step leaves the scroll alone while the playhead moves in the free zone", () => {
  const scrolls = run(createFollower(0), 100, 40, 60);
  assert.ok(scrolls.every((scroll) => scroll === 0));
});

test("step starts scrolling once the playhead reaches the anchor and keeps it there", () => {
  const follower = createFollower(0);
  const scrolls = run(follower, ANCHOR_PX - 10, 120, 240);
  const last = scrolls.length - 1;
  const x = ANCHOR_PX - 10 + (120 * (last * (1000 / 60))) / 1000;
  assert.ok(Math.abs(x - scrolls[last] - ANCHOR_PX) < 1e-6);
});

test("step moves the scroll by a small amount every frame while flowing", () => {
  const scrolls = run(createFollower(0), ANCHOR_PX, 600, 120);
  for (let i = 1; i < scrolls.length; i += 1) {
    const delta = scrolls[i] - scrolls[i - 1];
    assert.ok(delta > 0 && delta < 12, `frame ${i} moved ${delta}`);
  }
});

test("step flows at a steady speed, with no step the size of a page", () => {
  const scrolls = run(createFollower(0), ANCHOR_PX, 40, 600);
  const deltas = scrolls.slice(1).map((scroll, i) => scroll - scrolls[i]);
  assert.ok(Math.max(...deltas) < 1.5);
});

test("step slides to a playhead that was sent out of view instead of jumping", () => {
  const follower = createFollower(0);
  run(follower, 100, 0, 5);
  const scrolls = [];
  for (let i = 0; i < 90; i += 1) scrolls.push(follower.step(20_000, 100 + i * (1000 / 60), VIEW));
  assert.ok(scrolls[0] < 20_000 - ANCHOR_PX, "the first frame must not be at the destination");
  assert.ok(scrolls.every((scroll, i) => i === 0 || scroll >= scrolls[i - 1]), "slides one way");
  assert.ok(Math.abs(scrolls[89] - (20_000 - ANCHOR_PX)) < 5, "and arrives");
});

test("step starts a slide from at most a page and a half away", () => {
  const follower = createFollower(0);
  const first = follower.step(90_000, 0, VIEW);
  assert.ok(first >= 90_000 - ANCHOR_PX - VIEW.width * 1.5 - 1e-6);
});

test("step slides back to a playhead sent behind the view", () => {
  const follower = createFollower(50_000);
  const scroll = follower.step(10_000, 0, VIEW);
  assert.ok(scroll > 10_000 - ANCHOR_PX, "does not jump to the destination");
  assert.ok(scroll < 50_000, "but heads back");
});

test("step never scrolls past the ends of the timeline", () => {
  const follower = createFollower(0);
  assert.equal(follower.step(5, 0, VIEW), 0);
  const end = createFollower(99_900).step(200_000, 0, VIEW);
  assert.ok(end <= VIEW.max);
});

test("sliding is true until the slide has settled", () => {
  const follower = createFollower(0);
  follower.step(20_000, 0, VIEW);
  assert.equal(follower.sliding, true);
  for (let i = 1; i < 400; i += 1) follower.step(20_000, i * (1000 / 60), VIEW);
  assert.equal(follower.sliding, false);
});

test("sync takes the scroll the view really has", () => {
  const follower = createFollower(0);
  follower.sync(5_000);
  assert.equal(follower.step(5_100, 0, VIEW), 5_000);
});
