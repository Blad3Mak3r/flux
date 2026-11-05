import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import { copyFile, mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const configPath = "src-tauri/tauri.conf.json";
const manifestKey = "stable/latest.json";
const sumsFile = "SHA256SUMS.txt";
const installers = {
  "windows-x86_64-nsis": /_x64-setup\.exe$/,
  "linux-x86_64-deb": /_amd64\.deb$/,
  "linux-x86_64-rpm": /\.x86_64\.rpm$/,
};

const readText = (path) => readFile(path, "utf8");
const readJson = async (path) => JSON.parse(await readText(path));
const json = (value) => JSON.stringify(value, null, 2) + "\n";

export function versionParts(version) {
  if (typeof version !== "string" || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
    throw new Error("Expected a stable X.Y.Z version");
  }
  return version.split(".").map(BigInt);
}

export function compareVersions(a, b) {
  const left = versionParts(a);
  const right = versionParts(b);
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) return left[i] > right[i] ? 1 : -1;
  }
  return 0;
}

export function releaseOrigin(value) {
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.href !== url.origin + "/") {
    throw new Error("RELEASE_BASE_URL must be an HTTPS origin");
  }
  return url.origin;
}

function cargoVersion(lock, name) {
  const section = lock.split("[[package]]").find((entry) => entry.includes(`name = "${name}"\n`));
  return section?.match(/^version = "([^"]+)"/m)?.[1];
}

async function versionFiles(expected) {
  const [config, pkg, npmLock, cargo, cargoLock] = await Promise.all([
    readText(configPath), readJson("package.json"), readJson("package-lock.json"),
    readText("src-tauri/Cargo.toml"), readText("src-tauri/Cargo.lock"),
  ]);
  const version = expected ?? pkg.version;
  versionParts(version);
  const versions = {
    [configPath]: JSON.parse(config).version,
    "package.json": pkg.version,
    "package-lock.json": npmLock.version,
    "package-lock.json (root)": npmLock.packages[""].version,
    "src-tauri/Cargo.toml": cargo.match(/^version = "([^"]+)"/m)?.[1],
    "src-tauri/Cargo.lock": cargoVersion(cargoLock, pkg.name),
  };
  for (const [file, actual] of Object.entries(versions)) {
    if (actual !== version) throw new Error(`${file} version ${actual} does not match ${version}`);
  }
  return { config, pkg, npmLock, cargo, cargoLock };
}

export async function setVersion(version) {
  versionParts(version);
  const { config, pkg, npmLock, cargo, cargoLock } = await versionFiles();
  if (compareVersions(version, pkg.version) <= 0) {
    throw new Error(`Release version must be newer than ${pkg.version}`);
  }
  const versionLine = /^version = "[^"]+"/m;
  const sections = cargoLock.split("[[package]]");
  const app = sections.findIndex((section) => section.includes(`name = "${pkg.name}"\n`));
  sections[app] = sections[app].replace(versionLine, `version = "${version}"`);
  pkg.version = npmLock.version = npmLock.packages[""].version = version;

  await Promise.all([
    writeFile(configPath, config.replace(/"version": "[^"]+"/, `"version": "${version}"`)),
    writeFile("package.json", json(pkg)),
    writeFile("package-lock.json", json(npmLock)),
    writeFile("src-tauri/Cargo.toml", cargo.replace(versionLine, `version = "${version}"`)),
    writeFile("src-tauri/Cargo.lock", sections.join("[[package]]")),
  ]);
}

async function validate(tag) {
  if (!tag?.startsWith("v")) throw new Error("Expected a vX.Y.Z tag");
  const version = tag.slice(1);
  const [files, windows, linux] = await Promise.all([
    versionFiles(version), readJson("src-tauri/tauri.windows.conf.json"), readJson("src-tauri/tauri.linux.conf.json"),
  ]);
  const { npmLock, cargoLock } = files;
  const config = JSON.parse(files.config);
  const { bundle, plugins: { updater } } = config;
  if (config.identifier !== "com.fluxtimecode.app" || config.productName !== "Flux Timecode" ||
      bundle.active || bundle.targets.length !== 0 || bundle.createUpdaterArtifacts !== true ||
      !windows.bundle.active || windows.bundle.targets.join() !== "nsis" ||
      !linux.bundle.active || [...linux.bundle.targets].sort().join() !== "deb,rpm" ||
      bundle.windows.nsis.installMode !== "perMachine" ||
      bundle.windows.nsis.installerHooks !== "installer-hooks.nsh" ||
      updater.windows.installMode !== "passive" || updater.requireSignedVersion !== true) {
    throw new Error("Tauri release configuration does not match the supported installation policy");
  }
  if (!/^2\.\d+\.\d+$/.test(cargoVersion(cargoLock, "tauri-plugin-updater")) ||
      compareVersions(cargoVersion(cargoLock, "tauri-plugin-updater"), "2.10.0") < 0 ||
      compareVersions(npmLock.packages["node_modules/@tauri-apps/cli"].version, "2.12.4") < 0) {
    throw new Error("Tauri lockfiles must support native Linux updates and version-bound signatures");
  }
  return version;
}

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

