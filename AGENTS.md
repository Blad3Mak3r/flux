# Flux Timecode

Desktop application to build timecode tracks from songs, events and cues. It is a hybrid: QLab's cue / show-control
model with a REAPER-style timeline and track interface, in a modern look. The repository is greenfield (no code yet).

## Scope

- Supports both **MTC** (MIDI Timecode) and **LTC** (Linear/audio Timecode).
- Devices are user-selectable: audio interfaces (LTC and project audio) and MIDI interfaces/ports (MTC).
- Signal direction (generate/output vs. chase/input) is still an open decision; do not assume one.

## Project file format

Projects use the `.fxtc` extension. A `.fxtc` is a **zip container** holding:

- `project.json` — the full project description: tracks, timecode positions, triggers/cues/events, and
  configuration (timecode format, frame rate, start offset, MTC/LTC settings, device preferences).
- The **audio files** used by the project's tracks, bundled in the archive so a project is self-contained and portable.

Still to be defined with the first implementation: the internal layout (e.g. an `audio/` folder), a schema `version`
field in `project.json`, and how devices are referenced (devices are machine-specific: store a name/matching hint,
not an ID).

## Stack

- Rust + Tauri v2 for the backend (`src-tauri/`).
- React + TypeScript for the UI (`src/`).

This is the standard Tauri v2 layout, scaffolded from `create-tauri-app` (`react-ts` template, npm). The UI in
`src/App.tsx` is still the template demo.

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
  - `cargo fmt --check`, `cargo clippy`, `cargo test` — from `src-tauri/`.

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
