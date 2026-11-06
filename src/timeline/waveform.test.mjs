// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { canvasSize, renderWave } from "./waveform.ts";

/** A context that only records what is drawn. */
function recorder() {
  const rects = [];
  const log = [];
  return {
    rects,
    log,
    setTransform: (...args) => log.push(["setTransform", ...args]),
    clearRect: (...args) => log.push(["clearRect", ...args]),
    fillRect: (...args) => rects.push(args),
    set fillStyle(value) {
      log.push(["fillStyle", value]);
    },
  };
}

const job = { seed: "asset", width: 4, height: 20, dpr: 2, color: "#66d5ba", pxPerSecond: 2, startSeconds: 0 };

test("renderWave sets up the context before drawing", () => {
  const context = recorder();
  renderWave(context, job, { values: [-128, 127], perSecond: 1 });
  assert.deepEqual(context.log, [
    ["setTransform", 2, 0, 0, 2, 0, 0],
    ["clearRect", 0, 0, 4, 20],
    ["fillStyle", "#66d5ba"],
  ]);
});

test("renderWave draws one column per pixel from the lowest low and highest high", () => {
  const context = recorder();
  // Two pairs; each covers two pixel columns at 2 px per second and 1 pair per second.
  renderWave(context, job, { values: [-128, 127, -64, 64], perSecond: 1 });
  assert.equal(context.rects.length, 4);
  // Full scale spans the lane minus a 3 px margin on both sides.
  assert.deepEqual(context.rects[0], [0, 3, 1, 14]);
  assert.deepEqual(context.rects[1], [1, 3, 1, 14]);
  const [x, top, width, height] = context.rects[2];
  assert.equal(x, 2);
  assert.equal(width, 1);
  assert.ok(Math.abs(top - (10 - (64 / 127) * 7)) < 1e-9);
  assert.ok(Math.abs(top + height - (10 + (64 / 128) * 7)) < 1e-9);
});

test("renderWave draws a clip trimmed in from the part of the audio where it starts", () => {
  const context = recorder();
  // The second pair is the first one the clip shows when it skips one second.
  renderWave(context, { ...job, startSeconds: 1 }, { values: [-128, 127, -64, 64], perSecond: 1 });
  assert.equal(context.rects.length, 4 - 2);
  assert.ok(context.rects[0][2] === 1 && context.rects[0][1] > 3);
});

test("renderWave stops where the audio ends", () => {
  const context = recorder();
  renderWave(context, { ...job, width: 40 }, { values: [-128, 127, -64, 64], perSecond: 1 });
  assert.equal(context.rects.length, 4);
});

test("renderWave draws a column at least one pixel tall for silence", () => {
  const context = recorder();
  renderWave(context, job, { values: [0, 0], perSecond: 1 });
  assert.ok(context.rects.every(([, , , height]) => height >= 1));
});

test("the placeholder is the same for the same seed and different for another", () => {
  const draw = (seed) => {
    const context = recorder();
    renderWave(context, { ...job, seed, width: 60 }, undefined);
    return JSON.stringify(context.rects);
  };
  assert.equal(draw("a"), draw("a"));
  assert.notEqual(draw("a"), draw("b"));
});

test("canvasSize scales by the pixel ratio and lowers the resolution of a very wide canvas", () => {
  assert.deepEqual(canvasSize(100, 50, 2), { dpr: 2, pixelWidth: 200, pixelHeight: 100 });
  const wide = canvasSize(32_000, 80, 2);
  assert.equal(wide.dpr, 0.5);
  assert.equal(wide.pixelWidth, 16_000);
  assert.equal(canvasSize(100, 50, 0).dpr, 1);
});
