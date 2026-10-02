import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  throw new Error("Expected a release version in MAJOR.MINOR.PATCH format.");
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargoTomlPath = resolve(root, "Cargo.toml");
const cargoLockPath = resolve(root, "Cargo.lock");
const tauriConfigPath = resolve(root, "tauri.conf.json");
const packagePath = resolve(root, "ui/package.json");
const packageLockPath = resolve(root, "ui/package-lock.json");

function replaceOnce(path, source, pattern, replacement) {
  if (!pattern.test(source)) {
    throw new Error(`Could not update the release version in ${path}.`);
  }
  writeFileSync(path, source.replace(pattern, replacement));
}

replaceOnce(
  cargoTomlPath,
  readFileSync(cargoTomlPath, "utf8"),
  /^(version = ")[^"]+("\r?\n)/m,
  `$1${version}$2`,
);
replaceOnce(
  cargoLockPath,
  readFileSync(cargoLockPath, "utf8"),
  /(\[\[package\]\]\r?\nname = "flux"\r?\nversion = ")[^"]+("\r?\n)/,
  `$1${version}$2`,
);

for (const path of [tauriConfigPath, packagePath, packageLockPath]) {
  const json = JSON.parse(readFileSync(path, "utf8"));
  if (typeof json.version !== "string") {
    throw new Error(`Expected a root version in ${path}.`);
  }
  json.version = version;
  if (path === packageLockPath) {
    if (typeof json.packages?.[""]?.version !== "string") {
      throw new Error("Expected a root package version in ui/package-lock.json.");
    }
    json.packages[""].version = version;
  }
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
}
