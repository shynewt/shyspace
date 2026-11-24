import { expect, type Page, test } from "@playwright/test";

const PNG = [
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
  0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73,
  69, 78, 68, 174, 66, 96, 130,
];

function settings(page: Page) {
  return page.getByRole("dialog", { name: "Settings" });
}

async function openSection(page: Page, section: "Appearance" | "Workspace") {
  await page.keyboard.press("Control+,");
  await settings(page).getByRole("tab", { name: section }).click();
}

async function workspaceChose(page: Page, theme: string | null, accent: string | null) {
  await page.evaluate(
    ([theme, accent]) => {
      const id = window.fake.session?.workspace_id;
      if (!id) throw new Error("no workspace");
      window.fake.workspaces.receiveLook(id, { theme, accent });
    },
    [theme, accent] as const,
  );
}

const html = (page: Page) => page.locator("html");

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a theme can be chosen, is kept, and Shy is the default", async ({ page }) => {
  await expect(html(page)).not.toHaveAttribute("data-palette");
  await openSection(page, "Appearance");
  const themes = settings(page).getByRole("radiogroup", { name: "Theme" });
  await expect(themes.getByRole("radio", { name: /^Shy/ })).toBeChecked();
  await themes.getByRole("radio", { name: /^Paper/ }).click();
  await expect(html(page)).toHaveAttribute("data-palette", "paper");

  await page.reload();
  await expect(html(page)).toHaveAttribute("data-palette", "paper");
});

test("light or dark is its own choice beside the theme", async ({ page }) => {
  await openSection(page, "Appearance");
  await settings(page).getByRole("radiogroup", { name: "Light or dark" }).getByRole("radio", { name: "Dark" }).click();
  await settings(page).getByRole("radiogroup", { name: "Theme" }).getByRole("radio", { name: /^Moss/ }).click();
  await expect(html(page)).toHaveAttribute("data-theme", "dark");
  await expect(html(page)).toHaveAttribute("data-palette", "moss");
});

test("the workspace's theme and accent show until you pick your own", async ({ page }) => {
  await workspaceChose(page, "moss", "teal");
  await expect(html(page)).toHaveAttribute("data-palette", "moss");
  await expect(html(page)).toHaveAttribute("data-accent", "teal");

  await openSection(page, "Appearance");
  const themes = settings(page).getByRole("radiogroup", { name: "Theme" });
  await expect(themes.getByRole("radio", { name: "Moss, the workspace's default" })).toBeChecked();
  await themes.getByRole("radio", { name: /^Dusk/ }).click();
  await expect(html(page)).toHaveAttribute("data-palette", "dusk");

  // Your own choice stays when the workspace's changes.
  await workspaceChose(page, "fjord", "teal");
  await expect(themes.getByRole("radio", { name: "Fjord, the workspace's default" })).toBeVisible();
  await expect(html(page)).toHaveAttribute("data-palette", "dusk");

  // Picking the workspace's own theme follows it again.
  await themes.getByRole("radio", { name: /^Fjord/ }).click();
  await workspaceChose(page, "paper", "teal");
  await expect(html(page)).toHaveAttribute("data-palette", "paper");
});

test("a theme or accent the app doesn't know shows the default", async ({ page }) => {
  await workspaceChose(page, "neon", "plaid");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(html(page)).not.toHaveAttribute("data-palette");
  await expect(html(page)).not.toHaveAttribute("data-accent");
});

test("the admin sets the workspace's default look for everyone", async ({ page }) => {
  await openSection(page, "Workspace");
  const section = settings(page);
  await section.getByRole("radiogroup", { name: "Default theme" }).getByRole("radio", { name: "Fjord" }).click();
  await section.getByRole("radiogroup", { name: "Default accent colour" }).getByRole("radio", { name: "Teal" }).click();
  await expect
    .poll(() => page.evaluate(() => window.fake.handle("workspace_look", undefined)))
    .toEqual({ theme: "fjord", accent: "teal" });
  await expect(html(page)).toHaveAttribute("data-palette", "fjord");
  await expect(html(page)).toHaveAttribute("data-accent", "teal");
});

test("the admin gives the workspace a picture, and can go back to the letter", async ({ page }) => {
  // The rail shows once there are two workspaces.
  await page.evaluate(() => window.fake.workspaces.add("Team"));
  const railIcon = page.getByRole("navigation", { name: "Workspaces" }).getByRole("button", { name: /^Acme/ });
  await expect(railIcon).toBeVisible();

  await openSection(page, "Workspace");
  await settings(page)
    .getByLabel("Choose a workspace picture")
    .setInputFiles({
      name: "logo.png",
      mimeType: "image/png",
      buffer: Buffer.from(PNG),
    });
  await expect(settings(page).locator("img")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(railIcon.locator("img")).toHaveCount(1);

  await openSection(page, "Workspace");
  await settings(page).getByRole("button", { name: "Use the letter" }).click();
  await expect(settings(page).locator("img")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(railIcon.locator("img")).toHaveCount(0);
});

test.describe("as a member", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("?start=member");
    await expect(page.getByLabel("Message #general")).toBeVisible();
  });

  test("members see the workspace's look but can't change it", async ({ page }) => {
    await openSection(page, "Workspace");
    const section = settings(page);
    await expect(section).toContainText("Managed by Ada");
    await expect(section.getByRole("radiogroup", { name: "Default theme" }).getByRole("radio").first()).toBeDisabled();
    await expect(section.getByRole("button", { name: "Use the letter" })).toHaveCount(0);
    await expect(section.getByLabel("Choose a workspace picture")).toHaveCount(0);
  });
});
