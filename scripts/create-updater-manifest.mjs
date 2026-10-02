import { readdir, readFile, writeFile } from "node:fs/promises";
import { basename, join, relative, resolve } from "node:path";

const [version, tag, distArgument] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  throw new Error("Expected a release version in MAJOR.MINOR.PATCH format.");
}
if (!/^v\d+\.\d+\.\d+$/.test(tag ?? "")) {
  throw new Error("Expected a release tag in vMAJOR.MINOR.PATCH format.");
}
if (!distArgument) {
  throw new Error("Expected the distribution directory.");
}

const dist = resolve(distArgument);

async function filesWithin(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesWithin(path) : [path];
  }));
  return nested.flat();
}

function requireOne(files, description, predicate) {
  const matches = files.filter(predicate);
  if (matches.length !== 1) {
    throw new Error(`Expected exactly one ${description}; found ${matches.length}.`);
  }
  return matches[0];
}

function releaseAssetUrl(repository, asset) {
  return `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(basename(asset))}`;
}

const repository = process.env.GITHUB_REPOSITORY;
if (!repository) {
  throw new Error("GITHUB_REPOSITORY must be set when generating the updater manifest.");
}

const files = await filesWithin(dist);
const windowsBundle = requireOne(files, "Windows NSIS updater bundle", (file) => file.endsWith("-setup.exe"));
const linuxBundle = requireOne(files, "Linux AppImage updater bundle", (file) => file.endsWith(".AppImage"));
const windowsSignature = `${windowsBundle}.sig`;
const linuxSignature = `${linuxBundle}.sig`;
const fileSet = new Set(files);
for (const signature of [windowsSignature, linuxSignature]) {
  if (!fileSet.has(signature)) {
    throw new Error(`Missing updater signature: ${relative(dist, signature)}`);
  }
}

const manifest = {
  version,
  notes: `Flux ${tag}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: (await readFile(windowsSignature, "utf8")).trim(),
      url: releaseAssetUrl(repository, windowsBundle),
    },
    "linux-x86_64": {
      signature: (await readFile(linuxSignature, "utf8")).trim(),
      url: releaseAssetUrl(repository, linuxBundle),
    },
  },
};

await writeFile(join(dist, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
