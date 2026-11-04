# Releasing Flux Timecode

A release is a GitHub Release with signed installers, plus the download page and the update feed on
GitHub Pages. You choose the version; the workflows do the rest.

| What | Where |
| --- | --- |
| Installers | The GitHub Release: `.msi` (Windows), `.dmg` (macOS, Apple silicon and Intel), `.deb` and `.rpm` (Linux) |
| Download page | `https://blad3mak3r.github.io/flux-timecode/` (English), `/es/` (Spanish); source in `landing/` |
| Update feed | `https://blad3mak3r.github.io/flux-timecode/latest.json`, the endpoint written into `tauri.conf.json` |

## Cutting a release

1. **Actions > Prepare release > Run workflow**, and type the version, for example `0.2.0`.
   It must be newer than the current one and either `X.Y.Z` or `X.Y.Z-N` with a number only
   (`0.3.0-1`): the Windows MSI version is made of numbers, so Tauri rejects `-beta.1`.
2. The workflow opens a pull request, `chore(release): v0.2.0`, that sets the version in
   `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and
   `src-tauri/tauri.conf.json` (`scripts/set-version.mjs`).
3. **Merge that pull request.** The merge changes the version in `tauri.conf.json`, which starts
   `release.yml`:
   - `verify` checks that the manifests agree and that `v0.2.0` has no tag yet;
   - `build` makes the installers on macOS (both architectures), Linux and Windows, signs the
     updater files and attaches them to a **draft** release;
   - `publish` makes the release public (this creates the tag) once every platform has built.
4. For a normal release `publish` then runs `pages.yml`: it reads the newest published release,
   writes `latest.json` and the download table (`landing/scripts/release-site.mjs`), builds the
   landing page and deploys it.

A pre-release (`0.3.0-1`) is built and published as a pre-release, but it is not offered on the
download page or in the update feed: there is no beta channel.

The first release has nothing to bump: run **Actions > Release** by hand and it publishes the
version `main` already carries when that version has no tag.

A change to `landing/` redeploys the page by itself (`pages.yml`), keeping the current release's
links and feed. A failed build leaves a draft release; re-run the failed jobs, or delete the draft
and start again.

## One-time setup

1. The repository is public. Pages is free there and the release downloads need no sign-in.
2. Settings > Actions > General: workflow permissions "Read and write", and "Allow GitHub Actions
   to create and approve pull requests" on. The first lets the release workflows publish; the
   second lets Prepare release open its pull request.
3. Settings > Pages: Source "GitHub Actions".
4. The signing key. Every update is verified against the public key in `tauri.conf.json`
   (`plugins.updater.pubkey`), and updates cannot be unsigned.
   - Generated with `npm run tauri signer generate -- -w ~/.tauri/flux-timecode.key`; the private
     key is never committed.
   - Add its contents as the secret `TAURI_SIGNING_PRIVATE_KEY`, and its password (empty when it
     has none) as `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
   - **If the private key is lost, installed copies can never be updated again**: they only trust
     this public key. To rotate it, ship one release signed with the old key that carries the new
     public key, then switch.
5. Recommended: protect `main` so changes arrive through pull requests.

## What users see

- The launcher window checks for updates when Flux starts, in the **Updates** row. A newer version
  is downloaded and installed before the app opens, and Flux restarts. The user can press **Skip**
  while it downloads. A failed or impossible check (offline, no release yet, a development build)
  never stops Flux from opening.
- Windows installs the MSI for every user in Program Files, so updating asks for administrator
  permission (UAC). Linux installs a `.deb` or `.rpm`, which asks for the password through
  `pkexec` (polkit), or zenity/kdialog with `sudo`. macOS replaces the app without asking.
- Installers are not yet signed with an Apple or Microsoft certificate. Windows SmartScreen and
  macOS Gatekeeper warn on the first launch; the download page says how to get past each.

## Known limits

- The update endpoint is baked into every installed copy. If the site moves (for example to its own
  domain), keep serving `latest.json` at the old URL, or redirect it.
- On Windows, whether Flux opens again by itself after the MSI update has to be checked on a real
  machine.
- The asset names come from `tauri-action` (`releaseAssetNamePattern` in `release.yml`); if they
  change, update `classify` in `landing/scripts/release-site.mjs`.