function matchInstallers(names) {
  return Object.entries(installers).map(([platform, pattern]) => {
    const matches = names.filter((name) => pattern.test(name));
    if (matches.length !== 1 || !/^[\w .()-]+$/.test(matches[0])) {
      throw new Error(`Expected exactly one x64 installer for ${platform}`);
    }
    return [platform, matches[0]];
  });
}

async function checkAssets(assets, version, origin, scratch, config = configPath) {
  const names = await readdir(assets);
  const pairs = matchInstallers(names);
  const expected = pairs.flatMap(([, name]) => [name, name + ".sig"]);
  if (names.filter((name) => name !== sumsFile).sort().join("\n") !== expected.sort().join("\n")) {
    throw new Error("Expected only the three installers and their matching .sig files");
  }

  const pubkey = join(scratch, "updater.pub");
  const sigFile = join(scratch, "minisign.sig");
  await writeFile(pubkey, Buffer.from((await readJson(config)).plugins.updater.pubkey, "base64"));
  const platforms = {};
  for (const [platform, name] of pairs) {
    const signature = (await readText(join(assets, name + ".sig"))).trim();
    const decoded = Buffer.from(signature, "base64");
    if (decoded.toString("base64") !== signature) throw new Error(`Invalid .sig encoding for ${name}`);
    await writeFile(sigFile, decoded);
    execFileSync("minisign", ["-Vm", join(assets, name), "-p", pubkey, "-x", sigFile, "-q"]);
    const comment = decoded.toString().split(/\r?\n/)[2]?.split("\t") ?? [];
    if (!comment.includes(`file:${name}`) || !comment.includes(`version:${version}`)) {
      throw new Error(`Signature for ${name} does not bind its file name and release version`);
    }
    platforms[platform] = { url: `${origin}/stable/${version}/${encodeURIComponent(name)}`, signature };
  }

  const checksums = await Promise.all(expected.map(async (name) =>
    `${await sha256(join(assets, name))}  ${name}\n`));
  return { platforms, checksums: checksums.join(""), files: [...expected, sumsFile] };
}

async function prepare(input, output, version, origin, scratch, notes, config) {
  const assets = join(output, "assets");
  await mkdir(assets, { recursive: true });
  for (const entry of await readdir(input, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    await copyFile(join(entry.parentPath, entry.name), join(assets, entry.name), constants.COPYFILE_EXCL);
  }

  const { platforms, checksums } = await checkAssets(assets, version, origin, scratch, config);
  await writeFile(join(assets, sumsFile), checksums);
  await writeFile(join(output, "latest.json"), json({
    version,
    notes: notes?.trim() || `Flux Timecode ${version}.`,
    pub_date: new Date().toISOString(),
    platforms,
  }));
}

function s3(bucket, account) {
  if (!/^[a-f0-9]{32}$/.test(account ?? "")) throw new Error("Invalid R2_ACCOUNT_ID");
  if (!/^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$/.test(bucket)) throw new Error("Invalid R2_BUCKET");
  return (operation, key, ...args) => spawnSync("aws", [
    "s3api", operation, "--endpoint-url", `https://${account}.r2.cloudflarestorage.com`,
    "--region", "auto", "--no-cli-pager", "--output", "json", "--bucket", bucket, "--key", key, ...args,
  ], { encoding: "utf8" });
}

function awsOk(result) {
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr?.trim() || "R2 request failed");
}

function contentType(file) {
  if (file.endsWith(".exe")) return "application/vnd.microsoft.portable-executable";
  if (file.endsWith(".deb")) return "application/vnd.debian.binary-package";
  if (file.endsWith(".rpm")) return "application/x-rpm";
  return "text/plain; charset=utf-8";
}

