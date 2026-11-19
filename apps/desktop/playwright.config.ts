import { defineConfig, devices } from "@playwright/test";

const PORT = 1430;
const APP_URL = `http://localhost:${PORT}/tests/ui/app.html`;

export default defineConfig({
  testDir: "tests/ui",
  fullyParallel: true,
  use: { baseURL: APP_URL, trace: "retain-on-failure" },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
  webServer: { command: `pnpm exec vite --port ${PORT} --strictPort`, url: APP_URL },
});
