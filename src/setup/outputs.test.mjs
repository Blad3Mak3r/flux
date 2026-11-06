// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  addLtc,
  addMtc,
  clearMetronome,
  clearPreview,
  freeChannel,
  isAvailable,
  portChoices,
  portKey,
  portLabel,
  removeLtc,
  removeMtc,
  setMetronomeChannels,
  setMetronomeDevice,
  setMetronomeGain,
  setMtcPort,
  setPreviewDevice,
  setProgrammeDevice,
  stepSection,
  updateLtc,
  updateMtc,
} from "./outputs.ts";

const output = () => ({
  audio: { deviceName: "Main", channels: [1, 2] },
  ltc: [{ deviceName: "Main", channel: 8 }],
  mtc: [{ portName: "IAC" }],
});

test("freeChannel takes the highest channel up to eight that is not taken", () => {
  assert.equal(freeChannel([], 16), 8);
  assert.equal(freeChannel([8], 16), 7);
  assert.equal(freeChannel([], 4), 4);
  assert.equal(freeChannel([4, 3], 4), 2);
  assert.equal(freeChannel([1], 1), 1);
});

test("addLtc avoids the channels LTC and the programme already use on that device", () => {
  const before = output();
  const after = addLtc(before, "Main", 8);
  assert.deepEqual(after.ltc, [
    { deviceName: "Main", channel: 8 },
    { deviceName: "Main", channel: 7 },
  ]);
  assert.equal(before.ltc.length, 1);

  const stereo = addLtc({ audio: { deviceName: "Main", channels: [1, 2] }, ltc: [], mtc: [] }, "Main", 2);
  assert.equal(stereo.ltc[0].channel, 2, "the programme holds 1 and 2, so a two channel device reuses the top one");
});

test("addLtc on another device ignores the programme's channels", () => {
  const after = addLtc(output(), "Booth", 2);
  assert.deepEqual(after.ltc[1], { deviceName: "Booth", channel: 2 });
});

test("updateLtc and removeLtc touch only the output they are given", () => {
  const two = addLtc(output(), "Booth", 4);
  const changed = updateLtc(two, 1, { channel: 1 });
  assert.equal(changed.ltc[1].channel, 1);
  assert.equal(changed.ltc[0], two.ltc[0]);
  assert.deepEqual(removeLtc(two, 0).ltc, [two.ltc[1]]);
});

test("addMtc picks the first port that is not in use, or none", () => {
  assert.deepEqual(addMtc(output(), ["IAC", "Bus 2"]).mtc[1], { portName: "Bus 2" });
  assert.deepEqual(addMtc(output(), ["IAC"]).mtc[1], { portName: "" });
  assert.deepEqual(addMtc({ ...output(), mtc: [] }, []).mtc, [{ portName: "" }]);
});

test("ports that share a name are told apart by their place among them", () => {
  const names = ["Hub", "Other", "Hub"];
  const choices = portChoices(names);
  assert.deepEqual(choices, [{ portName: "Hub" }, { portName: "Other" }, { portName: "Hub", portIndex: 1 }]);
  assert.deepEqual(choices.map((choice) => portLabel(choice, choices)), ["Hub (1)", "Other", "Hub (2)"]);
  assert.deepEqual(addMtc({ ...output(), mtc: [{ portName: "Hub" }] }, names).mtc[1], { portName: "Other" });
  const both = addMtc(addMtc({ ...output(), mtc: [{ portName: "Hub" }] }, names), names);
  assert.deepEqual(both.mtc[2], { portName: "Hub", portIndex: 1 });
});

test("setMtcPort keeps the index only for a port after the first of its name", () => {
  const picked = setMtcPort(output(), 0, portKey({ portName: "Hub", portIndex: 1 }));
  assert.deepEqual(picked.mtc[0], { portName: "Hub", portIndex: 1 });
  assert.deepEqual(setMtcPort(picked, 0, portKey({ portName: "Hub" })).mtc[0], { portName: "Hub" });
  assert.deepEqual(setMtcPort(picked, 0, "").mtc[0], { portName: "" });
});

