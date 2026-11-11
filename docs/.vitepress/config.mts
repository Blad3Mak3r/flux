import { defineConfig } from "vitepress";

// The docs site is published next to the landing page: https://blad3mak3r.github.io/flux/docs/
// The markdown stays in docs/ so the README and GitHub keep linking to the same files.
// A new user-facing guide needs a sidebar entry below.
export default defineConfig({
  title: "FLUX Docs",
  description: "Guides and reference for FLUX: setlists, remote control, OSC, audio routing, the project file and timecode signals.",
  lang: "en-US",
  base: "/flux/docs/",
  cleanUrls: true,
  appearance: "force-dark", // dark only, as DESIGN.md requires
  srcExclude: ["releasing.md", "landing-page.md", "superpowers/**", "README.md"],
  head: [
    ["link", { rel: "icon", type: "image/svg+xml", href: "/flux/docs/flux-symbol.svg" }],
    ["link", { rel: "preload", href: "/flux/docs/fonts/archivo-latin-wdth-normal.woff2", as: "font", type: "font/woff2", crossorigin: "" }],
    ["meta", { name: "theme-color", content: "#1f2328" }],
  ],
  themeConfig: {
    logo: "/flux-symbol.svg",
    siteTitle: "FLUX",
    nav: [
      { text: "Use FLUX", link: "/setlists", activeMatch: "/(setlists|remote-control|audio-routing|voice-operator|diagnostics)" },
      { text: "Integrate", link: "/osc-api", activeMatch: "/(osc-api|fxtc-format|timecode-signals)" },
      { text: "Download", link: "https://github.com/Blad3Mak3r/flux/releases/latest" },
    ],
    sidebar: [
      {
        text: "Use FLUX",
        items: [
          { text: "Setlists and show operation", link: "/setlists" },
          { text: "Remote control", link: "/remote-control" },
          { text: "Per-track audio outputs", link: "/audio-routing" },
          { text: "Voice operator", link: "/voice-operator" },
          { text: "Logging and diagnostics", link: "/diagnostics" },
        ],
      },
      {
        text: "Integrate",
        items: [
          { text: "OSC operation API", link: "/osc-api" },
          { text: "Project file format", link: "/fxtc-format" },
          { text: "Timecode signals", link: "/timecode-signals" },
        ],
      },
    ],
    socialLinks: [{ icon: "github", link: "https://github.com/Blad3Mak3r/flux" }],
    search: { provider: "local" },
    editLink: {
      pattern: "https://github.com/Blad3Mak3r/flux/edit/main/docs/:path",
      text: "Edit this page on GitHub",
    },
    outline: { level: [2, 3], label: "On this page" },
    docFooter: { prev: "Previous", next: "Next" },
    footer: { message: "FLUX is in early development and licensed under the GNU General Public License v3.0." },
  },
});
