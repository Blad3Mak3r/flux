// Run with `npm test`. English and Spanish are split into one module per area: they must stay in step.
import assert from "node:assert/strict";
import { test } from "node:test";
import { en } from "./en.ts";
import { es } from "./es.ts";

const placeholders = (text) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test("English and Spanish have the same keys", () => {
  assert.deepEqual(Object.keys(es).sort(), Object.keys(en).sort());
});

test("no message is empty", () => {
  for (const [name, catalogue] of [["en", en], ["es", es]]) {
    for (const [key, text] of Object.entries(catalogue)) {
      assert.ok(text.trim().length > 0, `${name} ${key} is empty`);
    }
  }
});

test("both languages use the same {placeholders}", () => {
  for (const key of Object.keys(en)) {
    assert.deepEqual(placeholders(es[key]), placeholders(en[key]), key);
  }
});
