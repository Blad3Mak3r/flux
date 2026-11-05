// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ENGINE_RATE, createDisplayClock } from "./smooth.ts";

const FPS = 25;
const at = (seconds, state = "playing") => ({
  state,
  frame: 0,
  positionSamples: Math.round(seconds * ENGINE_RATE),
});

test("read has nothing before the backend reported", () => {
  assert.equal(createDisplayClock().read(0), null);
});

test("read projects a playing position forward with the wall clock", () => {
  const clock = createDisplayClock();
  clock.feed(at(10), 1000, FPS, 0);
  assert.equal(clock.read(1000), 250);
  assert.equal(clock.read(1100), 252.5);
});

test("read adds the timeline start to the position", () => {
  const clock = createDisplayClock();
  clock.feed(at(1), 0, FPS, 90_000);
  assert.equal(clock.read(0), 90_025);
});

test("read does not project further than the cap when the backend goes quiet", () => {
  const clock = createDisplayClock();
  clock.feed(at(0), 0, FPS, 0);
  assert.equal(clock.read(10_000), (250 * FPS) / 1000);
});

test("read does not step when a report arrives a little off the projection", () => {
  const clock = createDisplayClock();
  clock.feed(at(0), 0, FPS, 0);
  const before = clock.read(40); // 1 frame
  clock.feed(at(0.04 - 0.02), 40, FPS, 0); // the report puts it 0.5 frame behind
  assert.ok(Math.abs(clock.read(40) - before) < 1e-9);
});

test("read fades a small difference away", () => {
  const clock = createDisplayClock();
  clock.feed(at(0), 0, FPS, 0);
  clock.feed(at(0.02), 40, FPS, 0); // projection was 1 frame, report says 0.5
  const settled = clock.read(40 + 2_000);
  assert.ok(Math.abs(settled - (0.5 + (250 * FPS) / 1000)) < 1e-6);
});

test("read never runs backwards while a small difference fades", () => {
  const clock = createDisplayClock();
  clock.feed(at(0), 0, FPS, 0);
  clock.feed(at(0.0), 80, FPS, 0); // 2 frames behind: the largest difference that is blended
  let previous = clock.read(80);
  for (let t = 81; t < 330; t += 1) {
    const now = clock.read(t);
    assert.ok(now >= previous - 1e-9, `went back at ${t}`);
    previous = now;
  }
});

test("read takes a report far from the projection as a jump", () => {
  const clock = createDisplayClock();
  clock.feed(at(0), 0, FPS, 0);
  clock.feed(at(30), 40, FPS, 0);
  assert.equal(clock.read(40), 750);
});

test("read has nothing once the transport is not playing", () => {
  const clock = createDisplayClock();
  clock.feed(at(5), 0, FPS, 0);
  clock.feed(at(5, "paused"), 10, FPS, 0);
  assert.equal(clock.read(20), null);
});

test("jump forgets the projection", () => {
  const clock = createDisplayClock();
  clock.feed(at(5), 0, FPS, 0);
  clock.jump();
  assert.equal(clock.read(10), null);
});
