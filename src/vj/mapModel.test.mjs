// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { hasSlot, mappable, sourceKind, sourceLabel, targetKey } from "./mapTarget.ts";

test("targetKey is the same for the same target whatever the order of its fields", () => {
  assert.equal(
    targetKey({ kind: "effectParam", effect: "e1", param: "size" }),
    targetKey({ param: "size", effect: "e1", kind: "effectParam" }),
  );
  assert.notEqual(targetKey({ kind: "layerMaster", layer: "a" }), targetKey({ kind: "layerOpacity", layer: "a" }));
});

test("the knobs of a layer and of a clip follow their slot", () => {
  const knobs = [{ kind: "layerDashboard", layer: "a", knob: 2 }, { kind: "clipDashboard", clip: "c", knob: 0 }];
  for (const knob of knobs) {
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

test("group targets have a slot to follow", () => {
  assert.equal(hasSlot({ kind: "groupBypass", group: "g" }), true);
});
