import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { TARGETS, bump, compareVersions, currentVersion, parseVersion } from "./set-version.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const repo = () =>
  Object.fromEntries(Object.keys(TARGETS).map((file) => [file, readFileSync(join(root, file), "utf8")]));

test("parseVersion accepts X.Y.Z only", () => {
  assert.deepEqual(parseVersion("1.20.3"), [1, 20, 3]);
  for (const bad of ["1.2", "v1.2.3", "1.2.3-1", "1.2.3-beta.1", "01.2.3", ""]) {
    assert.throws(() => parseVersion(bad), /not a version/, bad);
  }
});

test("compareVersions orders by number, not text", () => {
  assert.ok(compareVersions("0.10.0", "0.9.0") > 0);
  assert.ok(compareVersions("1.0.0", "1.0.1") < 0);
  assert.equal(compareVersions("2.3.4", "2.3.4"), 0);
});

test("the repository carries one version everywhere", () => {
  assert.match(currentVersion(repo()), /^\d+\.\d+\.\d+$/);
});

test("bump writes the new version into every file and nothing else", () => {
  const before = repo();
  const current = currentVersion(before);
  const next = "99.1.2";
  const after = bump(before, next);
  assert.equal(currentVersion(after), next);
  for (const file of Object.keys(TARGETS)) {
    assert.equal(after[file].replaceAll(next, current), before[file], file);
  }
});

test("bump refuses a version that is not newer", () => {
  const texts = repo();
  const current = currentVersion(texts);
  assert.throws(() => bump(texts, current), /not newer/);
  assert.throws(() => bump(texts, "0.0.1"), /not newer/);
  assert.throws(() => bump(texts, "1.0.0-1"), /not a version/);
});

test("a file with a different version is reported", () => {
  const texts = repo();
  texts["package.json"] = texts["package.json"].replace(/"version": "[^"]+"/, '"version": "0.0.9"');
  assert.throws(() => currentVersion(texts), /differs between files[\s\S]*package\.json: 0\.0\.9/);
});

test("a file that lost its version fails loudly", () => {
  const texts = repo();
  texts["README.md"] = texts["README.md"].replace("early development (`", "early development (");
  assert.throws(() => currentVersion(texts), /README\.md: expected 1 version/);
});
