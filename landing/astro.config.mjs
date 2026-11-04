import { defineConfig } from "astro/config";

// Published at https://fxtc.blademaker.tv (Cloudflare Pages).
export default defineConfig({
  site: "https://fxtc.blademaker.tv",
  trailingSlash: "always",
  i18n: {
    defaultLocale: "en",
    locales: ["en", "es"],
    routing: { prefixDefaultLocale: false },
  },
});
