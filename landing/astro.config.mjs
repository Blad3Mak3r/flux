import { defineConfig } from "astro/config";

// Published at https://blad3mak3r.github.io/flux-timecode/ (GitHub Pages, project site).
export default defineConfig({
  site: "https://blad3mak3r.github.io",
  base: "/flux-timecode",
  trailingSlash: "always",
  i18n: {
    defaultLocale: "en",
    locales: ["en", "es"],
    routing: { prefixDefaultLocale: false },
  },
});
