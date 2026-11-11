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

| File | Holds |
| --- | --- |
| `site/index.html` | The page, in English. The drawn timeline in Fig. 3 and the signal path in Fig. 2 are inline SVG. |
| `site/style.css` | The Bench Instrument look from [DESIGN.md](https://github.com/Blad3Mak3r/flux/blob/main/DESIGN.md), dark only. |
| `site/site.js` | The EN/ES selector (Spanish strings live here), the section index and the Fig. 1 demo clock. |
| `site/img/` | The real Live screenshots (copies of `docs/screenshots/`) and the F symbol. |
| `site/fonts/` | Archivo (variable, latin), self-hosted, with its OFL licence. |

The version shown on the page (three `<span data-version>`) is written by `scripts/set-version.mjs` with the rest, so a
release updates it. Do not edit it by hand.

- The page makes no claim the README or the docs do not make. Not built yet: video and Art-Net cues, chasing an
  external timecode, macOS builds.
- Fig. 1 (the demo clock), Fig. 2 (the signal path) and Fig. 3 (the Edit view) are drawn for the page and say so. Fig. 4 is
  a real screenshot of the app rendered with sample show data. Replace Fig. 3 with a real Edit screenshot when one exists.
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
| `docs/.vitepress/theme/custom.css` | The Bench Instrument tokens (same values as `site/style.css`) mapped onto VitePress. |
| `docs/index.md` | The docs home. |
| `docs/public/` | The logo and the Archivo font. |

To add a guide: write `docs/<name>.md` starting with a `# Title`, link it from `docs/index.md` and add it to the sidebar
in the config. Link other guides with a relative `other.md` link; VitePress turns it into the site address.

## Preview locally

```bash
# landing page
cd site
python -m http.server 4173

# docs (once: npm --prefix docs ci)
npm run docs:dev
```

Then open <http://127.0.0.1:4173/> for the landing page and <http://localhost:5174/flux/docs/> for the docs.
