import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Component tests (`npm run test:ui`). The node --test files (`npm test`) are *.test.mjs and are not picked up here.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.ui.test.tsx"],
    setupFiles: ["src/test/setup.ts"],
  },
});
