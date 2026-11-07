// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { canvasSize, renderWave, waveSlice } from "./waveform.ts";

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

const job = { seed: "asset", width: 4, height: 20, x0: 0, dpr: 2, color: "#66d5ba", pxPerSecond: 2, startSeconds: 0 };

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

test("a slice of a wave draws the same columns as the whole of it", () => {
  const peaks = { values: Array.from({ length: 80 }, (_, i) => (i % 2 === 0 ? -(i % 50) : i % 70)), perSecond: 1 };
  const whole = recorder();
  renderWave(whole, { ...job, width: 40, pxPerSecond: 1 }, peaks);
  const slice = recorder();
  renderWave(slice, { ...job, width: 10, x0: 12, pxPerSecond: 1 }, peaks);
  assert.equal(slice.rects.length, 10);
  slice.rects.forEach(([x, top, width, height], index) => {
    assert.deepEqual([x + 12, top, width, height], whole.rects[12 + index]);
  });
});

test("a slice of the placeholder is the same as that part of the whole", () => {
  const whole = recorder();
  renderWave(whole, { ...job, width: 60 });
  const slice = recorder();
  renderWave(slice, { ...job, width: 20, x0: 30 });
  const expected = whole.rects.filter(([x]) => x >= 30 && x < 50).map(([x, ...rest]) => [x - 30, ...rest]);
  assert.deepEqual(slice.rects, expected);
});

test("waveSlice keeps to what is on screen plus a screenful each side, in whole chunks", () => {
  // A clip an hour long at 40 px/s, the view 1500 px wide, scrolled 100 000 px in.
  const width = 3600 * 40;
  const slice = waveSlice(0, width, { left: 100_000, width: 1500 });
  assert.ok(slice.x0 <= 100_000 - 1500 && slice.x1 >= 100_000 + 1500 + 1500);
  assert.ok(slice.x1 - slice.x0 <= 1500 * 3 + 2 * 512);
  assert.equal(slice.x0 % 512, 0);
  assert.equal(slice.x1 % 512, 0);
});

test("waveSlice never reaches outside the wave, and is empty for a clip that is far off screen", () => {
  assert.deepEqual(waveSlice(0, 300, { left: 0, width: 1000 }), { x0: 0, x1: 300 });
  assert.deepEqual(waveSlice(5000, 300, { left: 0, width: 1000 }), { x0: 0, x1: 0 });
  assert.deepEqual(waveSlice(0, 300, { left: 90_000, width: 1000 }), { x0: 0, x1: 0 });
  // A clip that starts left of the view is cut at its own start.
  const cut = waveSlice(-2000, 8000, { left: 0, width: 1000 });
  assert.ok(cut.x0 >= 0 && cut.x1 <= 8000 && cut.x1 > cut.x0);
});

test("waveSlice stays the same while the view moves inside the chunk", () => {
  const a = waveSlice(0, 100_000, { left: 20_000, width: 1500 });
  const b = waveSlice(0, 100_000, { left: 20_020, width: 1500 });
  assert.deepEqual(a, b);
});
