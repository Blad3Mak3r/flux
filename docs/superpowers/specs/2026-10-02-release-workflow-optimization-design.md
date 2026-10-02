# Release workflow optimization

## Goal

Keep Linux and Windows packaging coverage while reducing repeated release setup and preventing a tag from being published before its artifacts exist.

## Design

CI retains its two-platform bundle matrix for pull requests and `main`; it moves the Linux runner to `ubuntu-22.04`, the Tauri-recommended stable base for this package set. The Tauri system packages remain installed in each Linux runner because GitHub-hosted runners are ephemeral.

Release preparation only calculates the next version. It no longer installs frontend or Linux dependencies, builds the frontend, compiles Rust, commits, or creates a tag. The bundle matrix checks out the selected `main` revision, stamps that calculated version locally, then builds the Windows and Linux artifacts in parallel. Each bundle job restores the existing npm cache and adds the standard Rust cache.

A small Node script validates a semantic version and synchronizes the version in `Cargo.toml`, `Cargo.lock`, `tauri.conf.json`, `ui/package.json`, and `ui/package-lock.json`. The bundle jobs and the final publish job share this operation, avoiding platform-specific shell logic. After both artifacts upload successfully, the publish job stamps and commits the version, creates and pushes the annotated tag, then creates the GitHub Release from the uploaded artifacts.

## Error handling

An invalid version or an unexpected manifest layout fails the stamping script before a bundle is built or a release is published. A failed bundle blocks the publish job, leaving `main` and tags unchanged.

## Verification

Run the version script with the current version and confirm the working tree stays unchanged. Validate workflow YAML and run Rust and TypeScript production builds. In GitHub Actions, dispatch a release and confirm each bundle reports the requested version, then confirm the commit, tag, and GitHub Release are created only after the artifacts succeed.
