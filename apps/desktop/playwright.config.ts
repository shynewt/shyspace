import { defineConfig, devices, type Project } from "@playwright/test";

const PORT = 1430;
// One address for every run, so a second server on the port fails to start
// instead of quietly binding the other IP version and serving another app.
const HOST = "127.0.0.1";
const APP_URL = `http://${HOST}:${PORT}/tests/ui/app.html`;
const VIEWPORT = { width: 1280, height: 800 };
// Playwright's WebKit poses as Safari on a Mac; the app runs in WebKitGTK, which says Linux.
const WEBKITGTK_USER_AGENT =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

const browsers: Project[] = [
  { name: "chromium", use: { ...devices["Desktop Chrome"], viewport: VIEWPORT } },
  { name: "webkit", use: { browserName: "webkit", viewport: VIEWPORT, userAgent: WEBKITGTK_USER_AGENT } },
];

// Chromium alone by default: Playwright's WebKit needs Ubuntu's libraries (see docs/development.md).
const chosen = (process.env.UI_TEST_BROWSERS ?? "chromium").split(",");
const unknown = chosen.filter((name) => !browsers.some((browser) => browser.name === name));
if (unknown.length > 0) throw new Error(`UI_TEST_BROWSERS names unknown browsers: ${unknown.join(", ")}`);

export default defineConfig({
  testDir: "tests/ui",
  fullyParallel: true,
  use: { baseURL: APP_URL, trace: "retain-on-failure" },
  projects: browsers.filter((browser) => chosen.includes(browser.name ?? "")),
  webServer: { command: `pnpm exec vite --host ${HOST} --port ${PORT} --strictPort`, url: APP_URL },
});
