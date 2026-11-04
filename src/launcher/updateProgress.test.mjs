// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { PROGRESS_START, formatMegabytes, percent, stepProgress } from "./updateProgress.ts";

test("a download starts empty, with the size when the server gives it", () => {
  assert.deepEqual(stepProgress(PROGRESS_START, { event: "Started", data: { contentLength: 1000 } }), {
    downloaded: 0,
    total: 1000,
  });
  assert.equal(stepProgress(PROGRESS_START, { event: "Started", data: {} }).total, null);
  assert.equal(stepProgress(PROGRESS_START, { event: "Started", data: { contentLength: 0 } }).total, null);
});

test("chunks add up and finishing settles an unknown size", () => {
  let progress = stepProgress(PROGRESS_START, { event: "Started", data: {} });
  progress = stepProgress(progress, { event: "Progress", data: { chunkLength: 300 } });
  progress = stepProgress(progress, { event: "Progress", data: { chunkLength: 200 } });
  assert.deepEqual(progress, { downloaded: 500, total: null });
  assert.deepEqual(stepProgress(progress, { event: "Finished" }), { downloaded: 500, total: 500 });
});

test("percent is whole, capped at 100 and null while the size is unknown", () => {
  assert.equal(percent({ downloaded: 0, total: 1000 }), 0);
  assert.equal(percent({ downloaded: 399, total: 1000 }), 39);
  assert.equal(percent({ downloaded: 2000, total: 1000 }), 100);
  assert.equal(percent({ downloaded: 10, total: null }), null);
  assert.equal(percent({ downloaded: 0, total: 0 }), null);
});

test("formatMegabytes prints decimal megabytes with one decimal", () => {
  assert.equal(formatMegabytes(0), "0.0");
  assert.equal(formatMegabytes(18_400_000), "18.4");
  assert.equal(formatMegabytes(47_249_999), "47.2");
});
