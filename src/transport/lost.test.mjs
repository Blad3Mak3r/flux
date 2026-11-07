// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { isLost } from "./lost.ts";

test("isLost reads the bit of the output", () => {
  assert.equal(isLost(0b10, 0), false);
  assert.equal(isLost(0b10, 1), true);
});

test("isLost reads the highest bit as set, not as a sign", () => {
  assert.equal(isLost(2 ** 31, 31), true);
});

test("isLost gives outputs past the 32nd no bit", () => {
  assert.equal(isLost(0xffffffff, 32), false);
});
