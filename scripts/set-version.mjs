// Writes one SemVer version into every file that carries it, or checks that they all agree.
//
//   node scripts/set-version.mjs 0.2.0     bump; the version must be newer than the current one
//   node scripts/set-version.mjs --check   print the version when every file agrees, else fail
//
// Pre-releases are `X.Y.Z-N` with a number only: the Windows MSI version is four numbers, so
// Tauri refuses a pre-release such as `-beta.1` when it builds the installer.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(0|[1-9]\d*))?$/;
const MSI_PRERELEASE_MAX = 65535;

/** `1.2.3` or `1.2.3-4` as `{ core: [1, 2, 3], pre: 4 | null }`. */
export function parseVersion(text) {
  const match = SEMVER.exec(text);
  const pre = match?.[4] === undefined ? null : Number(match[4]);
  if (!match || (pre !== null && pre > MSI_PRERELEASE_MAX)) {
    throw new Error(
      `"${text}" is not a version Flux can release. Use X.Y.Z, or X.Y.Z-N with N up to ${MSI_PRERELEASE_MAX}.`,
    );
  }
  return { core: [Number(match[1]), Number(match[2]), Number(match[3])], pre };
}

/** Negative when `a` is older than `b`; a pre-release is older than its final version. */
export function compareVersions(a, b) {
  const left = parseVersion(a);
  const right = parseVersion(b);
  for (let i = 0; i < 3; i++) {
    if (left.core[i] !== right.core[i]) return left.core[i] - right.core[i];
  }
  if (left.pre === right.pre) return 0;
  if (left.pre === null) return 1;
  if (right.pre === null) return -1;
  return left.pre - right.pre;
}

/** A version written as `<prefix><version><suffix>` that a regular expression can find. */
function pattern(regex) {
  return {
    read(text) {
      const match = regex.exec(text);
      if (!match) throw new Error("version not found");
      return match[2];
    },
    write: (text, version) => text.replace(regex, (_, before, __, after) => before + version + after),
  };
}

const lockfile = {
  read: (text) => JSON.parse(text).version,
  write(text, version) {
    const lock = JSON.parse(text);
    lock.version = version;
    lock.packages[""].version = version;
    return JSON.stringify(lock, null, 2) + (text.endsWith("\n") ? "\n" : "");
  },
};

/** Every file that carries the app version, relative to the repository root. */
export const TARGETS = {
  "package.json": pattern(/^(\s*"version":\s*")([^"]+)(")/m),
  "package-lock.json": lockfile,
  "src-tauri/Cargo.toml": pattern(/^(version = ")([^"]+)(")/m),
  "src-tauri/Cargo.lock": pattern(/(name = "flux-timecode"\r?\nversion = ")([^"]+)(")/),
  "src-tauri/tauri.conf.json": pattern(/^(\s*"version":\s*")([^"]+)(")/m),
};

/** The version every file in `texts` (path -> contents) carries; throws when they differ. */
export function currentVersion(texts) {
  const found = Object.entries(TARGETS).map(([file, target]) => {
    try {
      return [file, target.read(texts[file])];
    } catch (err) {
      throw new Error(`${file}: ${err.message}`);
    }
  });
  const versions = new Set(found.map(([, version]) => version));
  if (versions.size !== 1) {
    const list = found.map(([file, version]) => `  ${file}: ${version}`).join("\n");
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
    Object.entries(TARGETS).map(([file, target]) => [file, target.write(texts[file], version)]),
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
