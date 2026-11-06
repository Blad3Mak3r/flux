# Release distribution

The private GitHub repository builds Flux Timecode in GitHub Actions. Cloudflare R2 serves the signed x64 installers and the Tauri update feed over HTTPS. The public bucket contains release files only. The workflow uses the S3-compatible API for distribution. GitHub write access is scoped to the delivery job that pushes the version commit and tag.

## Configure GitHub and R2

1. Create the R2 bucket `fxtc-releases` and attach a public custom domain. Use a dedicated release origin, such as `https://fxtc.blademaker.tv`.
2. Set the repository Actions variables `RELEASE_BASE_URL` to that HTTPS origin and `R2_BUCKET` to the bucket name. The default bucket name is `fxtc-releases`. The build embeds the feed URL from `RELEASE_BASE_URL`, so keep the old feed reachable if changing domains for installed clients.
3. Configure Cloudflare Cache Rules to bypass caching for `/stable/latest.json` and to respect the origin cache policy on versioned objects. Avoid negative caching for release files. Versioned objects use `public, max-age=31536000, immutable` and the feed uses `no-store`.
4. Create an R2 S3 API token with Object Read & Write scoped to this bucket. Set `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` as Actions secrets in the `stable` environment. The runner uses AWS CLI v2 with `s3api put-object --if-none-match` and `--if-match` support.
5. In the `release-signing` environment, set `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` using the existing keypair. Its public key is already in `src-tauri/tauri.conf.json`. If the key has no password, the password value may be omitted. Keep the private key in the secret store.
6. Use a GitHub tag ruleset to restrict creation, deletion and updates of `v*` tags to release maintainers. Allow `main` and stable tags in the `stable` environment. The manual release uses its built-in token with `contents: write`. The repository rules must permit that token to push the generated version commit to `main` and create the stable tag. Protect existing tags against changes and deletion. Restrict `release-signing` to trusted release tags, `main` and trusted refs for dry-runs.

## Publish a stable release

In GitHub, select **Actions → Release → Run workflow**, choose `main`, and enter the next stable version, such as `1.2.0`. The workflow updates `package.json`, both root versions in `package-lock.json`, `src-tauri/Cargo.toml`, the app entry in `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`. Use three numeric SemVer components and a version higher than the current version on `main`.

After the builds and signature checks pass, the workflow pushes the version commit directly to `main` and creates `vX.Y.Z` at the same commit in one atomic Git push. If `main` advanced during the build or GitHub rejects either update, start a new run after resolving the cause. The same run delivers the tagged release to R2 and uses a generic release note.

Enable **dry_run** to build and verify a version without updating `main`, creating a tag or publishing. The private `prepared-release` artifact contains the installers and manifest. Signing secrets are required and R2 credentials are optional for a dry-run. Use `base_url` to point dry-run installers at a test origin. If both the input and the repository variable are unset, a dry-run uses `https://releases.example.invalid`.

The tag-push workflow also accepts a stable tag that points to a commit already on `main` with matching versions. An annotated tag supplies the public release notes. Use the acceptance checks below before publishing the first release.

The workflow validates the lockfiles, versions, bundle settings and tests. Windows builds the NSIS installer and Linux builds DEB and RPM. Tauri CLI signs the original installer bytes and puts the filename and version in the signed Minisign comment. Preparation verifies each signature against the public key and records SHA-256 for each installer and signature.

The delivery job uploads under `stable/X.Y.Z/`, then downloads every file anonymously over the public origin and compares its hash and cache policy. It writes `stable/latest.json` only after all files match. GitHub serializes the delivery job. Conditional R2 writes also enforce create-once uploads and check the manifest ETag at promotion time, protecting against concurrent writers. Older versions cannot replace the active version.

## Installers and updater

- **Windows x64.** NSIS uses `perMachine` and requests elevation through UAC. The x64 install directory is `%ProgramFiles%\Flux Timecode` and the installer rejects another destination. Tauri registers the uninstaller under HKLM and creates the Start menu shortcut. The application runs in the user's session and stores its webview data and recent projects in user directories. Projects save at the path the user chooses. Keep the application identifier, product name, executable name and install location consistent across releases.
- **Windows updates.** Tauri verifies the download before starting the installer with its passive update arguments. The installer requests UAC when necessary and updates the machine installation. The plugin exits the old process after launching the installer. NSIS relaunches Flux as the signed-in user when installation succeeds.
- **Linux x64.** The Tauri bundler embeds the package type in the executable. The locked updater selects the matching platform entry and installs DEB with `dpkg -i` or RPM with `rpm -U`, then Flux relaunches. A graphical PolicyKit agent with `pkexec` provides the normal privilege prompt, with Tauri's sudo fallbacks where available. For a clean install, use `apt install ./<file>.deb` or `dnf install ./<file>.rpm` with appropriate privileges to resolve dependencies.

