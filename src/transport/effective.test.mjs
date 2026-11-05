import assert from "node:assert/strict";
import test from "node:test";
import { effectiveOutput } from "./effective.ts";

const output = {
  audio: { deviceName: "Main", channels: [1, 2] },
  preview: { deviceName: "Phones", channels: [3, 4] },
  ltc: [{ deviceName: "Main", channel: 8 }],
  mtc: [{ portName: "Bus" }],
};

test("edit with a preview device plays only there, with no timecode", () => {
  assert.deepEqual(effectiveOutput(output, "edit"), {
    audio: { deviceName: "Phones", channels: [3, 4] },
    ltc: [],
    mtc: [],
  });
});

test("live ignores the preview device", () => {
  assert.equal(effectiveOutput(output, "live"), output);
});

test("edit without a preview device plays like live", () => {
  const { preview, ...plain } = output;
  assert.equal(effectiveOutput(plain, "edit"), plain);
});

test("edit with a preview device keeps the metronome on its own device", () => {
  const metronome = { deviceName: "Band", channels: [3, 4], gainDb: -6 };
  assert.deepEqual(effectiveOutput({ ...output, metronome }, "edit"), {
    audio: { deviceName: "Phones", channels: [3, 4] },
    ltc: [],
    mtc: [],
    metronome,
  });
  assert.equal(effectiveOutput({ ...output, metronome }, "live").metronome, metronome);
});
