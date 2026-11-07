// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ACTION_TYPES,
  OSC_ADDRESSES,
  acceptsValue,
  addBinding,
  defaultAction,
  isNumbered,
  noteName,
  removeBinding,
  setBindingAction,
  triggerLabel,
  withValue,
} from "./model.ts";

const prefs = () => ({
  midi: {
    enabled: true,
    portName: "APC40",
    portIndex: 0,
    bindings: [{ trigger: { kind: "note", channel: 1, number: 36 }, action: { type: "play" } }],
  },
  osc: { enabled: false, port: 9000 },
});

test("noteName counts middle C as C4, as the MIDI cue tracks do", () => {
  assert.equal(noteName(60), "C4");
  assert.equal(noteName(61), "C#4");
  assert.equal(noteName(0), "C-1");
  assert.equal(noteName(127), "G9");
});

test("triggerLabel writes a trigger as a controller's manual would", () => {
  assert.equal(triggerLabel({ kind: "note", channel: 1, number: 36 }), "Note C2 · ch 1");
  assert.equal(triggerLabel({ kind: "cc", channel: 16, number: 64 }), "CC 64 · ch 16");
  assert.equal(triggerLabel({ kind: "program", channel: 3, number: 5 }), "Program 5 · ch 3");
});

test("only the actions with a number start with one", () => {
  assert.deepEqual(defaultAction("play"), { type: "play" });
  assert.deepEqual(defaultAction("gotoCue"), { type: "gotoCue", value: 1 });
  assert.deepEqual(defaultAction("stepSeconds"), { type: "stepSeconds", value: 1 });
  assert.deepEqual(ACTION_TYPES.filter(isNumbered), ["gotoCue", "stepFrames", "stepSeconds", "selectTimeline"]);
});

test("withValue changes the number of a numbered action and leaves the others alone", () => {
  assert.deepEqual(withValue({ type: "gotoCue", value: 1 }, 4), { type: "gotoCue", value: 4 });
  assert.deepEqual(withValue({ type: "stop" }, 4), { type: "stop" });
});

test("acceptsValue wants a place from 1 for cues and timelines and any step but zero", () => {
  assert.equal(acceptsValue("gotoCue", 1), true);
  assert.equal(acceptsValue("gotoCue", 0), false);
  assert.equal(acceptsValue("selectTimeline", -1), false);
  assert.equal(acceptsValue("stepFrames", -5), true);
  assert.equal(acceptsValue("stepFrames", 0), false);
  assert.equal(acceptsValue("stepSeconds", 1.5), false);
});

test("addBinding puts the new one first, so it wins over an older one on the same pad", () => {
  const before = prefs();
  const learned = { trigger: { kind: "note", channel: 1, number: 36 }, action: { type: "stop" } };
  const after = addBinding(before, learned);
  assert.deepEqual(after.midi.bindings[0], learned);
  assert.equal(after.midi.bindings.length, 2);
  assert.equal(before.midi.bindings.length, 1, "the input is left alone");
  assert.equal(after.osc, before.osc);
});

test("setBindingAction and removeBinding touch only the binding they are given", () => {
  const two = addBinding(prefs(), { trigger: { kind: "cc", channel: 1, number: 7 }, action: { type: "nextCue" } });
  const changed = setBindingAction(two, 1, { type: "pause" });
  assert.deepEqual(changed.midi.bindings.map((b) => b.action.type), ["nextCue", "pause"]);
  assert.deepEqual(removeBinding(two, 0).midi.bindings.map((b) => b.action.type), ["play"]);
  assert.equal(two.midi.bindings.length, 2);
});

test("the OSC addresses are the fixed ones the backend answers to", () => {
  assert.equal(OSC_ADDRESSES.length, ACTION_TYPES.length);
  assert.ok(OSC_ADDRESSES.every(({ address }) => address.startsWith("/flux/")));
  assert.deepEqual(
    OSC_ADDRESSES.filter(({ numbered }) => numbered).map(({ address }) => address),
    ["/flux/cue/goto", "/flux/seek/frames", "/flux/seek/seconds", "/flux/timeline/select"],
  );
});
