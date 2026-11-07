# Flux Timecode

Desktop application to build timecode tracks from songs, events and cues. It is a hybrid: QLab's cue / show-control
model with a REAPER-style timeline and track interface, in a modern look.

## Scope

- Supports both **MTC** (MIDI Timecode) and **LTC** (Linear/audio Timecode).
- Devices are user-selectable: audio interfaces (LTC and project audio) and MIDI interfaces/ports (MTC).
- Flux is the **master clock and only generates** timecode (LTC out through audio, MTC out through MIDI). Chasing an
  external timecode (input) is out of scope for now. Flux can be **remote controlled** (MIDI controllers, OSC) to start,
  stop and move its own clock; that is not chasing. See [docs/remote-control.md](docs/remote-control.md).

## Views

The timeline has two views, switched with the EDIT | LIVE control (Ctrl+1 / Ctrl+2): **Edit** is the editor, and
**Live** runs the show on stage, read-only apart from going to a cue. The backend owns everything Live shows: it derives
the cues from the project and computes their state and countdowns (`src-tauri/crates/flux-engine/src/cues.rs`), and the UI
(`src/live/`) only draws it. See [docs/timecode-signals.md](docs/timecode-signals.md).

## Project file format

Projects use the `.fxtc` extension. A `.fxtc` is a **zip container** holding:

- `project.json` — the full project description: one or more **timelines** (each with its own tracks, audio and notes,
  frame rate and start offset), the audio assets they share, and the output configuration (MTC/LTC settings, device
  preferences, OSC destinations). Tracks are audio, notes, **MIDI** (timed messages to a port) or **OSC** (timed
  messages to a destination). Only one timeline plays at a time. The backend owns the timeline operations (add, rename, remove,
  select the active one, set its timecode); the UI only shows the project it answers with.
- The **audio files** used by the project's tracks, bundled in the archive so a project is self-contained and portable.

The full spec (layout, fields, versioning, device-name hints) is in [docs/fxtc-format.md](docs/fxtc-format.md); the
code is `src-tauri/crates/flux-project/`.

## Logging and diagnostics

The backend writes a rotating `flux.log` in the app's log folder and the Help menu opens that folder or exports a zip
with the logs and a report of the system and devices. Use `log::info!`/`warn!`/`error!` for events, and **never log
from an audio callback or `flux-audio`/`flux-ltc`/`flux-click`/`flux-mtc`/`flux-midi`/`flux-osc`**: count in an atomic and let the `flux-log`
lane write it. See [docs/diagnostics.md](docs/diagnostics.md).

## Stack

- Rust + Tauri v2 for the backend (`src-tauri/`), a Cargo workspace (see below).
- React + TypeScript for the UI (`src/`).

## Responsibilities: backend vs frontend

**The Rust backend does all the logic and every complex operation; the frontend only shows state and information.**

- Backend: validation, editing operations (add/move/remove/split/retarget), ordering, derived data (cues, labels'
  notation, countdowns), timing, device and network access, file formats. It answers with the resulting state.
- Frontend: renders what the backend sends, forwards the user's intent (a click, a typed value, a drag end) and
  translates words through `src/i18n`. No parsing, sorting, validation rules, scheduling or other processing beyond
  formatting for display.
- A new operation is a backend command that takes the project, applies the change and returns the new state, which the
  UI shows with the `apply` action (so it enters the undo history). Follow `timeline_*`, `clip_edit` and `cue_edit`.
  The pure edits in `src/state/edit.ts` predate this rule: do not grow them with new logic.

## Backend workspace (`src-tauri/`)

`src-tauri/Cargo.toml` is the app crate (Tauri glue: commands, windows, diagnostics, recents) and the workspace root.
The logic lives in `src-tauri/crates/`, none of which depends on Tauri:

| Crate | Holds |
| --- | --- |
| `flux-core` | `FrameRate`, `Timecode`, the sample `Clock`, `ENGINE_RATE`. |
| `flux-project` | The `.fxtc` model, archive, migrations, the editing `Session` and its operations. |
| `flux-audio` | Decoding, waveform peaks, tempo detection. |
| `flux-engine` | Transport, LTC/MTC, mixer, cues, outputs, device lookup, cue firing (MIDI/OSC). |