test("updateLtc sets the level of one output only", () => {
  const two = addLtc(output(), "Main", 8);
  const changed = updateLtc(two, 1, { level: 0.25 });
  assert.equal(changed.ltc[1].level, 0.25);
  assert.equal(changed.ltc[0], two.ltc[0]);
});

test("updateMtc and removeMtc touch only the output they are given", () => {
  const two = addMtc(output(), ["IAC", "Bus 2"]);
  assert.equal(updateMtc(two, 0, { portName: "X" }).mtc[1], two.mtc[1]);
  assert.deepEqual(removeMtc(two, 1).mtc, [{ portName: "IAC" }]);
});

test("setProgrammeDevice moves the LTC outputs that shared the old device, and only those", () => {
  const before = { ...output(), ltc: [{ deviceName: "Main", channel: 8 }, { deviceName: "Booth", channel: 1 }] };
  const after = setProgrammeDevice(before, "Other", 4);
  assert.deepEqual(after.audio, { deviceName: "Other", channels: [1, 2] });
  assert.deepEqual(after.ltc, [{ deviceName: "Other", channel: 4 }, { deviceName: "Booth", channel: 1 }]);
  assert.deepEqual(setProgrammeDevice(before, "Mono", 1).audio, { deviceName: "Mono", channels: [1] });
});

test("stepSection wraps round and skips the section that is not available", () => {
  assert.equal(isAvailable("video"), false);
  assert.equal(stepSection("project", 1), "audio");
  assert.equal(stepSection("general", 1), "timeline");
  assert.equal(stepSection("timeline", -1), "general");
  assert.equal(stepSection("project", -1), "timeline");
});

test("setPreviewDevice sets only the preview and leaves the programme and timecode alone", () => {
  const before = output();
  const after = setPreviewDevice(before, "Phones", 2);
  assert.deepEqual(after.preview, { deviceName: "Phones", channels: [1, 2] });
  assert.deepEqual({ ...after, preview: undefined }, { ...before, preview: undefined });
  assert.deepEqual(setPreviewDevice(before, "Mono", 1).preview.channels, [1]);
});

test("clearPreview drops the preview and keeps the rest", () => {
  const withPreview = setPreviewDevice(output(), "Phones", 2);
  assert.deepEqual(clearPreview(withPreview), output());
  assert.ok(!("preview" in clearPreview(withPreview)));
});

test("the metronome gets its own device, on its first pair, without touching the programme", () => {
  const before = output();
  const after = setMetronomeDevice(before, "Band", 8);
  assert.deepEqual(after.metronome, { deviceName: "Band", channels: [1, 2] });
  assert.deepEqual(after.audio, before.audio);
  assert.deepEqual(after.ltc, before.ltc);
  assert.equal(before.metronome, undefined);
  assert.deepEqual(setMetronomeDevice(before, "Mono", 1).metronome.channels, [1]);
});

test("changing the metronome device keeps its level", () => {
  const loud = setMetronomeGain(setMetronomeDevice(output(), "Band", 4), -6);
  assert.equal(setMetronomeDevice(loud, "Other", 4).metronome.gainDb, -6);
});

test("the metronome channels and level only change a metronome that exists", () => {
  const none = output();
  assert.equal(setMetronomeChannels(none, [3, 4]), none);
  assert.equal(setMetronomeGain(none, -3), none);
  const set = setMetronomeDevice(none, "Band", 8);
  assert.deepEqual(setMetronomeChannels(set, [3, 4]).metronome.channels, [3, 4]);
  assert.equal(setMetronomeGain(set, -3).metronome.gainDb, -3);
});

test("0 dB on the metronome is stored as no value, and no metronome output leaves the key out", () => {
  const set = setMetronomeGain(setMetronomeDevice(output(), "Band", 4), -3);
  assert.ok(!("gainDb" in setMetronomeGain(set, 0).metronome));
  assert.ok(!("metronome" in clearMetronome(set)));
});
