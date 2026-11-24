import { defineConfig } from "@playwright/test";
import config from "./playwright.config";

// Keep React's development-only StrictMode lifecycle checks without opening the
// entire visual suite through Vite's module/HMR connections on Windows runners.
export default defineConfig(config, {
  grep: /slow document refresh|final document refresh|leaving .+ cancels every pending artifact GET/,
  use: { ...config.use, baseURL: "http://127.0.0.1:4174" },
  outputDir: "test-results-dev",
  reporter: [["list"], ["html", { open: "never", outputFolder: "playwright-report-dev" }]],
  webServer: { command: "npm run dev -- --host 127.0.0.1 --port 4174 --strictPort", url: "http://127.0.0.1:4174", reuseExistingServer: false },
});