New heavy or native dependencies (e.g. video) go in a crate of their own. The app crate only wires commands to them.

This is the standard Tauri v2 layout, scaffolded from `create-tauri-app` (`react-ts` template, npm). The UI follows
the "Bench Instrument" design recorded in `PRODUCT.md` and `DESIGN.md`; read them before UI work. Every user-facing
string goes through `src/i18n` (English default, Spanish second), never hard-coded.

## Skills

Installed in `.agents/skills/` (locked in `skills-lock.json`) and linked into `.claude/skills/`. Use them whenever
the task matches:

| Skill | Use it for |
| --- | --- |
| `ponytail` | Any coding task: the smallest change that fully solves it. |
| `impeccable` | Any UI work (design, critique, polish, layout, motion). Create `PRODUCT.md` with `init` and `DESIGN.md` with `document` when it asks for them. |
| `rust-best-practices` | Writing or reviewing Rust code. |
| `vercel-react-best-practices` | Writing or reviewing React code. Ignore Next.js-only advice: this is a Tauri SPA, not Next.js. |
| `find-skills` | Discovering and installing further skills. |

## Agents

- `git-operator` (`.claude/agents/git-operator.md`) performs every git write operation and all pull request work.
  The main session does not commit, push, branch, merge or open PRs itself: it delegates, giving the intent (what
  changed and why), the branch/PR, and whether pushing or opening a PR is wanted.
- The `impeccable-*` agents are driven by the `impeccable` skill.

## Conventions

- English for code, comments, docs, commit messages and PR text.
- Commands (npm, run from the repo root unless noted):
  - `npm install` — install frontend dependencies.
  - `npm run tauri dev` — run the desktop app with hot reload.
  - `npm run tauri build` — production bundle.
  - `npm run typecheck` — TypeScript check (`tsc --noEmit`).
  - `npm test` — frontend unit tests (`node --test`, no dependencies).
  - `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace` — from `src-tauri/`.

## Git workflow

This section is authoritative; `git-operator` follows it.

- **No AI attribution, ever.** No mention of AI, Claude, Codex or any assistant in commits or PRs: no
  `Co-Authored-By:` trailers, no "Generated with ..." footers. This overrides default commit/PR templates.
- **English only** for commit messages, branch names, PR titles and PR descriptions.
- **Never commit or push directly to `main`.** Work on a branch and open a PR.
  - Bootstrap exception: the repository has no commits yet, so the very first scaffold commit may go straight to
    `main` when the user asks for it. Everything after that goes through branches and PRs.
- **Branch names** are `<type>/<short-description>` in lowercase kebab-case, with `<type>` one of `feature`, `fix`,
  `docs`, `chore`, `refactor`, `test`, `perf`, `build`, `ci`, `hotfix` (e.g. `feature/ltc-generator`).
- **Commits** use Conventional-Commit-like subjects: `feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`,
  `build`, `ci`, with an optional scope, imperative mood, lowercase, no trailing period, at most 72 characters
  (e.g. `feat(timeline): add snap-to-grid for cue blocks`). Add a short body (why, not what) when it is not
  self-explanatory.
- **Commit incrementally**: one logical change per commit, not one big commit at the end.
- **Stage explicitly** with `git add <paths>`, never `git add -A` / `git add .`. Never commit
  `.claude/settings.local.json`, `.env*`, credentials, or build output (`src-tauri/target/`, `dist/`,
  `node_modules/`).
- **Pull requests**: open one when the work is ready (base `main`). Title in the same style as commit subjects;
  body with **Summary**, **Changes** and **Validation** (exactly which checks were run and which were not, and why).
  Do not enable auto-merge unless asked.
- **Never use bare `git stash` / `git stash pop`** (the stash is shared across worktrees); prefer a temporary WIP
  commit.
- **No destructive commands** (`reset --hard`, `clean -f`, `checkout .`, `branch -D`, `push --force`) unless
  explicitly requested. When a force push is needed, use `--force-with-lease`, never on `main`.
- **Never skip hooks or signing** (`--no-verify`, `--no-gpg-sign`) unless explicitly told to.
- **Tags and releases** only when explicitly requested; never move or delete an existing tag.
