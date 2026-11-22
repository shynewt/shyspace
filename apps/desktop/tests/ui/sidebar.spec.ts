import { expect, test } from "@playwright/test";
import { answerWith, chooseInSubmenu } from "./support";

const LONG_NAME = "customer-feedback-and-support-escalations";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("navigation", { name: "Workspace" }).waitFor();
});

test("a channel's actions never cover its name", async ({ page }) => {
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill(LONG_NAME);
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel(`Message #${LONG_NAME}`)).toBeVisible();

  const item = page
    .getByRole("navigation", { name: "Workspace" })
    .getByRole("button", { name: LONG_NAME, exact: true });
  await item.hover();
  const actions = page.getByRole("button", { name: `Actions for #${LONG_NAME}` });
  await expect(actions).toHaveCSS("opacity", "1");
  // The actions grow into place, so measure both at once until they settle.
  const name = item.getByText(LONG_NAME);
  await expect
    .poll(async () => {
      const [nameBox, buttonBox] = await Promise.all([name.boundingBox(), actions.boundingBox()]);
      if (!nameBox || !buttonBox) throw new Error("sidebar not laid out");
      return buttonBox.x - (nameBox.x + nameBox.width);
    })
    .toBeGreaterThanOrEqual(0);
});

test("a channel's alert level doesn't look like a mention", async ({ page }) => {
  const general = page.getByRole("button", { name: "general", exact: true });
  await general.hover();
  await page.getByRole("button", { name: "Actions for #general" }).click();
  await chooseInSubmenu(page, "Notifications", "Mentions only");

  const item = page.getByRole("button", { name: "general, mentions only" });
  await expect(item).toBeVisible();
  await expect(item.locator(".lucide-at-sign")).toHaveCount(0);
});

test("syncing through the bucket alone looks calm, not like a warning", async ({ page }) => {
  const idle = {
    peers: 0,
    last_sync_ms: Date.now(),
    error: null,
    removed: false,
    needs_update: false,
    needs_update_from: [],
  };
  await answerWith(page, "status", idle, { kind: "sync" });

  const label = page.getByText("Synced via bucket");
  await expect(label).toBeVisible();
  const [dot, warning] = await label.locator("xpath=preceding-sibling::span/span[last()]").evaluate((element) => {
    const probe = document.createElement("span");
    probe.style.color = "var(--warning)";
    document.body.append(probe);
    const colors = [getComputedStyle(element).backgroundColor, getComputedStyle(probe).color];
    probe.remove();
    return colors;
  });
  expect(dot).not.toBe(warning);
});

test("switching views while one fades in on a slow machine still shows the one chosen", async ({ page }) => {
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Inbox", level: 1 })).toBeVisible();

  // Frames take 60 ms, as on a busy machine, and each switch comes a frame after the last view appears.
  const shown = await page.evaluate(async () => {
    const slowFrame = () =>
      new Promise((resolve) =>
        requestAnimationFrame(() => {
          const end = performance.now() + 60;
          while (performance.now() < end) {}
          resolve(null);
        }),
      );
    const heading = () => document.querySelector("main h1")?.textContent;
    const headings: (string | null | undefined)[] = [];
    for (let round = 0; round < 12; round++) {
      for (const name of ["Later", "Inbox"]) {
        const button = [...document.querySelectorAll("nav button")].find((item) => item.textContent === name);
        if (!(button instanceof HTMLButtonElement)) throw new Error(`no ${name} button`);
        button.click();
        for (let frame = 0; frame < 40 && heading() !== name; frame++) await slowFrame();
        headings.push(heading());
        await slowFrame();
      }
    }
    return headings;
  });
  expect(shown).toEqual(Array.from({ length: 12 }, () => ["Later", "Inbox"]).flat());
});
