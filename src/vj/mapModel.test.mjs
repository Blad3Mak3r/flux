// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { linkable, mappable, targetClass, targetKey } from "./mapTarget.ts";

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

test("mappable carries the target as JSON and nothing without one", () => {
  assert.deepEqual(mappable({ kind: "stopAll" }), { "data-map": '{"kind":"stopAll"}' });
  assert.deepEqual(mappable(undefined), {});
});