The Minisign updater signature is separate from Windows Authenticode and Linux package-manager signing. To add Authenticode later, provision a certificate or signing service on the Windows runner and configure Tauri's `bundle.windows.signCommand` in a Windows-only build overlay. Let the bundler sign the executable and installer before Tauri generates the updater signature.

## Public layout and manifest

```text
stable/
  latest.json
  X.Y.Z/
    <NSIS installer>.exe
    <NSIS installer>.exe.sig
    <DEB installer>.deb
    <DEB installer>.deb.sig
    <RPM installer>.rpm
    <RPM installer>.rpm.sig
    SHA256SUMS.txt
```

`latest.json` contains `version`, `notes`, `pub_date` (UTC ISO 8601) and a `platforms` object. The platform keys are `windows-x86_64-nsis`, `linux-x86_64-deb` and `linux-x86_64-rpm`. Each entry has an absolute HTTPS `url` and the original base64 `signature` from the corresponding `.sig`. Spaces in Tauri's installer names are URL-encoded. The updater requires the artifact signature to bind the manifest version.

## Local checks and dry-run

Run `npm ci`, `npm run typecheck`, `npm test`, `cargo fmt --check`, `cargo clippy --locked --all-targets` and `cargo test --locked` (Cargo commands from `src-tauri`). Install Minisign to exercise the signature test. It generates and removes its own temporary keypair.

On a machine with Minisign, flatten or collect the three native bundles and their signatures under an `incoming` directory. With `RELEASE_BASE_URL` set, run the following from the repository root, using the current version in place of `vX.Y.Z`.

```sh
node scripts/release.mjs validate vX.Y.Z
node scripts/release.mjs prepare vX.Y.Z incoming prepared
node scripts/release.mjs dry-run vX.Y.Z prepared
```

For an isolated test build, a Tauri config overlay can provide a temporary public key and test feed URL. Use the corresponding private key to build. Pass the same overlay as the last argument to `prepare` (use an empty string for the optional notes-file argument) and to `dry-run`. The publish command uses only the production key. Keep the test private key out of the workspace and artifacts.

## Install and update acceptance checks

Use disposable machines and two signed consecutive builds pointed at a separate test origin with the same paths. Start with the first version installed. Publish the second version to the test origin after recording the first version's package identity and data paths.

| System | Clean install | Update |
| --- | --- | --- |
| Windows x64, signed in as a standard user | Start NSIS from Explorer. Enter administrator credentials at UAC. Check Program Files, the HKLM uninstall registration, Start menu shortcut and Installed Apps. Launch Flux and confirm the process runs with the standard user's identity and token. Try an alternate path and confirm the installer rejects it. | Open the old version from the same standard account. Confirm that the launcher downloads the signed installer and UAC requests administrator credentials. Check that Flux closes, updates the same directory and relaunches under the original user without elevation. Verify one HKLM entry, no app copy in AppData and the same project, recents and preferences. Exercise UAC cancellation and uninstall from Installed Apps. |
| Debian or Ubuntu x64 desktop | Install the DEB with APT. Record `dpkg-query -W flux-timecode`, open a saved project and record its recent-project entry. | Check that the DEB URL is used, the privilege prompt appears and `dpkg-query` shows the new version. Verify the project, recents and preferences after relaunch. |
| Fedora x64 desktop | Install the RPM with DNF. Record `rpm -q flux-timecode` and the same user data. | Check that the RPM URL is used, the privilege prompt appears and `rpm -q` shows the new version. Verify the same user data after relaunch. |

In the test origin, also try a corrupted installer or mismatched signature. The updater must leave the previous version runnable. For a delivered release, use `curl -I` to check the feed's `no-store` header and a versioned object's cache policy. Download all versioned files into a single directory and run `sha256sum -c SHA256SUMS.txt`. Check that the previous version is still downloadable.

## Recovery

If a build or verification fails before promotion, the current stable manifest remains in place. Resolve the error and rerun **only the failed delivery job** to reuse the original `prepared-release` artifact. If the version commit and tag already reached GitHub, the retry checks that the tag identifies the same version commit on `main` before continuing. An existing versioned file can be reused when the public bytes and cache policy match. For a cache-related failure, correct the cache rule or purge the stale entry before retrying.

A conflict at promotion means the manifest changed since the job read it. Rerun delivery to recheck the active version. If the artifact has expired, the tag points to a broken build or the versioned bytes differ, prepare a corrected higher version through the normal release process. Keep earlier artifacts available. To recover from a promoted bad build, ship a higher version containing the fix or revert, so installed clients can upgrade to it.

## References

- [Tauri Updater 2.14.1 platform selection and installers](https://github.com/tauri-apps/plugins-workspace/blob/updater-v2.14.1/plugins/updater/src/updater.rs)
- [Tauri CLI 2.12.4 updater signing](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-cli/src/bundle.rs)
- [Cloudflare R2 conditional writes](https://developers.cloudflare.com/r2/reference/consistency/#conditional-writes)
