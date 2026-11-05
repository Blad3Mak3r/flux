import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { compareVersions, publish, releaseOrigin, versionParts } from "./release.mjs";

const scratchDir = (t) => {
  const dir = mkdtempSync(join(tmpdir(), "fxtc-release-test-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
};

test("stable tags and origins exclude prereleases and non-HTTPS URLs", () => {
  assert.equal(compareVersions("1.10.0", "1.9.99"), 1);
  assert.equal(compareVersions("1.0.0", "1.0.0"), 0);
  for (const version of ["01.0.0", "1.0.0-beta.1", "1.0.0+test", "1.0", "v1.0.0"]) {
    assert.throws(() => versionParts(version), /stable/);
  }
  assert.equal(releaseOrigin("https://releases.example.invalid/"), "https://releases.example.invalid");
  for (const url of ["http://example.invalid", "https://a@b.invalid", "https://example.invalid/latest"]) {
    assert.throws(() => releaseOrigin(url), /HTTPS origin/);
  }
});

function delivery(t, active = "1.9.0") {
  const scratch = scratchDir(t);
  const latest = { version: "1.10.0" };
  const manifest = join(scratch, "latest.json");
  writeFileSync(manifest, JSON.stringify(latest));
  const writes = [];
  const verified = [];
  const files = ["Flux Timecode_1.10.0_x64-setup.exe", "SHA256SUMS.txt"];
  const ok = { status: 0, stdout: JSON.stringify({ ETag: '"previous"' }), stderr: "" };
  const conflict = { status: 255, stderr: "(PreconditionFailed)" };
  const stable = "stable/latest.json";
  let current = JSON.stringify({ version: active });
  const store = (op, key, ...args) => {
    if (op === "get-object") {
      assert.equal(key, stable);
      writeFileSync(args[0], current);
      return ok;
    }
    writes.push(key);
    if (key !== stable) {
      assert.deepEqual(args.slice(2, 4), ["--if-none-match", "*"]);
      assert.match(args[5], /immutable/);
      return conflict; // A previous run already uploaded these files.
    }
    assert.deepEqual(args.slice(2, 6), ["--if-match", '"previous"', "--cache-control", "no-store"]);
    assert.equal(verified.length, files.length);
    current = readFileSync(manifest, "utf8");
    return ok;
  };
  const verify = (url) => { verified.push(url); };
  return {
    options: { store, verify, latest, files, assets: scratch, manifest, scratch, origin: "https://example.invalid" },
    writes, verified, stable, activeVersion: () => JSON.parse(current).version,
  };
}

test("publishes the manifest only after all public downloads match, including a retry", async (t) => {
  const { options, writes, verified, stable, activeVersion } = delivery(t);
  await publish(options);
  assert.equal(writes.at(-1), stable);
  assert.equal(activeVersion(), options.latest.version);
  assert.equal(verified[0], "https://example.invalid/stable/1.10.0/Flux%20Timecode_1.10.0_x64-setup.exe");
  writes.length = 0;
  await publish(options);
  assert(!writes.includes(stable));
});

test("an incomplete public release leaves the previous version active", async (t) => {
  const { options, writes, stable, activeVersion } = delivery(t);
  options.verify = () => { throw new Error("Public download differs"); };
  await assert.rejects(publish(options), /Public download differs/);
  assert(!writes.includes(stable));
  assert.equal(activeVersion(), "1.9.0");
});

test("an older version makes no writes", async (t) => {
  const { options, writes } = delivery(t, "2.0.0");
  await assert.rejects(publish(options), /this version or a newer/);
  assert.deepEqual(writes, []);
});

test("prepare and dry-run verify genuine Minisign signatures", {
  skip: spawnSync("minisign", ["-v"]).error?.code === "ENOENT" ? "Minisign is unavailable" : false,
}, (t) => {
  const dir = scratchDir(t);
  const pub = join(dir, "test.pub");
  const secret = join(dir, "test.key");
  const config = join(dir, "test.json");
  const input = join(dir, "incoming");
  const prepared = join(dir, "prepared");
  const { version } = JSON.parse(readFileSync("package.json"));
  mkdirSync(input);
  execFileSync("minisign", ["-GW", "-p", pub, "-s", secret]);
  writeFileSync(config, JSON.stringify({ plugins: { updater: { pubkey: readFileSync(pub).toString("base64") } } }));

  const names = [
    `Flux Timecode_${version}_x64-setup.exe`, `Flux Timecode_${version}_amd64.deb`,
    `Flux Timecode-${version}-1.x86_64.rpm`,
  ];
  for (const name of names) {
    const file = join(input, name);
    const signature = join(dir, "test.sig");
    writeFileSync(file, name);
    execFileSync("minisign", ["-Sm", file, "-s", secret, "-x", signature,
      "-t", `timestamp:1\tfile:${name}\tversion:${version}`]);
    writeFileSync(file + ".sig", readFileSync(signature).toString("base64"));
  }

  const release = (command, ...args) => spawnSync(process.execPath,
    ["scripts/release.mjs", command, "v" + version, ...args], {
      encoding: "utf8",
      env: { ...process.env, RELEASE_BASE_URL: "https://example.invalid", R2_ACCOUNT_ID: "" },
    });
  const preparedResult = release("prepare", input, prepared, "", config);
  assert.equal(preparedResult.status, 0, preparedResult.stderr);
  const latest = JSON.parse(readFileSync(join(prepared, "latest.json")));
  assert.deepEqual(Object.keys(latest.platforms), ["windows-x86_64-nsis", "linux-x86_64-deb", "linux-x86_64-rpm"]);
  assert.equal(latest.platforms["linux-x86_64-deb"].signature, readFileSync(join(input, names[1] + ".sig"), "utf8"));

  const dry = release("dry-run", prepared, config);
  assert.equal(dry.status, 0, dry.stderr);
  assert.match(dry.stdout, /Dry run verified 7 files/);
  writeFileSync(join(prepared, "assets", names[1]), "tampered");
  assert.notEqual(release("dry-run", prepared, config).status, 0);
});

test("first publication creates the manifest conditionally", async (t) => {
  const { options, stable } = delivery(t);
  const store = options.store;
  let promoted = false;
  options.store = (op, key, ...args) => {
    if (op === "get-object") return { status: 255, stderr: "(NoSuchKey)" };
    if (key !== stable) return store(op, key, ...args);
    assert.deepEqual(args.slice(2, 4), ["--if-none-match", "*"]);
    promoted = true;
    return { status: 0 };
  };
  await publish(options);
  assert(promoted);
});

test("a concurrent promotion causes a conditional write failure", async (t) => {
  const { options, stable, activeVersion } = delivery(t);
  const store = options.store;
  options.store = (op, key, ...args) => {
    if (op === "put-object" && key === stable) {
      assert.deepEqual(args.slice(2, 4), ["--if-match", '"previous"']);
      return { status: 255, stderr: "(PreconditionFailed)" };
    }
    return store(op, key, ...args);
  };
  await assert.rejects(publish(options), /PreconditionFailed/);
  assert.equal(activeVersion(), "1.9.0");
});

test("set-version updates only the app versions across manifests and lockfiles", (t) => {
  const dir = scratchDir(t);
  mkdirSync(join(dir, "src-tauri"));
  const files = [
    "package.json", "package-lock.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock",
    "src-tauri/tauri.conf.json", "src-tauri/tauri.windows.conf.json", "src-tauri/tauri.linux.conf.json",
  ];
  const before = Object.fromEntries(files.map((file) => [file, readFileSync(file, "utf8")]));
  for (const [file, contents] of Object.entries(before)) writeFileSync(join(dir, file), contents);

  const oldVersion = JSON.parse(before["package.json"]).version;
  const [major, minor, patch] = versionParts(oldVersion);
  const version = `${major}.${minor}.${patch + 1n}`;
  const script = new URL("./release.mjs", import.meta.url);
  const release = (command, value) => spawnSync(process.execPath, [fileURLToPath(script), command, value], {
    cwd: dir,
    encoding: "utf8",
    env: { ...process.env, RELEASE_BASE_URL: "https://example.invalid" },
  });

  assert.notEqual(release("set-version", oldVersion).status, 0);
  assert.notEqual(release("set-version", version + "-beta").status, 0);
  for (const file of files) assert.equal(readFileSync(join(dir, file), "utf8"), before[file]);
  const result = release("set-version", version);
  assert.equal(result.status, 0, result.stderr);
  const validated = release("validate", "v" + version);
  assert.equal(validated.status, 0, validated.stderr);

  const lockPath = "package-lock.json";
  const lock = JSON.parse(readFileSync(join(dir, lockPath), "utf8"));
  lock.version = lock.packages[""].version = oldVersion;
  assert.deepEqual(lock, JSON.parse(before[lockPath]));
  const cargoPath = "src-tauri/Cargo.lock";
  const cargo = readFileSync(join(dir, cargoPath), "utf8");
  assert.equal(cargo.replace(`name = "flux-timecode"\nversion = "${version}"`,
    `name = "flux-timecode"\nversion = "${oldVersion}"`), before[cargoPath]);
});
