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
  const name = await item.getByText(LONG_NAME).boundingBox();
  const button = await actions.boundingBox();
  if (!name || !button) throw new Error("sidebar not laid out");
  expect(name.x + name.width).toBeLessThanOrEqual(button.x);
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
  const idle = { peers: 0, last_sync_ms: Date.now(), error: null, removed: false, needs_update: false };
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