export async function publish({ store, latest, files, assets, manifest, origin, scratch, verify }) {
  const existing = join(scratch, "existing");
  const current = store("get-object", manifestKey, existing);
  let condition = ["--if-none-match", "*"];
  let alreadyPromoted = false;
  if (!current.stderr?.includes("(NoSuchKey)")) {
    awsOk(current);
    const etag = JSON.parse(current.stdout).ETag;
    if (!etag) throw new Error("R2 did not return the current manifest ETag");
    const currentText = await readText(existing);
    const comparison = compareVersions(latest.version, JSON.parse(currentText).version);
    if (comparison < 0 || (comparison === 0 && currentText !== await readText(manifest))) {
      throw new Error("The stable channel already contains this version or a newer one");
    }
    alreadyPromoted = comparison === 0;
    condition = ["--if-match", etag];
  }

  const prefix = `stable/${latest.version}`;
  for (const name of files) {
    const key = `${prefix}/${name}`;
    const body = join(assets, name);
    const upload = store("put-object", key, "--body", body, "--if-none-match", "*",
      "--cache-control", "public, max-age=31536000, immutable", "--content-type", contentType(name));
    // A retry may encounter assets from a partial attempt. Public verification below
    // requires byte-for-byte equality even when R2 refuses to overwrite them.
    if (!upload.stderr?.includes("(PreconditionFailed)")) awsOk(upload);
  }

  for (const name of files) {
    await verify(`${origin}/${prefix}/${encodeURIComponent(name)}`, join(assets, name));
  }
  if (!alreadyPromoted) {
    awsOk(store("put-object", manifestKey, "--body", manifest, ...condition,
      "--cache-control", "no-store", "--content-type", "application/json"));
  }
}

async function verifyPublic(url, file) {
  const response = await fetch(url, { redirect: "error", signal: AbortSignal.timeout(180_000) });
  if (!response.ok || !response.body) throw new Error(`Public download failed (${response.status}): ${url}`);
  const cache = response.headers.get("cache-control") ?? "";
  if (!/\bimmutable\b/i.test(cache) || !/\bmax-age=31536000\b/i.test(cache)) {
    throw new Error(`Incorrect cache policy: ${url}`);
  }
  const hash = createHash("sha256");
  for await (const chunk of response.body) hash.update(chunk);
  if (hash.digest("hex") !== await sha256(file)) throw new Error(`Public download differs: ${url}`);
}

async function deliver(output, version, origin, scratch, dryRun, config) {
  const assets = join(output, "assets");
  const manifest = join(output, "latest.json");
  const latest = await readJson(manifest);
  const { platforms, checksums, files } = await checkAssets(assets, version, origin, scratch, config);
  if (latest.version !== version || typeof latest.notes !== "string" || !latest.notes ||
      Number.isNaN(Date.parse(latest.pub_date)) ||
      json(latest.platforms) !== json(platforms) || await readText(join(assets, sumsFile)) !== checksums) {
    throw new Error("Prepared manifest or checksums do not match the verified installers");
  }
  if (dryRun) {
    console.log(`Dry run verified ${files.length} files for ${version} and ${origin}/${manifestKey}`);
    return;
  }
  if (!process.env.AWS_ACCESS_KEY_ID || !process.env.AWS_SECRET_ACCESS_KEY) {
    throw new Error("R2 S3 credentials are missing");
  }
  await publish({
    store: s3(process.env.R2_BUCKET || "fxtc-releases", process.env.R2_ACCOUNT_ID),
    latest, files, assets, manifest, origin, scratch, verify: verifyPublic,
  });
  console.log(`Stable manifest for ${version}: ${origin}/${manifestKey}`);
}

async function main([command, tag, ...args]) {
  if (command === "set-version") {
    await setVersion(tag);
    return;
  }
  const version = await validate(tag);
  const base = process.env.RELEASE_BASE_URL;
  const origin = releaseOrigin(base);
  if (command === "validate") {
    const [output] = args;
    if (output) await writeFile(output, json({ plugins: { updater: { endpoints: [`${origin}/${manifestKey}`] } } }));
    return;
  }

  const scratch = await mkdtemp(join(tmpdir(), "fxtc-release-"));
  try {
    if (command === "prepare") {
      const [input, output, notesFile, testConfig] = args;
      await prepare(input, output, version, origin, scratch, notesFile && await readText(notesFile), testConfig);
    } else if (command === "publish" || command === "dry-run") {
      const [output, testConfig] = args;
      if (command === "publish" && testConfig) throw new Error("Publishing requires the production signing key");
      await deliver(output, version, origin, scratch, command === "dry-run", testConfig);
    } else {
      throw new Error("Expected validate, prepare, dry-run or publish");
    }
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
