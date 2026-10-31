import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  updateSnapshots: "none",
  timeout: 30_000,
  expect: { timeout: 7000, toHaveScreenshot: { animations: "disabled", maxDiffPixelRatio: 0.001 } },
  use: { baseURL: "http://127.0.0.1:4173", viewport: { width: 1440, height: 1000 }, locale: "ru-RU", timezoneId: "Europe/Moscow", colorScheme: "dark", reducedMotion: "reduce", trace: "retain-on-failure" },
  reporter: [["list"], ["html", { open: "never" }]],
  webServer: { command: "npm run dev -- --host 127.0.0.1 --port 4173 --strictPort", url: "http://127.0.0.1:4173", reuseExistingServer: false },
});
