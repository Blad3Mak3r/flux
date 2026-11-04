// Run with `npm test` in landing/.
import assert from "node:assert/strict";
import { test } from "node:test";
import { buildSite, classify } from "./release-site.mjs";

const url = (name) => `https://github.com/o/r/releases/download/v0.2.0/${name}`;
const asset = (name, digest) => ({ name, size: 1000, browser_download_url: url(name), digest });

const NAMES = [
  "flux-timecode_0.2.0_linux_amd64.deb",
  "flux-timecode_0.2.0_linux_x86_64.rpm",
  "flux-timecode_0.2.0_windows_x64.msi",
  "flux-timecode_0.2.0_macos_aarch64.dmg",
  "flux-timecode_0.2.0_macos_x64.dmg",
  "flux-timecode_0.2.0_macos_aarch64.app.tar.gz",
  "flux-timecode_0.2.0_macos_x64.app.tar.gz",
];
const release = {
  tag_name: "v0.2.0",
  body: "Notes",
  published_at: "2026-11-04T12:00:00Z",
  assets: [
    ...NAMES.flatMap((name) => [asset(name, "sha256:abc"), asset(`${name}.sig`)]),
    asset("something.txt"),
  ],
};
const signatures = Object.fromEntries(NAMES.map((name) => [`${name}.sig`, `sig of ${name}\n`]));

test("classify names what an asset installs and how the updater finds it", () => {
  assert.deepEqual(classify("a_linux_amd64.deb"), { os: "linux", arch: "x86_64", updater: "deb", install: "deb" });
  assert.deepEqual(classify("a_macos_arm64.dmg"), { os: "macos", arch: "aarch64", install: "dmg" });
  assert.deepEqual(classify("a_macos_x64.app.tar.gz"), { os: "macos", arch: "x86_64", updater: "" });
  assert.equal(classify("a_windows_x64.msi.sig"), null);
  assert.equal(classify("notes.txt"), null);
  assert.equal(classify("a_linux.deb"), null);
});

test("latest.json has an entry per updater target with the signature and the release URL", () => {
  const { latest } = buildSite(release, signatures);
  assert.equal(latest.version, "0.2.0");
  assert.equal(latest.pub_date, "2026-11-04T12:00:00Z");
  assert.deepEqual(Object.keys(latest.platforms).sort(), [
    "darwin-aarch64",
    "darwin-x86_64",
    "linux-x86_64-deb",
    "linux-x86_64-rpm",
    "windows-x86_64-msi",
  ]);
  assert.deepEqual(latest.platforms["windows-x86_64-msi"], {
    signature: "sig of flux-timecode_0.2.0_windows_x64.msi",
    url: url("flux-timecode_0.2.0_windows_x64.msi"),
  });
});

test("the downloads list the installers people run, not the update archives", () => {
  const { site } = buildSite(release, signatures);
  assert.deepEqual(
    site.files.map((file) => `${file.os}/${file.arch}/${file.format}`).sort(),
    ["linux/x86_64/deb", "linux/x86_64/rpm", "macos/aarch64/dmg", "macos/x86_64/dmg", "windows/x86_64/msi"],
  );
  assert.equal(site.files[0].sha256, "abc");
  assert.equal(site.version, "0.2.0");
});

test("an installer without a signature is left out of latest.json but still downloadable", () => {
  const { latest, site } = buildSite(release, { ...signatures, "flux-timecode_0.2.0_windows_x64.msi.sig": undefined });
  assert.equal("windows-x86_64-msi" in latest.platforms, false);
  assert.ok(site.files.some((file) => file.format === "msi"));
});

test("a release the updater cannot use is an error", () => {
  assert.throws(() => buildSite(release, {}), /no signed installer/);
});
