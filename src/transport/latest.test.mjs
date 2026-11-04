// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { latestOnly } from "./latest.ts";

/** A `send` that resolves when told to, so a test decides when a call is "in flight". */
function gated() {
  const sent = [];
  const gates = [];
  const send = (value) => {
    sent.push(value);
    return new Promise((resolve) => gates.push(resolve));
  };
  const finish = async () => {
    gates.shift()();
    await new Promise((resolve) => setImmediate(resolve));
  };
  return { sent, send, finish };
}

test("latestOnly sends the first value at once", () => {
  const { sent, send } = gated();
  latestOnly(send, assert.fail)(1);
  assert.deepEqual(sent, [1]);
});

test("latestOnly skips what is overtaken while a call is in flight and sends the newest", async () => {
  const { sent, send, finish } = gated();
  const call = latestOnly(send, assert.fail);
  call(1);
  call(2);
  call(3);
  call(4);
  assert.deepEqual(sent, [1]);
  await finish();
  assert.deepEqual(sent, [1, 4]);
  await finish();
  assert.deepEqual(sent, [1, 4]);
});

test("latestOnly sends a falsy newest value", async () => {
  const { sent, send, finish } = gated();
  const call = latestOnly(send, assert.fail);
  call(5);
  call(0);
  await finish();
  assert.deepEqual(sent, [5, 0]);
});

test("latestOnly reports a failure and keeps going", async () => {
  const errors = [];
  const sent = [];
  const call = latestOnly(
    (value) => {
      sent.push(value);
      return value === 1 ? Promise.reject(new Error("no output")) : Promise.resolve();
    },
    (error) => errors.push(error.message),
  );
  call(1);
  await new Promise((resolve) => setImmediate(resolve));
  call(2);
  assert.deepEqual(errors, ["no output"]);
  assert.deepEqual(sent, [1, 2]);
});
