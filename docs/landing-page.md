# Public site: landing page and docs

The public site is published to GitHub Pages from one workflow, `.github/workflows/pages.yml`:

| URL | Source | Built with |
| --- | --- | --- |
| `https://blad3mak3r.github.io/flux/` | `site/` | Nothing: plain HTML, CSS and JavaScript, copied as is. |
| `https://blad3mak3r.github.io/flux/docs/` | `docs/` | VitePress, from the guides in this folder. |

All paths in `site/` are relative and the docs are built with `base: "/flux/docs/"`, so both work under the `/flux/`
project path. This file, `releasing.md` and `superpowers/` are excluded from the docs site (`srcExclude` in
`docs/.vitepress/config.mts`).

## Publishing

The workflow runs when `site/` or `docs/` change on `main`, or by hand from Actions > Pages > Run workflow. It builds the
docs, copies `site/` and the docs output into one `_site/` folder (docs under `_site/docs/`) and deploys that. Once per
repository: **Settings > Pages > Build and deployment > Source: GitHub Actions**.

The CI `Docs` job builds the docs on every pull request. It fails on a dead link or on a page Vue cannot parse (a bare
`<placeholder>` outside backticks is read as a tag).

## The landing page (`site/`)

The landing introduces the two project types. CUE covers timeline editing and cue operation in Live. VJ covers
launching clips and mixing visuals on the fly. Each section links to its corresponding guide in the docs site.

| File | Holds |
| --- | --- |
| `site/index.html` | The page and English strings, including the mode sections, download links and the inline signal path diagram. |
| `site/style.css` | The landing layout, dark palette, grid background and screenshot frames. |
| `site/site.js` | Spanish translations, the EN/ES selector and the demo clock. |
| `site/img/` | FLUX screenshots and the F symbol. The video guide uses copies of some screenshots in `docs/screenshots/`. |
| `site/fonts/` | Archivo (variable, latin), self-hosted, with its OFL licence. |

The version shown on the page (three `<span data-version>`) is written by `scripts/set-version.mjs` with the rest, so a
release updates it. Do not edit it by hand.

- Check product claims against the README and the guides. The browser timecode demo and signal path diagram explain
  the CUE clock. Screenshots show FLUX running sample shows.
- Every visible string has an English default in the HTML and a Spanish entry in `site/site.js`, keyed by `data-i18n`.
  Add both when adding text.
- If the site moves to a custom domain, update the `og:url` and `og:image` addresses in `site/index.html` and `base` in
  the docs config.

## The docs site (`docs/`)

VitePress lives inside `docs/` as its own npm package (`docs/package.json`), so the app's dependencies and lockfile are
untouched. The markdown stays in `docs/*.md`, which is also what the README links to.

| File | Holds |
| --- | --- |
| `docs/.vitepress/config.mts` | Title, `base`, the dark-only setting, the excluded files, the nav and the sidebar. |
| `docs/.vitepress/theme/custom.css` | The desktop Bench Instrument tokens mapped onto VitePress. |
| `docs/index.md` | The docs home. |
| `docs/public/` | The logo and the Archivo font. |

To add a guide: write `docs/<name>.md` starting with a `# Title`, link it from `docs/index.md` and add it to the sidebar
in the config. Link other guides with a relative `other.md` link; VitePress turns it into the site address.

## Screenshots

The CUE screenshots show the desktop app (`npm run tauri dev`) running the sample show *Summer tour*. It has two
timelines (Opening and Anthem) in a setlist, audio and video tracks, operator notes, MIDI Show Control and OSC cues,
and two projector screens blended with a soft edge, one with a mask. The song and abstract visuals were generated
for the sample show.

They are taken at twice the pixel density (on Linux, `GDK_SCALE=2`, with the editor at 1440 × 900 points, so 2880 ×
1800 pixels) and saved as JPEG at full size plus a half-size copy for `srcset` (the mapping editor is cropped to the
Preferences dialog, 1560 pixels wide; the output windows are captured at their own size and put side by side).
Use sample media you can redistribute and match the pixel density when adding a screenshot.

| File | Shows |
| --- | --- |
| `edit-*.jpg` | The Edit view while playing. Also the `og:image`. |
| `live-*.jpg` | Live with the setlist prepared. |
| `outputs-*.jpg` | Two projector windows side by side. |
| `mapping-*.jpg` | Preferences > Outputs > Video with a slice selected. |
| `vj-test-card.webp` | VJ LED slice test output. A WebP copy of `docs/screenshots/test-card-led.png`. |

## Preview locally

```bash
# landing page (from the repository root)
python -m http.server 4173 --directory site

# docs (once: npm --prefix docs ci)
npm run docs:dev
```

Then open <http://127.0.0.1:4173/> for the landing page and <http://localhost:5174/flux/docs/> for the docs.
