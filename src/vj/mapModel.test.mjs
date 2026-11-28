// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { hasSlot, linkable, mappable, sourceKind, sourceLabel, targetClass, targetKey } from "./mapTarget.ts";

test("targetKey is the same for the same target whatever the order of its fields", () => {
  assert.equal(
    targetKey({ kind: "effectParam", effect: "e1", param: "size" }),
    targetKey({ param: "size", effect: "e1", kind: "effectParam" }),
  );
  assert.notEqual(targetKey({ kind: "layerMaster", layer: "a" }), targetKey({ kind: "layerOpacity", layer: "a" }));
});

test("targetClass tells levels, switches and actions apart", () => {
  assert.equal(targetClass({ kind: "layerOpacity", layer: "a" }), "continuous");
  assert.equal(targetClass({ kind: "layerBypass", layer: "a" }), "switch");
  assert.equal(targetClass({ kind: "stopAll" }), "action");
  assert.equal(targetClass({ kind: "dashboard", knob: 0 }), "continuous");
});

test("a knob links levels but not another knob", () => {
  assert.equal(linkable({ kind: "layerMaster", layer: "a" }), true);
  assert.equal(linkable({ kind: "dashboard", knob: 1 }), false);
  assert.equal(linkable({ kind: "stopAll" }), false);
});

test("the knobs of a layer and of a clip are levels that no knob links, and they follow their slot", () => {
  const knobs = [{ kind: "layerDashboard", layer: "a", knob: 2 }, { kind: "clipDashboard", clip: "c", knob: 0 }];
  for (const knob of knobs) {
    assert.equal(targetClass(knob), "continuous");
    assert.equal(linkable(knob), false);
    assert.equal(hasSlot(knob), true);
  }
  assert.notEqual(targetKey(knobs[0]), targetKey({ kind: "layerDashboard", layer: "a", knob: 3 }));
});

test("mappable carries the target as JSON and nothing without one", () => {
  assert.deepEqual(mappable({ kind: "stopAll" }), { "data-map": '{"kind":"stopAll"}' });
  assert.deepEqual(mappable(undefined), {});
});

test("a DMX source reads universe:channel, with the fine channel after a slash", () => {
  assert.equal(sourceLabel({ type: "artnet", universe: 1, channel: 37 }), "DMX 1:37");
  assert.equal(sourceLabel({ type: "artnet", universe: 1, channel: 37, fine: 38 }), "DMX 1:37/38");
});

test("a key source names the key by its place, with its modifiers in a fixed order", () => {
  assert.equal(sourceLabel({ type: "key", code: "KeyQ", shift: true }), "Key Shift+Q");
  assert.equal(sourceLabel({ type: "key", code: "Digit5", alt: true, ctrl: true }), "Key Ctrl+Alt+5");
  assert.equal(sourceLabel({ type: "key", code: "Numpad3" }), "Key Num 3");
  assert.equal(sourceLabel({ type: "key", code: "F5" }), "Key F5");
});

test("sources are told apart by kind for the filter", () => {
  assert.equal(sourceKind({ type: "artnet", universe: 0, channel: 1 }), "dmx");
  assert.equal(sourceKind({ type: "key", code: "KeyA" }), "key");
  assert.equal(sourceKind({ type: "osc", address: "/a" }), "osc");
});

test("only a target with an id has a slot to follow", () => {
  assert.equal(hasSlot({ kind: "layerOpacity", layer: "a" }), true);
  assert.equal(hasSlot({ kind: "effectParam", effect: "e", param: "p" }), true);
  assert.equal(hasSlot({ kind: "crossfader" }), false);
  assert.equal(hasSlot({ kind: "dashboard", knob: 1 }), false);
});

test("group targets are told apart by class and have a slot to follow", () => {
  assert.equal(targetClass({ kind: "groupMaster", group: "g" }), "continuous");
  assert.equal(targetClass({ kind: "groupSolo", group: "g" }), "switch");
  assert.equal(hasSlot({ kind: "groupBypass", group: "g" }), true);
});
