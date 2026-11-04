// Turns the newest published GitHub Release into what the site publishes:
//   public/latest.json     the updater endpoint the app checks at launch
//   src/data/release.json  the version and download links the landing page prints
//
//   node scripts/release-site.mjs <release.json> <signatures dir> <landing dir> <downloads url>
//
// <release.json> is `gh api repos/{repo}/releases/latest`; <signatures dir> holds the release's
// `.sig` files. The repository is private, so its release assets cannot be downloaded without
// signing in: release.yml copies the installers to the downloads host (`<downloads url>/<tag>/<file>`)
// and every link here points there.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

/** The CPU architecture named in a file name, in the names the updater uses. */
function archOf(name) {
  if (/aarch64|arm64/i.test(name)) return "aarch64";
  if (/x86_64|x64|amd64/i.test(name)) return "x86_64";
  return null;
}

/**
 * What a release asset is for, or `null` for the ones the site does not use.
 * `updater` is the part of the `latest.json` key after the architecture; `install` is how people
 * install it (the downloads table); both are optional.
 */
export function classify(name) {
  const arch = archOf(name);
  if (!arch) return null;
  if (name.endsWith(".deb")) return { os: "linux", arch, updater: "deb", install: "deb" };
  if (name.endsWith(".rpm")) return { os: "linux", arch, updater: "rpm", install: "rpm" };
  if (name.endsWith(".msi")) return { os: "windows", arch, updater: "msi", install: "msi" };
  if (name.endsWith(".dmg")) return { os: "macos", arch, install: "dmg" };
  // macOS updates through the app archive, whose key has no bundle suffix.
  if (name.endsWith(".app.tar.gz")) return { os: "macos", arch, updater: "" };
  return null;
}

const updaterKey = ({ os, arch, updater }) =>
  [os === "macos" ? "darwin" : os, arch, updater].filter(Boolean).join("-");

/**
 * @param release the Release as the GitHub API returns it
 * @param signatures file name -> contents of the `.sig` file
 * @param downloadsUrl where the installers are served from, without a trailing slash
 */
export function buildSite(release, signatures, downloadsUrl) {
  const version = String(release.tag_name).replace(/^v/, "");
  const platforms = {};
  const files = [];

  for (const asset of release.assets) {
    const kind = classify(asset.name);
    if (!kind) continue;
    const url = `${downloadsUrl}/${release.tag_name}/${encodeURIComponent(asset.name)}`;
    if (kind.updater !== undefined) {
      const signature = signatures[`${asset.name}.sig`];
      if (signature) platforms[updaterKey(kind)] = { signature: signature.trim(), url };
      else console.warn(`No signature for ${asset.name}: it is left out of latest.json.`);
    }
    if (kind.install) {
      files.push({
        name: asset.name,
        url,
        size: asset.size,
        sha256: /^sha256:/.test(asset.digest ?? "") ? asset.digest.slice("sha256:".length) : null,
        os: kind.os,
        arch: kind.arch,
        format: kind.install,
      });
    }
  }
  if (Object.keys(platforms).length === 0) {
    throw new Error(`Release ${release.tag_name} has no signed installer the updater can use.`);
  }

  return {
    // Tauri checks the whole file, so every entry above is complete.
    latest: { version, notes: release.body ?? "", pub_date: release.published_at, platforms },
    site: { version, tag: release.tag_name, publishedAt: release.published_at, files },
  };
}

function main([releaseFile, signaturesDir, landingDir, downloadsUrl]) {
  if (!releaseFile || !signaturesDir || !landingDir || !downloadsUrl) {
    throw new Error("Usage: release-site.mjs <release.json> <signatures dir> <landing dir> <downloads url>");
  }
  const release = JSON.parse(readFileSync(releaseFile, "utf8"));
  const signatures = {};
  for (const asset of release.assets) {
    if (!asset.name.endsWith(".sig")) continue;
    signatures[asset.name] = readFileSync(join(signaturesDir, asset.name), "utf8");
  }

  const { latest, site } = buildSite(release, signatures, downloadsUrl.replace(/\/$/, ""));
  const write = (file, data) => {
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, JSON.stringify(data, null, 2) + "\n");
  };
  write(join(landingDir, "public", "latest.json"), latest);
  write(join(landingDir, "src", "data", "release.json"), site);
  console.log(`Release ${site.tag}: ${Object.keys(latest.platforms).length} update targets, ${site.files.length} downloads.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    console.error(err.message);
    process.exit(1);
  }
}
