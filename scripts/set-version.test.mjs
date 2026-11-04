// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { TARGETS, bump, compareVersions, currentVersion, parseVersion } from "./set-version.mjs";

const fixture = (version) => ({
  "package.json": `{\n  "name": "flux-timecode",\n  "private": true,\n  "version": "${version}",\n  "type": "module"\n}\n`,
  "package-lock.json":
    JSON.stringify({ name: "flux-timecode", version, packages: { "": { name: "flux-timecode", version } } }, null, 2) +
    "\n",
  "src-tauri/Cargo.toml": `[package]\nname = "flux-timecode"\nversion = "${version}"\n\n[dependencies]\nserde = { version = "1" }\n`,
  "src-tauri/Cargo.lock": `[[package]]\nname = "cpal"\nversion = "9.9.9"\n\n[[package]]\nname = "flux-timecode"\nversion = "${version}"\n`,
  "src-tauri/tauri.conf.json": `{\n  "productName": "Flux Timecode",\n  "version": "${version}",\n  "identifier": "x"\n}\n`,
});

test("parseVersion accepts X.Y.Z and a numeric pre-release", () => {
  assert.deepEqual(parseVersion("1.20.3"), { core: [1, 20, 3], pre: null });
  assert.deepEqual(parseVersion("0.3.0-1"), { core: [0, 3, 0], pre: 1 });
});

test("parseVersion rejects what SemVer or the MSI cannot carry", () => {
  for (const bad of ["1.0", "v1.0.0", "01.0.0", "1.0.0-beta.1", "1.0.0+build", "1.0.0-65536", ""]) {
    assert.throws(() => parseVersion(bad), /not a version/, bad);
  }
});

test("compareVersions orders the core numerically and puts a pre-release before its release", () => {
  assert.ok(compareVersions("0.10.0", "0.9.0") > 0);
  assert.ok(compareVersions("1.0.0", "0.99.99") > 0);
  assert.ok(compareVersions("1.0.0-2", "1.0.0") < 0);
  assert.ok(compareVersions("1.0.0-2", "1.0.0-10") < 0);
  assert.equal(compareVersions("1.2.3", "1.2.3"), 0);
});

test("currentVersion reads the one version every file carries", () => {
  assert.equal(currentVersion(fixture("0.1.0")), "0.1.0");
});

test("currentVersion names the files that disagree", () => {
  const texts = { ...fixture("0.1.0"), "src-tauri/Cargo.toml": fixture("0.2.0")["src-tauri/Cargo.toml"] };
  assert.throws(() => currentVersion(texts), /package\.json: 0\.1\.0[\s\S]*Cargo\.toml: 0\.2\.0/);
});

test("bump writes the version to every file and leaves other versions alone", () => {
  const updated = bump(fixture("0.1.0"), "0.2.0");
  assert.equal(currentVersion(updated), "0.2.0");
  assert.match(updated["src-tauri/Cargo.toml"], /serde = \{ version = "1" \}/);
  assert.match(updated["src-tauri/Cargo.lock"], /name = "cpal"\nversion = "9\.9\.9"/);
  assert.equal(JSON.parse(updated["package-lock.json"]).packages[""].version, "0.2.0");
  assert.ok(updated["package-lock.json"].endsWith("\n"));
});

test("bump refuses an equal or older version", () => {
  assert.throws(() => bump(fixture("0.2.0"), "0.2.0"), /not newer/);
  assert.throws(() => bump(fixture("0.2.0"), "0.1.9"), /not newer/);
});

test("every target is covered by the fixture", () => {
  assert.deepEqual(Object.keys(fixture("0.1.0")).sort(), Object.keys(TARGETS).sort());
});
