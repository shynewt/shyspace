import { expect, type Page, test } from "@playwright/test";
import { chooseInSubmenu } from "./support";

async function sectionMenu(page: Page, section: string) {
  await page.getByRole("region", { name: section }).hover();
  await page.getByRole("button", { name: `Actions for the ${section} section` }).click();
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
});

test("channels can be grouped, collapsed and given back", async ({ page }) => {
  await sectionMenu(page, "Channels");
  await page.getByRole("menuitem", { name: "New section" }).click();
  await page.getByLabel("Section name").fill("Projects");
  await page.getByLabel("Section name").press("Enter");
  const projects = page.getByRole("region", { name: "Projects" });
  await expect(projects).toBeVisible();

  await page.getByRole("button", { name: "random", exact: true }).click({ button: "right" });
  await chooseInSubmenu(page, "Move to", "Projects");
  await expect(projects.getByRole("button", { name: "random", exact: true })).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Channels" }).getByRole("button", { name: "random", exact: true }),
  ).toHaveCount(0);

  await projects.getByRole("button", { name: "Projects", exact: true }).click();
  await expect(projects.getByRole("button", { name: "random", exact: true })).toHaveCount(0);
  await projects.getByRole("button", { name: "Projects", exact: true }).click();

  await sectionMenu(page, "Projects");
  await page.getByRole("menuitem", { name: "Delete section" }).click();
  await page.getByRole("button", { name: "Delete section" }).click();
  await expect(projects).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Channels" }).getByRole("button", { name: "random", exact: true }),
  ).toBeVisible();
});

test("a section can sort by recent activity", async ({ page }) => {
  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "news"));
  const channels = page.getByRole("region", { name: "Channels" });
  await expect(channels.getByRole("button", { name: /^(general|random)/ }).first()).toHaveAccessibleName(/general/);

  await sectionMenu(page, "Channels");
  await chooseInSubmenu(page, "Sort", "Recent activity");
  await expect(channels.getByRole("button", { name: /^(general|random)/ }).first()).toHaveAccessibleName(/random/);
});

test("showing unread channels only hides sections with none", async ({ page }) => {
  await sectionMenu(page, "Channels");
  await page.getByRole("menuitem", { name: "New section" }).click();
  await page.getByLabel("Section name").fill("Projects");
  await page.getByLabel("Section name").press("Enter");
  await page.getByRole("button", { name: "random", exact: true }).click({ button: "right" });
  await chooseInSubmenu(page, "Move to", "Projects");
  await page.getByRole("button", { name: "general", exact: true }).click();
  const projects = page.getByRole("region", { name: "Projects" });
  await expect(projects.getByRole("button", { name: "random", exact: true })).toBeVisible();

  await page.getByRole("region", { name: "Channels" }).hover();
  await page.getByRole("button", { name: "Show unread channels only" }).click();
  await expect(projects).toHaveCount(0);

  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "news"));
  await expect(projects.getByRole("button", { name: "random, 1 unread" })).toBeVisible();
});
