# Releasing and automatic updates

FLUX is published as a GitHub Release of [Blad3Mak3r/flux](https://github.com/Blad3Mak3r/flux). The installed app
checks that Release for a newer version and can install it (About FLUX > Check for updates).

## One-time setup

The updater only installs files signed with the project's key. The public half is in `src-tauri/tauri.conf.json`
(`plugins.updater.pubkey`); the private half is a repository secret.

1. If you need a new key pair: `npm run tauri signer generate -- -w ~/.tauri/flux.key`. Put the printed public key in
   `plugins.updater.pubkey`. Never commit the private key. **Losing it, or replacing it, means installed apps can no longer
   update to new releases.**
2. In the repository, Settings > Secrets and variables > Actions, add:
   - `TAURI_SIGNING_PRIVATE_KEY`: the content of the private key file.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password (an empty secret if the key has none).
3. The release workflow pushes the version commit to `main` with the workflow token, so `main` must allow that (no
   branch protection that requires a pull request for everyone, or an exception for GitHub Actions). The workflow also
   needs Settings > Actions > General > Workflow permissions set to *Read and write*.

## Publishing a version

Actions > **Release** > Run workflow (from `main`):

| Input | |
| --- | --- |
| `version` | `X.Y.Z`, newer than the current one. Nothing else is accepted: no `v` prefix, no `-beta.1`. |
| `prerelease` | Marks the Release as a pre-release. The updater endpoint is `releases/latest`, which ignores those, so no installed app is offered it. |

The workflow then:

1. **Prepare**: runs `node scripts/set-version.mjs <version>`, which writes the version into `package.json`,
   `package-lock.json`, `src-tauri/tauri.conf.json`, the Cargo manifests of the app and its crates, `src-tauri/Cargo.lock`
   and the README, and commits it to `release/vX.Y.Z`. `main` is not touched yet.
2. **Build** on Windows and Linux, from that commit, into a *draft* Release.
3. **Publish**: when both builds worked, moves `main` to the version commit, publishes the Release (this creates the tag
   `vX.Y.Z`) and deletes `release/vX.Y.Z`.

If a build fails, `main` is as it was. Fix the cause and run the workflow again with the same version: the branch is
replaced and the draft Release is reused. If `main` got new commits while the builds ran, `publish` refuses to move it;
run the workflow again to build on top of them.

`node scripts/set-version.mjs --check` prints the current version, or lists the files that disagree.

## What the Release holds

File names are the ones Tauri gives them, so the exact spelling can differ.

| File | For |
| --- | --- |
| `FLUX_<version>_x64-setup.exe` | Windows installer (NSIS). |
| `FLUX_<version>_amd64.deb`, `FLUX-<version>-1.x86_64.rpm` | Linux packages. |
| `FLUX_<version>_amd64.AppImage` | Linux, portable. |
| `*.sig` | Signatures of the files the updater downloads. |
| `latest.json` | What the app reads: the newest version and, per platform, the file to download and its signature. |

## How each platform updates

- **Windows**: downloads the installer and runs it in passive mode (a progress bar, no questions), then FLUX restarts.
- **Linux AppImage**: replaces the AppImage in place, then FLUX restarts.
- **Linux DEB / RPM**: Tauri cannot update these. Install the new package with the system's package manager.
- **macOS**: not built yet.

The app refuses to install while the transport is playing, so an update cannot cut the timecode in the middle of a show.

The Linux build runs on `ubuntu-latest`, like CI, so its binaries need that image's glibc or newer. To support older
distributions, pin the runner to an older Ubuntu in `release.yml`, once the audio crates are checked to build there.
