// Writes one SemVer version into every file that carries it, or checks that they all agree.
//
//   node scripts/set-version.mjs 0.2.0     bump; the version must be newer than the current one
//   node scripts/set-version.mjs --check   print the version when every file agrees, else fail
//
// Only final versions (X.Y.Z) are accepted: a pre-release is a GitHub Release flag, not a version
// string, so the installers (NSIS, DEB, RPM, AppImage) always get a plain number.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;

/** `1.2.3` as `[1, 2, 3]`. */
export function parseVersion(text) {
  const match = SEMVER.exec(text);
  if (!match) throw new Error(`"${text}" is not a version Flux can release. Use X.Y.Z.`);
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** Negative when `a` is older than `b`, positive when newer, 0 when equal. */
export function compareVersions(a, b) {
  const left = parseVersion(a);
  const right = parseVersion(b);
  for (let i = 0; i < 3; i++) {
    if (left[i] !== right[i]) return left[i] - right[i];
  }
  return 0;
}

/**
 * A place where the version is written. `regex` matches `<before><version><after>` as three
 * groups and must find `count` of them; a miss fails loudly instead of leaving a stale number.
 */
function place(regex, count = 1) {
  const all = new RegExp(regex.source, regex.flags.includes("g") ? regex.flags : `${regex.flags}g`);
  return {
    read(text) {
      const found = [...text.matchAll(all)];
      if (found.length !== count) {
        throw new Error(`expected ${count} version(s), found ${found.length}`);
      }
      return found.map((match) => match[2]);
    },
    write: (text, version) => text.replace(all, (_, before, __, after) => before + version + after),
  };
}

/** The `version` that follows the package name in package.json, package-lock.json and tauri.conf.json. */
const jsonVersion = /^(\s*"version":\s*")([^"]+)(")/m;
const crateVersion = /^(version = ")([^"]+)(")/m;

/** Every file that carries the app version, relative to the repository root. */
export const TARGETS = {
  "package.json": [place(jsonVersion)],
  // The top of the lockfile, then the root package under "packages".
  "package-lock.json": [
    place(/(^\{\r?\n  "name": "flux-timecode",\r?\n  "version": ")([^"]+)(")/),
    place(/(\r?\n    "": \{\r?\n      "name": "flux-timecode",\r?\n      "version": ")([^"]+)(")/),
  ],
  "src-tauri/tauri.conf.json": [place(jsonVersion)],
  "src-tauri/Cargo.toml": [place(crateVersion)],
  "src-tauri/crates/flux-audio/Cargo.toml": [place(crateVersion)],
  "src-tauri/crates/flux-core/Cargo.toml": [place(crateVersion)],
  "src-tauri/crates/flux-engine/Cargo.toml": [place(crateVersion)],
  "src-tauri/crates/flux-project/Cargo.toml": [place(crateVersion)],
  "src-tauri/crates/flux-voice/Cargo.toml": [place(crateVersion)],
  // The app and its five crates.
  "src-tauri/Cargo.lock": [
    place(/(name = "flux-(?:timecode|core|project|audio|engine|voice)"\r?\nversion = ")([^"]+)(")/, 6),
  ],
  "README.md": [
    place(/(<img alt="Version )([^"]+)(")/),
    place(/(img\.shields\.io\/badge\/version-)([^-]+)(-)/),
    place(/(FLUX is in early development \(`)([^`]+)(`\))/),
  ],
};

/** The version every file in `texts` (path -> contents) carries; throws when they differ. */
export function currentVersion(texts) {
  const found = [];
  for (const [file, places] of Object.entries(TARGETS)) {
    for (const target of places) {
      try {
        for (const version of target.read(texts[file])) found.push([file, version]);
      } catch (err) {
        throw new Error(`${file}: ${err.message}`);
      }
    }
  }
  const versions = new Set(found.map(([, version]) => version));
  if (versions.size !== 1) {
    const list = [...new Set(found.map(([file, version]) => `  ${file}: ${version}`))].join("\n");
    throw new Error(`The version differs between files:\n${list}`);
  }
  const [version] = versions;
  parseVersion(version);
  return version;
}

/** `texts` with `version` written everywhere; throws unless it is newer than the current one. */
export function bump(texts, version) {
  parseVersion(version);
  const current = currentVersion(texts);
  if (compareVersions(version, current) <= 0) {
    throw new Error(`${version} is not newer than the current version ${current}.`);
  }
  return Object.fromEntries(
    Object.entries(TARGETS).map(([file, places]) => [
      file,
      places.reduce((text, target) => target.write(text, version), texts[file]),
    ]),
  );
}

function main(argv) {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const texts = Object.fromEntries(
    Object.keys(TARGETS).map((file) => [file, readFileSync(join(root, file), "utf8")]),
  );
  const [arg] = argv;
  if (arg === "--check") {
    console.log(currentVersion(texts));
    return;
  }
  if (!arg) throw new Error("Usage: set-version.mjs <version> | --check");
  const updated = bump(texts, arg);
  for (const [file, text] of Object.entries(updated)) writeFileSync(join(root, file), text);
  console.log(`Version set to ${arg} in ${Object.keys(updated).length} files.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }
}
