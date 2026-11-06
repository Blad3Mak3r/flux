import assert from "node:assert/strict";
import { test } from "node:test";
import { projectInitials } from "./initials.ts";

test("projectInitials uses no more than the first three words", () => {
  assert.equal(projectInitials("MegaSuper Proyecto To Guapo"), "MSP");
});

test("projectInitials handles a single project name", () => {
  assert.equal(projectInitials("Titanium"), "T");
});
