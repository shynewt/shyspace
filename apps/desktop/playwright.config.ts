import { defineConfig, devices } from "@playwright/test";

const PORT = 1430;
// One address for every run, so a second server on the port fails to start
// instead of quietly binding the other IP version and serving another app.
const HOST = "127.0.0.1";
const APP_URL = `http://${HOST}:${PORT}/tests/ui/app.html`;

export default defineConfig({
  testDir: "tests/ui",
  fullyParallel: true,
  use: { baseURL: APP_URL, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
  webServer: { command: `pnpm exec vite --host ${HOST} --port ${PORT} --strictPort`, url: APP_URL },
});
