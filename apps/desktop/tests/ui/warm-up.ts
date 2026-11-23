/**
 * Loads the app once before any test. On first use the dev server bundles
 * the dependencies and compiles every module, which on a CI machine with an
 * empty cache takes longer than a test's checks wait: the first tests of each
 * worker would fail for reasons that have nothing to do with them.
 */
import { chromium, type FullConfig, webkit } from "@playwright/test";

/** Generous: bundling every dependency from cold on a two-core machine. */
const FIRST_LOAD_LIMIT_MS = 120_000;

export default async function warmUp(config: FullConfig) {
  const project = config.projects[0];
  if (!project) return;
  const browser = await (project.use.browserName === "webkit" ? webkit : chromium).launch();
  try {
    const page = await browser.newPage({ baseURL: project.use.baseURL });
    await page.goto("");
    await page.getByLabel("Message #general").waitFor({ timeout: FIRST_LOAD_LIMIT_MS });
  } finally {
    await browser.close();
  }
}
